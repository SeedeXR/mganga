// GUI-side client for the elevated broker.
//
// launch() fires the one UAC prompt via ShellExecuteW("runas"), connect()
// opens the broker's named pipe (retrying while the broker starts up), and
// call() does one JSON request/response round trip.
//
// Errors are returned as short machine-readable codes ("uac-declined",
// "broker-missing", "connect-timeout", ...) so the frontend can translate
// them into friendly language.

use serde_json::{json, Value};
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::time::Duration;

use windows::core::HSTRING;
use windows::Win32::Foundation::GetLastError;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;

const PIPE_PATH: &str = r"\\.\pipe\mganga-broker";

pub struct BrokerConn {
    reader: BufReader<File>,
    writer: File,
}

/// Launch mganga-broker.exe elevated. One UAC prompt. The broker exe lives
/// next to our own exe (cargo puts both bins in the same target dir).
pub fn launch() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("io: {e}"))?;
    let broker = exe
        .parent()
        .ok_or("broker-missing")?
        .join("mganga-broker.exe");
    if !broker.exists() {
        return Err("broker-missing".into());
    }

    // Pass our PID so the broker can exit if we die.
    let result = unsafe {
        ShellExecuteW(
            None,
            &HSTRING::from("runas"),
            &HSTRING::from(broker.as_os_str()),
            &HSTRING::from(std::process::id().to_string()),
            None,
            SW_HIDE,
        )
    };
    // ShellExecuteW returns a fake HINSTANCE; values <= 32 mean failure.
    if result.0 as isize <= 32 {
        let code = unsafe { GetLastError() }.0;
        if code == 1223 {
            // ERROR_CANCELLED: the user said no to the UAC prompt.
            return Err("uac-declined".into());
        }
        return Err(format!("launch-failed: code {code}"));
    }
    Ok(())
}

/// Connect to the broker's pipe, retrying while it boots (UAC + process start
/// take a moment). Gives up after ~10 seconds.
pub fn connect() -> Result<BrokerConn, String> {
    for _ in 0..50 {
        match File::options().read(true).write(true).open(PIPE_PATH) {
            Ok(file) => {
                let reader = BufReader::new(file.try_clone().map_err(|e| format!("io: {e}"))?);
                return Ok(BrokerConn {
                    reader,
                    writer: file,
                });
            }
            Err(_) => std::thread::sleep(Duration::from_millis(200)),
        }
    }
    Err("connect-timeout".into())
}

/// One round trip: send { op, args }, read one JSON line back.
pub fn call(conn: &mut BrokerConn, op: &str, args: Value) -> Result<Value, String> {
    let request = json!({ "op": op, "args": args });
    writeln!(conn.writer, "{request}").map_err(|e| format!("pipe-write: {e}"))?;
    conn.writer.flush().map_err(|e| format!("pipe-write: {e}"))?;

    let mut line = String::new();
    let n = conn
        .reader
        .read_line(&mut line)
        .map_err(|e| format!("pipe-read: {e}"))?;
    if n == 0 {
        return Err("broker-gone".into());
    }
    let response: Value =
        serde_json::from_str(&line).map_err(|e| format!("bad-response: {e}"))?;
    if response["ok"].as_bool() == Some(true) {
        Ok(response["result"].clone())
    } else {
        Err(response["error"]
            .as_str()
            .unwrap_or("unknown broker error")
            .to_string())
    }
}
