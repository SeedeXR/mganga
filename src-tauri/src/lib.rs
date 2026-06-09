mod actions;
mod audit;
mod autostart;
mod broker_client;
mod guard;
mod judge;
mod proc_control;
mod processes;
mod settings;
mod updater;
mod usage;

use broker_client::BrokerConn;
use serde_json::{json, Value};
use std::sync::Mutex;
use tauri::State;

/// The one live connection to the elevated broker, if it has been started.
struct Broker(Mutex<Option<BrokerConn>>);

/// Persistent sysinfo System: CPU percentages are deltas between two
/// observations, so the observer must outlive single calls.
struct ProcState(Mutex<sysinfo::System>);

/// Mganga's own memory of what it throttled or suspended, per PID. Windows
/// lets you set EcoQoS but not read it back, so this is the only record.
#[derive(Default)]
struct ProcCtl {
    throttled: Mutex<std::collections::HashSet<u32>>,
    suspended: Mutex<std::collections::HashSet<u32>>,
}

// Brick 0: prove the frontend can call Rust and get an answer back.
#[tauri::command]
fn ping() -> String {
    "pong from Rust. The healer is awake.".to_string()
}

/// Brick 1: launch the broker (one UAC prompt), connect, and prove the channel
/// with a ping. Returns the broker's reply, including its elevation state.
#[tauri::command]
fn broker_start(state: State<Broker>) -> Result<Value, String> {
    let mut guard = state.0.lock().map_err(|_| "state poisoned".to_string())?;
    if guard.is_none() {
        broker_client::launch()?;
        *guard = Some(broker_client::connect()?);
    }
    let conn = guard.as_mut().expect("just set");
    match broker_client::call(conn, "ping", json!({})) {
        Ok(v) => Ok(v),
        Err(e) => {
            // A dead pipe is not recoverable; drop it so the user can retry.
            *guard = None;
            Err(e)
        }
    }
}

/// Read one value from HKLM through the broker. The privileged no-op that
/// proves the privilege boundary end to end.
#[tauri::command]
fn broker_read_hklm(state: State<Broker>, path: String, name: String) -> Result<Value, String> {
    let mut guard = state.0.lock().map_err(|_| "state poisoned".to_string())?;
    let conn = guard.as_mut().ok_or("broker-not-running".to_string())?;
    match broker_client::call(conn, "read_hklm", json!({ "path": path, "name": name })) {
        Ok(v) => Ok(v),
        Err(e) => {
            if e == "broker-gone" {
                *guard = None;
            }
            Err(e)
        }
    }
}

/// Brick 2: the full read-only autostart inventory.
#[tauri::command]
fn scan_autostarts() -> Vec<autostart::AutostartEntry> {
    autostart::scan()
}

/// Brick 4: flip one autostarter on or off by its StartupApproved value.
/// HKCU is written directly, HKLM goes through the broker. Every change is
/// appended to the audit log with the old bytes so it can be undone.
#[tauri::command]
fn set_autostart_enabled(
    state: State<Broker>,
    hive: String,
    approved_path: String,
    value_name: String,
    enable: bool,
) -> Result<(), String> {
    // Fail fast in the GUI; the broker re-checks for HKLM regardless.
    if !guard::is_allowed_approved_path(&approved_path) {
        return Err("bad-path".into());
    }
    if guard::is_protected_autostart(&value_name) {
        return Err("protected".into());
    }

    let old_value_hex = match hive.as_str() {
        "HKCU" => actions::set_enabled_hkcu(&approved_path, &value_name, enable)?,
        "HKLM" => {
            let mut guard_conn = state.0.lock().map_err(|_| "state poisoned".to_string())?;
            let conn = guard_conn.as_mut().ok_or("broker-not-running".to_string())?;
            let result = broker_client::call(
                conn,
                "set_startup_approved",
                json!({ "path": approved_path, "name": value_name, "enable": enable }),
            )
            .map_err(|e| {
                if e == "broker-gone" {
                    *guard_conn = None;
                }
                e
            })?;
            result["old_value_hex"].as_str().map(str::to_string)
        }
        _ => return Err("bad-hive".into()),
    };

    let time_ms = audit::now_ms();
    audit::append(&audit::AuditRecord {
        id: audit::new_id(time_ms),
        time_ms,
        action: if enable { "enable-autostart" } else { "disable-autostart" }.to_string(),
        hive,
        approved_path,
        value_name,
        old_value_hex,
        undoes: None,
        detail: None,
    })
}

/// Brick 5: one live snapshot of what is running and what it costs.
/// Brick 6 adds the flags: protected, and Mganga's own throttled/suspended
/// memory (pruned here as processes die).
#[tauri::command]
fn get_processes(
    state: State<ProcState>,
    ctl: State<ProcCtl>,
) -> Result<processes::ProcessSnapshot, String> {
    let mut sys = state.0.lock().map_err(|_| "state poisoned".to_string())?;
    let mut snap = processes::snapshot(&mut sys);

    let mut throttled = ctl.throttled.lock().map_err(|_| "state poisoned".to_string())?;
    let mut suspended = ctl.suspended.lock().map_err(|_| "state poisoned".to_string())?;
    let live: std::collections::HashSet<u32> =
        snap.groups.iter().flat_map(|g| g.pids.iter().copied()).collect();
    throttled.retain(|pid| live.contains(pid));
    suspended.retain(|pid| live.contains(pid));

    for group in &mut snap.groups {
        let name_lower = group.name.to_lowercase();
        group.protected = proc_control::is_protected_process(&name_lower);
        group.throttled = group.pids.iter().any(|p| throttled.contains(p));
        group.suspended = group.pids.iter().any(|p| suspended.contains(p));
        if group.protected {
            group.verdict = Some("protected".into());
            group.reason = Some("This keeps Windows running. Mganga won't touch it.".into());
        } else if let Some((verdict, reason)) = judge::judge_process(&name_lower) {
            group.verdict = Some(verdict);
            group.reason = Some(reason);
        }
    }
    Ok(snap)
}

/// Brick 6: act on a group of processes, gentle or otherwise. Tries
/// unelevated first; PIDs Windows refuses (other users, elevated) are retried
/// through the broker if it is running. Protected names refuse everywhere.
#[tauri::command]
fn process_action(
    broker: State<Broker>,
    proc_state: State<ProcState>,
    ctl: State<ProcCtl>,
    pids: Vec<u32>,
    name: String,
    action: String,
) -> Result<Value, String> {
    // Never act on our own descendants: Mganga's UI lives in a webview
    // process whose name (msedgewebview2) other apps share.
    let pids = filter_own_descendants(&proc_state, pids)?;
    if pids.is_empty() {
        return Err("Those processes belong to Mganga itself.".into());
    }

    let mut ok: Vec<u32> = Vec::new();
    let mut denied: Vec<u32> = Vec::new();
    let mut first_error: Option<String> = None;
    for &pid in &pids {
        let result = match action.as_str() {
            "throttle" => proc_control::set_efficiency(pid, true),
            "unthrottle" => proc_control::set_efficiency(pid, false),
            "suspend" => proc_control::suspend(pid),
            "resume" => proc_control::resume(pid),
            "kill" => proc_control::kill(pid),
            _ => return Err("bad-action".into()),
        };
        match result {
            Ok(()) => ok.push(pid),
            Err(e) if e == "access-denied" => denied.push(pid),
            Err(e) if e == "protected" => return Err("protected".into()),
            Err(e) => {
                first_error.get_or_insert(e);
            }
        }
    }

    // Elevated stragglers go through the broker, which re-checks everything.
    let mut needs_helper = 0usize;
    if !denied.is_empty() {
        let mut guard_conn = broker.0.lock().map_err(|_| "state poisoned".to_string())?;
        match guard_conn.as_mut() {
            Some(conn) => {
                let result = broker_client::call(
                    conn,
                    "process_action",
                    json!({ "pids": denied, "action": action }),
                )
                .map_err(|e| {
                    if e == "broker-gone" {
                        *guard_conn = None;
                    }
                    e
                })?;
                if result["ok_count"].as_u64().unwrap_or(0) as usize == denied.len() {
                    ok.extend(&denied);
                } else if let Some(e) = result["error"].as_str() {
                    first_error.get_or_insert(e.to_string());
                }
            }
            None => needs_helper = denied.len(),
        }
    }

    // Remember what we did, so the UI can show it.
    {
        let mut throttled = ctl.throttled.lock().map_err(|_| "state poisoned".to_string())?;
        let mut suspended = ctl.suspended.lock().map_err(|_| "state poisoned".to_string())?;
        for &pid in &ok {
            match action.as_str() {
                "throttle" => {
                    throttled.insert(pid);
                }
                "unthrottle" => {
                    throttled.remove(&pid);
                }
                "suspend" => {
                    suspended.insert(pid);
                }
                "resume" => {
                    suspended.remove(&pid);
                }
                _ => {}
            }
        }
    }

    if !ok.is_empty() {
        let time_ms = audit::now_ms();
        let _ = audit::append(&audit::AuditRecord {
            id: audit::new_id(time_ms),
            time_ms,
            action: format!("{action}-process"),
            hive: String::new(),
            approved_path: String::new(),
            value_name: name,
            old_value_hex: None,
            undoes: None,
            detail: Some(format!(
                "{} process{}",
                ok.len(),
                if ok.len() == 1 { "" } else { "es" }
            )),
        });
    }

    Ok(json!({
        "ok_count": ok.len(),
        "needs_helper": needs_helper,
        "error": first_error,
    }))
}

/// Drop PIDs that are Mganga's own process or its descendants (the webview
/// and any helpers), using the live parent chain from sysinfo.
fn filter_own_descendants(
    proc_state: &State<ProcState>,
    pids: Vec<u32>,
) -> Result<Vec<u32>, String> {
    use sysinfo::Pid;
    let me = std::process::id();
    let sys = proc_state.0.lock().map_err(|_| "state poisoned".to_string())?;
    Ok(pids
        .into_iter()
        .filter(|&pid| {
            if pid == me {
                return false;
            }
            // Walk up the parent chain (bounded, trees are shallow).
            let mut current = Pid::from_u32(pid);
            for _ in 0..32 {
                match sys.process(current).and_then(|p| p.parent()) {
                    Some(parent) => {
                        if parent.as_u32() == me {
                            return false;
                        }
                        current = parent;
                    }
                    None => break,
                }
            }
            true
        })
        .collect())
}

/// The audit log, newest first.
#[tauri::command]
fn list_audit_log() -> Result<Vec<audit::AuditRecord>, String> {
    let mut records = audit::read_all()?;
    records.reverse();
    Ok(records)
}

/// Undo one logged change by restoring the exact old bytes (or deleting the
/// value if it did not exist before). The undo itself is logged too.
#[tauri::command]
fn undo_change(state: State<Broker>, id: String) -> Result<(), String> {
    let record = audit::read_all()?
        .into_iter()
        .find(|r| r.id == id)
        .ok_or("no such change in the log".to_string())?;

    // Applied updates are recorded but cannot be reversed: there is no old
    // version to restore. The UI hides Undo for these; this is the backstop.
    if record.action == "update" {
        return Err("updates cannot be undone".into());
    }

    let old_hex = record.old_value_hex.as_deref();
    let before_undo = match record.hive.as_str() {
        "HKCU" => actions::restore_hkcu(&record.approved_path, &record.value_name, old_hex)?,
        "HKLM" => {
            let mut guard_conn = state.0.lock().map_err(|_| "state poisoned".to_string())?;
            let conn = guard_conn.as_mut().ok_or("broker-not-running".to_string())?;
            let result = broker_client::call(
                conn,
                "restore_startup_approved",
                json!({
                    "path": record.approved_path,
                    "name": record.value_name,
                    "old_value_hex": record.old_value_hex,
                }),
            )
            .map_err(|e| {
                if e == "broker-gone" {
                    *guard_conn = None;
                }
                e
            })?;
            result["old_value_hex"].as_str().map(str::to_string)
        }
        _ => return Err("bad-hive".into()),
    };

    let time_ms = audit::now_ms();
    audit::append(&audit::AuditRecord {
        id: audit::new_id(time_ms),
        time_ms,
        action: "undo".to_string(),
        hive: record.hive,
        approved_path: record.approved_path,
        value_name: record.value_name,
        old_value_hex: before_undo,
        undoes: Some(record.id),
        detail: None,
    })
}

// ---- Settings + updates ----

/// Current settings, defaults if the file is missing or damaged.
#[tauri::command]
fn get_settings() -> settings::Settings {
    settings::load()
}

/// Flip the background update check on or off.
#[tauri::command]
fn set_auto_update_check(enabled: bool) -> Result<(), String> {
    let mut s = settings::load();
    s.auto_update_check = enabled;
    settings::save(&s)
}

/// The running version, for the Settings view.
#[tauri::command]
fn app_version(app: tauri::AppHandle) -> String {
    app.package_info().version.to_string()
}

/// Check now, on demand (the "Check for updates" button).
#[tauri::command]
async fn check_for_update(app: tauri::AppHandle) -> Result<Option<updater::UpdateInfo>, String> {
    updater::check(&app).await
}

/// Download, install, and relaunch onto the new version.
#[tauri::command]
async fn install_update(app: tauri::AppHandle) -> Result<(), String> {
    updater::apply(&app).await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(Broker(Mutex::new(None)))
        .manage(ProcState(Mutex::new(sysinfo::System::new())))
        .manage(ProcCtl::default())
        .setup(|app| {
            // Quiet background check for new versions, gated on the user's
            // setting. Never blocks startup.
            updater::spawn_background_check(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ping,
            broker_start,
            broker_read_hklm,
            scan_autostarts,
            get_processes,
            process_action,
            set_autostart_enabled,
            list_audit_log,
            undo_change,
            get_settings,
            set_auto_update_check,
            app_version,
            check_for_update,
            install_update
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
