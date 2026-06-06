// Brick 2: the autostart scanner. Read-only.
//
// Windows launches things from four kinds of places (see
// mganga-docs/docs/windows-internals.md section 1):
//   1. Registry Run / RunOnce keys, in HKCU, HKLM, and the 32-bit WOW6432Node mirror
//   2. The Startup folders (per-user and all-users)
//   3. Scheduled tasks with a logon trigger
//   4. Services set to start automatically
//
// The true on/off state of Run entries and Startup-folder items lives in the
// separate StartupApproved key (section 2): missing value means enabled, first
// byte even (0x02) means enabled, odd (0x03) means disabled. The Run entry
// itself is never deleted by a clean disable, which is why we merge the two.
//
// Everything here is best-effort and tolerant: a source we cannot read is
// skipped, not fatal. All reads work unelevated; deeper reads can be routed
// through the broker later if a locked-down machine needs it.

use serde::Serialize;
use std::os::windows::process::CommandExt;
use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
use winreg::{RegKey, HKEY};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Serialize, Clone, Default)]
pub struct AutostartEntry {
    pub name: String,
    /// Human label of the kind of place it starts from.
    pub source: String,
    /// The exact key, folder, task path, or service name.
    pub source_detail: String,
    pub command: String,
    pub publisher: Option<String>,
    pub enabled: bool,
    /// "user" or "machine"
    pub scope: String,
    /// "run" | "folder" | "task" | "service", for grouping in the UI.
    pub kind: String,
    /// Brick 3: "safe-to-disable" | "keep" | "your-call" | "protected".
    pub verdict: String,
    /// The plain-language why behind the verdict. Always present.
    pub reason: String,
    pub category: Option<String>,
    /// Usage evidence from UserAssist: how often the user deliberately opened
    /// this app, and how many days ago the last time was. None when Windows
    /// has no record, which is not evidence of anything.
    pub open_count: Option<u32>,
    pub last_opened_days: Option<u32>,
    /// Brick 4: where the on/off switch for this entry lives, if it has one.
    /// Run-key entries and Startup-folder items are toggleable via
    /// StartupApproved; RunOnce, tasks, and services are not (yet).
    pub toggle: Option<ToggleInfo>,
}

#[derive(Serialize, Clone)]
pub struct ToggleInfo {
    /// "HKCU" (GUI writes directly) or "HKLM" (goes through the broker).
    pub hive: String,
    pub approved_path: String,
    pub value_name: String,
}

pub fn scan() -> Vec<AutostartEntry> {
    let mut entries = Vec::new();
    scan_run_keys(&mut entries);
    scan_startup_folders(&mut entries);
    scan_scheduled_tasks(&mut entries);
    scan_services(&mut entries);

    // Usage evidence first, then every entry gets a verdict and a reason.
    let usage = crate::usage::collect();
    for entry in &mut entries {
        let exe = extract_exe(&entry.command);

        // Folder items match UserAssist by shortcut stem, the rest by exe name.
        let usage_key = if entry.kind == "folder" {
            entry.name.to_lowercase()
        } else {
            exe.as_deref()
                .and_then(|p| std::path::Path::new(p).file_name())
                .map(|f| f.to_string_lossy().to_lowercase())
                .unwrap_or_default()
        };
        if let Some(u) = usage.get(&usage_key) {
            // Windows 11 often leaves the run counter at 0 even though the
            // last-run timestamp updates fine. Count is bonus, time is truth.
            entry.open_count = (u.run_count > 0).then_some(u.run_count);
            entry.last_opened_days = u.last_run_days;
        }

        let judgment = crate::judge::judge(entry, exe.as_deref());
        entry.verdict = judgment.verdict;
        entry.reason = judgment.reason;
        entry.category = judgment.category;
    }

    let kind_order = |k: &str| match k {
        "run" => 0,
        "folder" => 1,
        "task" => 2,
        _ => 3,
    };
    entries.sort_by(|a, b| {
        kind_order(&a.kind)
            .cmp(&kind_order(&b.kind))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    entries
}

// ---------------------------------------------------------------- Run keys

const RUN_PATH: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUNONCE_PATH: &str = r"Software\Microsoft\Windows\CurrentVersion\RunOnce";
const RUN32_PATH: &str = r"Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Run";
const RUNONCE32_PATH: &str = r"Software\WOW6432Node\Microsoft\Windows\CurrentVersion\RunOnce";
const APPROVED_RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
const APPROVED_RUN32: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run32";
const APPROVED_FOLDER: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\StartupFolder";

fn scan_run_keys(out: &mut Vec<AutostartEntry>) {
    // (hive, run key, StartupApproved key in the same hive, scope, label)
    // RunOnce entries are one-shot and have no StartupApproved state.
    let sources: [(HKEY, &str, Option<&str>, &str, &str); 6] = [
        (HKEY_CURRENT_USER, RUN_PATH, Some(APPROVED_RUN), "user", "Run key (user)"),
        (HKEY_CURRENT_USER, RUNONCE_PATH, None, "user", "RunOnce key (user)"),
        (HKEY_LOCAL_MACHINE, RUN_PATH, Some(APPROVED_RUN), "machine", "Run key (machine)"),
        (HKEY_LOCAL_MACHINE, RUNONCE_PATH, None, "machine", "RunOnce key (machine)"),
        (HKEY_LOCAL_MACHINE, RUN32_PATH, Some(APPROVED_RUN32), "machine", "Run key (machine, 32-bit)"),
        (HKEY_LOCAL_MACHINE, RUNONCE32_PATH, None, "machine", "RunOnce key (machine, 32-bit)"),
    ];

    for (hive, path, approved, scope, label) in sources {
        let root = RegKey::predef(hive);
        let key = match root.open_subkey(path) {
            Ok(k) => k,
            Err(_) => continue, // key absent on this machine, fine
        };
        for (name, _value) in key.enum_values().flatten() {
            if name.is_empty() {
                continue; // the unnamed default value
            }
            let command: String = key.get_value(&name).unwrap_or_default();
            let hive_label = if hive == HKEY_CURRENT_USER { "HKCU" } else { "HKLM" };
            let (enabled, toggle) = match approved {
                Some(approved_path) => (
                    approved_state(hive, approved_path, &name),
                    // Never offer a switch the guard would refuse anyway.
                    (!crate::guard::is_protected_autostart(&name)).then(|| ToggleInfo {
                        hive: hive_label.to_string(),
                        approved_path: approved_path.to_string(),
                        value_name: name.clone(),
                    }),
                ),
                None => (true, None), // RunOnce has no on/off state
            };
            out.push(AutostartEntry {
                publisher: extract_exe(&command).and_then(|p| file_publisher(&p)),
                name,
                source: label.to_string(),
                source_detail: format!("{hive_label}\\{path}"),
                command,
                enabled,
                toggle,
                scope: scope.to_string(),
                kind: "run".to_string(),
                ..Default::default()
            });
        }
    }
}

/// True state from StartupApproved: missing value means enabled, first byte
/// even (0x02) means enabled, odd (0x03) means disabled.
fn approved_state(hive: HKEY, approved_path: &str, value_name: &str) -> bool {
    match RegKey::predef(hive)
        .open_subkey(approved_path)
        .and_then(|k| k.get_raw_value(value_name))
    {
        Ok(raw) => raw.bytes.first().map(|b| b & 1 == 0).unwrap_or(true),
        Err(_) => true,
    }
}

// --------------------------------------------------------- Startup folders

fn scan_startup_folders(out: &mut Vec<AutostartEntry>) {
    let folders = [
        (std::env::var("APPDATA").ok(), HKEY_CURRENT_USER, "user", "Startup folder (user)"),
        (std::env::var("PROGRAMDATA").ok(), HKEY_LOCAL_MACHINE, "machine", "Startup folder (all users)"),
    ];
    for (base, hive, scope, label) in folders {
        let Some(base) = base else { continue };
        let dir = std::path::Path::new(&base).join(r"Microsoft\Windows\Start Menu\Programs\Startup");
        let Ok(read) = std::fs::read_dir(&dir) else { continue };
        for entry in read.flatten() {
            let path = entry.path();
            let file_name = entry.file_name().to_string_lossy().to_string();
            if file_name.eq_ignore_ascii_case("desktop.ini") {
                continue;
            }
            let name = path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| file_name.clone());
            out.push(AutostartEntry {
                name,
                source: label.to_string(),
                source_detail: dir.to_string_lossy().to_string(),
                command: path.to_string_lossy().to_string(),
                publisher: None, // shortcuts carry no version info; resolve later if needed
                enabled: approved_state(hive, APPROVED_FOLDER, &file_name),
                toggle: (!crate::guard::is_protected_autostart(&file_name)).then(|| ToggleInfo {
                    hive: if hive == HKEY_CURRENT_USER { "HKCU" } else { "HKLM" }.to_string(),
                    approved_path: APPROVED_FOLDER.to_string(),
                    value_name: file_name.clone(),
                }),
                scope: scope.to_string(),
                kind: "folder".to_string(),
                ..Default::default()
            });
        }
    }
}

// --------------------------------------------------------- Scheduled tasks

/// First pass per the internals doc: parse `schtasks /query /v /fo CSV` and
/// keep rows whose schedule type is a logon trigger. The verbose CSV repeats
/// its header row per task folder, so those are filtered out.
fn scan_scheduled_tasks(out: &mut Vec<AutostartEntry>) {
    let output = match std::process::Command::new("schtasks")
        .args(["/query", "/v", "/fo", "CSV"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
    {
        Ok(o) if o.status.success() => o,
        _ => return,
    };

    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .from_reader(output.stdout.as_slice());

    let headers = match reader.headers() {
        Ok(h) => h.clone(),
        Err(_) => return,
    };
    let col = |want: &str| headers.iter().position(|h| h == want);
    let (Some(c_name), Some(c_run), Some(c_type), Some(c_state)) = (
        col("TaskName"),
        col("Task To Run"),
        col("Schedule Type"),
        col("Scheduled Task State"),
    ) else {
        return;
    };

    for record in reader.records().flatten() {
        let task_name = record.get(c_name).unwrap_or("");
        if task_name.is_empty() || task_name == "TaskName" {
            continue; // repeated header row
        }
        let schedule_type = record.get(c_type).unwrap_or("");
        if !schedule_type.to_lowercase().contains("logon") {
            continue;
        }
        let command = record.get(c_run).unwrap_or("").trim().to_string();
        let state = record.get(c_state).unwrap_or("");
        let name = task_name.rsplit('\\').next().unwrap_or(task_name).to_string();

        out.push(AutostartEntry {
            publisher: extract_exe(&command).and_then(|p| file_publisher(&p)),
            name,
            source: "Scheduled task (at logon)".to_string(),
            source_detail: task_name.to_string(),
            command,
            enabled: state.eq_ignore_ascii_case("Enabled"),
            scope: "machine".to_string(),
            kind: "task".to_string(),
            ..Default::default()
        });
    }
}

// ----------------------------------------------------------------- Services

/// Services set to start automatically, read straight from the registry.
/// Start == 2 is Automatic; Type & 0x30 keeps real Win32 services and drops
/// kernel drivers. DelayedAutostart == 1 marks Automatic (Delayed).
fn scan_services(out: &mut Vec<AutostartEntry>) {
    let services = match RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey(r"SYSTEM\CurrentControlSet\Services")
    {
        Ok(k) => k,
        Err(_) => return,
    };

    for key_name in services.enum_keys().flatten() {
        let Ok(key) = services.open_subkey(&key_name) else { continue };
        let start: u32 = key.get_value("Start").unwrap_or(99);
        if start != 2 {
            continue;
        }
        let service_type: u32 = key.get_value("Type").unwrap_or(0);
        if service_type & 0x30 == 0 {
            continue;
        }
        let delayed: u32 = key.get_value("DelayedAutostart").unwrap_or(0);
        let display: String = key.get_value("DisplayName").unwrap_or_default();
        // DisplayName is often a resource reference like "@%SystemRoot%\...";
        // fall back to the key name rather than show that.
        let name = if display.is_empty() || display.starts_with('@') {
            key_name.clone()
        } else {
            display
        };
        let command: String = key.get_value("ImagePath").unwrap_or_default();

        out.push(AutostartEntry {
            publisher: extract_exe(&command).and_then(|p| file_publisher(&p)),
            name,
            source: if delayed == 1 {
                "Service (automatic, delayed)".to_string()
            } else {
                "Service (automatic)".to_string()
            },
            source_detail: format!(r"HKLM\SYSTEM\CurrentControlSet\Services\{key_name}"),
            command,
            enabled: true, // Start == 2 is what "enabled" means for a service
            scope: "machine".to_string(),
            kind: "service".to_string(),
            ..Default::default()
        });
    }
}

// ------------------------------------------------------------------ helpers

/// Expand %VAR% style environment references the way Windows does.
fn expand_env(s: &str) -> String {
    use windows::core::HSTRING;
    use windows::Win32::System::Environment::ExpandEnvironmentStringsW;
    unsafe {
        let h = HSTRING::from(s);
        let mut buf = vec![0u16; 2048];
        let n = ExpandEnvironmentStringsW(&h, Some(&mut buf));
        if n == 0 || n as usize > buf.len() {
            return s.to_string();
        }
        String::from_utf16_lossy(&buf[..n.saturating_sub(1) as usize])
    }
}

/// Pull the executable path out of a command line, tolerating quotes, env
/// vars, NT-style prefixes (\??\, \SystemRoot), and bare relative service
/// paths like "system32\foo.exe".
fn extract_exe(command: &str) -> Option<String> {
    if command.is_empty() {
        return None;
    }
    let c = expand_env(command.trim());
    let c = c.trim_start_matches(r"\??\").to_string();
    let c = if let Some(rest) = c.strip_prefix(r"\SystemRoot") {
        format!("{}{}", expand_env("%SystemRoot%"), rest)
    } else {
        c
    };

    let path = if let Some(stripped) = c.strip_prefix('"') {
        stripped.split('"').next().map(str::to_string)?
    } else {
        let lower = c.to_lowercase();
        match lower.find(".exe") {
            Some(i) => c[..i + 4].to_string(),
            None => return None,
        }
    };

    let p = std::path::PathBuf::from(&path);
    if p.is_relative() {
        let abs = std::path::PathBuf::from(expand_env("%SystemRoot%")).join(&p);
        return Some(abs.to_string_lossy().to_string());
    }
    Some(path)
}

#[cfg(test)]
mod tests {
    /// Not an assertion, a probe: dumps the live scan to target/scan-dump.json
    /// so the result can be diffed against Task Manager / independent sources.
    #[test]
    fn dump_scan() {
        let entries = super::scan();
        let json = serde_json::to_string_pretty(&entries).unwrap();
        // The workspace target dir lives at the project root, one level up.
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../target/scan-dump.json");
        std::fs::write(path, json).unwrap();
        assert!(!entries.is_empty());
    }
}

/// CompanyName from the file's version resource, if it has one.
fn file_publisher(exe_path: &str) -> Option<String> {
    use windows::core::HSTRING;
    use windows::Win32::Storage::FileSystem::{
        GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
    };

    if !std::path::Path::new(exe_path).exists() {
        return None;
    }
    unsafe {
        let h = HSTRING::from(exe_path);
        let size = GetFileVersionInfoSizeW(&h, None);
        if size == 0 {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        if GetFileVersionInfoW(&h, None, size, data.as_mut_ptr() as *mut _).is_err() {
            return None;
        }

        // Find the first language/codepage pair, then ask for its CompanyName.
        let mut ptr: *mut core::ffi::c_void = std::ptr::null_mut();
        let mut len = 0u32;
        if !VerQueryValueW(
            data.as_ptr() as *const _,
            &HSTRING::from(r"\VarFileInfo\Translation"),
            &mut ptr,
            &mut len,
        )
        .as_bool()
            || len < 4
        {
            return None;
        }
        let lang = *(ptr as *const u16);
        let codepage = *(ptr as *const u16).add(1);

        let query = format!(r"\StringFileInfo\{lang:04x}{codepage:04x}\CompanyName");
        let mut sptr: *mut core::ffi::c_void = std::ptr::null_mut();
        let mut slen = 0u32;
        if !VerQueryValueW(
            data.as_ptr() as *const _,
            &HSTRING::from(query.as_str()),
            &mut sptr,
            &mut slen,
        )
        .as_bool()
            || slen == 0
        {
            return None;
        }
        let wide = std::slice::from_raw_parts(sptr as *const u16, slen as usize);
        let s = String::from_utf16_lossy(wide)
            .trim_end_matches('\0')
            .trim()
            .to_string();
        if s.is_empty() {
            None
        } else {
            Some(s)
        }
    }
}
