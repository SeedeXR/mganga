// Mganga broker. A tiny elevated helper.
//
// The GUI runs unelevated and launches this binary once with the "runas" verb
// (one UAC prompt). We create a named pipe, the GUI connects, and we answer
// JSON requests, one per line:
//   { "op": "...", "args": {...} }  ->  { "ok": true/false, "result": ..., "error": ... }
//
// The broker is the security boundary. It never trusts the caller. Today it
// only proves the channel (ping + one privileged HKLM read); the protected
// list will be enforced here when write ops arrive in Brick 4.
//
// Lifetime: we exit when the GUI's pipe connection drops (EOF), and as a
// safety net we also watch the GUI's process (passed as argv[1]) and exit if
// it dies. No orphaned admin processes.

#![windows_subsystem = "windows"]

// The guard is shared source with the GUI, included directly so the broker
// stays a small std-only binary and enforces the same rules itself.
#[path = "../guard.rs"]
mod guard;
#[path = "../proc_control.rs"]
mod proc_control;

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::os::windows::io::FromRawHandle;

use windows::core::{HSTRING, PWSTR};
use windows::Win32::Foundation::{CloseHandle, LocalFree, HANDLE, HLOCAL, INVALID_HANDLE_VALUE};
use windows::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{
    GetTokenInformation, TokenElevation, TokenUser, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES,
    TOKEN_ELEVATION, TOKEN_QUERY, TOKEN_USER,
};
use windows::Win32::Storage::FileSystem::PIPE_ACCESS_DUPLEX;
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_TYPE_BYTE, PIPE_WAIT,
};
use windows::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, WaitForSingleObject, INFINITE,
    PROCESS_SYNCHRONIZE,
};

const PIPE_NAME: &str = r"\\.\pipe\mganga-broker";

fn main() {
    // Safety net: if the GUI process dies without closing the pipe, exit too.
    if let Some(pid) = std::env::args().nth(1).and_then(|s| s.parse::<u32>().ok()) {
        watch_parent(pid);
    }

    let pipe = match create_pipe() {
        Ok(p) => p,
        Err(_) => std::process::exit(1),
    };

    // Wait for the GUI to connect, then convert the raw handle into a File so
    // we can use plain buffered line IO on it.
    unsafe {
        if ConnectNamedPipe(pipe, None).is_err() {
            let _ = CloseHandle(pipe);
            std::process::exit(1);
        }
    }
    let file = unsafe { std::fs::File::from_raw_handle(pipe.0 as _) };
    serve(file);
}

/// One JSON request per line in, one JSON response per line out.
/// EOF means the GUI closed; we are done.
fn serve(file: std::fs::File) {
    let mut writer = match file.try_clone() {
        Ok(f) => f,
        Err(_) => return,
    };
    let mut reader = BufReader::new(file);
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => return, // GUI closed the pipe
            Ok(_) => {}
        }
        let response = handle_request(&line);
        if writeln!(writer, "{response}").is_err() {
            return;
        }
        let _ = writer.flush();
    }
}

fn handle_request(raw: &str) -> Value {
    let req: Value = match serde_json::from_str(raw) {
        Ok(v) => v,
        Err(e) => return json!({ "ok": false, "error": format!("bad request: {e}") }),
    };
    let op = req["op"].as_str().unwrap_or("");
    let args = &req["args"];

    match op {
        // Proof of life. Also reports whether this process really is elevated,
        // read from our own token, so the GUI can show it.
        "ping" => json!({
            "ok": true,
            "result": { "msg": "pong from broker", "elevated": is_elevated() }
        }),

        // The privileged no-op from the build plan: read one HKLM value.
        "read_hklm" => {
            let path = args["path"].as_str().unwrap_or("");
            let name = args["name"].as_str().unwrap_or("");
            match read_hklm(path, name) {
                Ok(v) => json!({ "ok": true, "result": v }),
                Err(e) => json!({ "ok": false, "error": e }),
            }
        }

        // Brick 4: flip one HKLM StartupApproved value. The broker is the
        // security boundary: path whitelist and protected list are checked
        // HERE, regardless of what the GUI already did.
        "set_startup_approved" => {
            let path = args["path"].as_str().unwrap_or("");
            let name = args["name"].as_str().unwrap_or("");
            let enable = args["enable"].as_bool().unwrap_or(false);
            match write_approved(path, name, Some(guard::make_approved_bytes(enable))) {
                Ok(old_hex) => json!({ "ok": true, "result": { "old_value_hex": old_hex } }),
                Err(e) => json!({ "ok": false, "error": e }),
            }
        }

        // Restore exact old bytes (or delete if the value did not exist).
        "restore_startup_approved" => {
            let path = args["path"].as_str().unwrap_or("");
            let name = args["name"].as_str().unwrap_or("");
            let bytes = match &args["old_value_hex"] {
                Value::String(hex) => match guard::from_hex(hex) {
                    Some(b) => Some(b),
                    None => return json!({ "ok": false, "error": "bad hex" }),
                },
                _ => None,
            };
            match write_approved(path, name, bytes) {
                Ok(old_hex) => json!({ "ok": true, "result": { "old_value_hex": old_hex } }),
                Err(e) => json!({ "ok": false, "error": e }),
            }
        }

        // Brick 6: act on processes the GUI could not open (other users,
        // elevated). The protected check runs inside each proc_control call,
        // against a name the broker resolves itself.
        "process_action" => {
            let action = args["action"].as_str().unwrap_or("");
            let pids: Vec<u32> = args["pids"]
                .as_array()
                .map(|a| a.iter().filter_map(|v| v.as_u64()).map(|v| v as u32).collect())
                .unwrap_or_default();
            let mut ok = 0u32;
            let mut first_error: Option<String> = None;
            for pid in pids {
                let result = match action {
                    "throttle" => proc_control::set_efficiency(pid, true),
                    "unthrottle" => proc_control::set_efficiency(pid, false),
                    "suspend" => proc_control::suspend(pid),
                    "resume" => proc_control::resume(pid),
                    "kill" => proc_control::kill(pid),
                    _ => Err("bad-action".into()),
                };
                match result {
                    Ok(()) => ok += 1,
                    Err(e) => {
                        if e == "protected" {
                            return json!({ "ok": false, "error": "protected" });
                        }
                        first_error.get_or_insert(e);
                    }
                }
            }
            json!({ "ok": true, "result": { "ok_count": ok, "error": first_error } })
        }

        other => json!({ "ok": false, "error": format!("unknown op: {other}") }),
    }
}

/// HKLM StartupApproved write. Some(bytes) sets, None deletes. Returns the
/// previous bytes as hex for the audit trail.
fn write_approved(
    approved_path: &str,
    value_name: &str,
    new_bytes: Option<Vec<u8>>,
) -> Result<Option<String>, String> {
    use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ, KEY_WRITE, REG_BINARY};
    use winreg::{RegKey, RegValue};

    if !guard::is_allowed_approved_path(approved_path) {
        return Err("bad-path".into());
    }
    if guard::is_protected_autostart(value_name) {
        return Err("protected".into());
    }

    let (key, _) = RegKey::predef(HKEY_LOCAL_MACHINE)
        .create_subkey_with_flags(approved_path, KEY_READ | KEY_WRITE)
        .map_err(|e| format!("open HKLM\\{approved_path}: {e}"))?;

    let old = key
        .get_raw_value(value_name)
        .ok()
        .map(|v| guard::to_hex(&v.bytes));

    match new_bytes {
        Some(bytes) => key
            .set_raw_value(
                value_name,
                &RegValue {
                    bytes,
                    vtype: REG_BINARY,
                },
            )
            .map_err(|e| format!("write {value_name}: {e}"))?,
        None => {
            if old.is_some() {
                key.delete_value(value_name)
                    .map_err(|e| format!("delete {value_name}: {e}"))?;
            }
        }
    }
    Ok(old)
}

fn read_hklm(path: &str, name: &str) -> Result<String, String> {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;
    let key = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey(path)
        .map_err(|e| format!("open HKLM\\{path}: {e}"))?;
    key.get_value::<String, _>(name)
        .map_err(|e| format!("read {name}: {e}"))
}

/// Create the named pipe with a security descriptor that lets the launching
/// user's unelevated GUI connect. A pipe created by an elevated process is not
/// writable by a non-elevated one under the default ACL, so we grant
/// read+write to our own user SID explicitly (plus SYSTEM and Administrators).
fn create_pipe() -> Result<HANDLE, String> {
    let sid = current_user_sid()?;
    let sddl = format!("D:P(A;;GRGW;;;{sid})(A;;GA;;;SY)(A;;GA;;;BA)");

    unsafe {
        let mut sd = PSECURITY_DESCRIPTOR::default();
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            &HSTRING::from(sddl.as_str()),
            SDDL_REVISION_1,
            &mut sd,
            None,
        )
        .map_err(|e| format!("security descriptor: {e}"))?;

        let sa = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: sd.0,
            bInheritHandle: false.into(),
        };

        let handle = CreateNamedPipeW(
            &HSTRING::from(PIPE_NAME),
            PIPE_ACCESS_DUPLEX,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
            1,          // one instance, one client: the GUI
            64 * 1024,  // out buffer
            64 * 1024,  // in buffer
            0,          // default timeout
            Some(&sa),
        );
        let _ = LocalFree(Some(HLOCAL(sd.0)));

        if handle == INVALID_HANDLE_VALUE {
            return Err("CreateNamedPipeW failed".into());
        }
        Ok(handle)
    }
}

/// The SID of the user this broker runs as, as a string like S-1-5-21-...
/// Elevation does not change the user, so this is also the GUI's user.
fn current_user_sid() -> Result<String, String> {
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token)
            .map_err(|e| format!("open token: {e}"))?;

        let mut len = 0u32;
        let _ = GetTokenInformation(token, TokenUser, None, 0, &mut len);
        let mut buf = vec![0u8; len as usize];
        GetTokenInformation(
            token,
            TokenUser,
            Some(buf.as_mut_ptr() as *mut _),
            len,
            &mut len,
        )
        .map_err(|e| format!("token user: {e}"))?;
        let _ = CloseHandle(token);

        let token_user = &*(buf.as_ptr() as *const TOKEN_USER);
        let mut sid_str = PWSTR::null();
        ConvertSidToStringSidW(token_user.User.Sid, &mut sid_str)
            .map_err(|e| format!("sid to string: {e}"))?;
        let out = sid_str
            .to_string()
            .map_err(|e| format!("sid utf16: {e}"))?;
        let _ = LocalFree(Some(HLOCAL(sid_str.0 as _)));
        Ok(out)
    }
}

/// True if our own token says we are elevated.
fn is_elevated() -> bool {
    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION::default();
        let mut len = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut _ as *mut _),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        )
        .is_ok();
        let _ = CloseHandle(token);
        ok && elevation.TokenIsElevated != 0
    }
}

/// Exit when the GUI process (whose PID we got on the command line) ends.
fn watch_parent(pid: u32) {
    std::thread::spawn(move || unsafe {
        if let Ok(handle) = OpenProcess(PROCESS_SYNCHRONIZE, false, pid) {
            WaitForSingleObject(handle, INFINITE);
            std::process::exit(0);
        }
    });
}
