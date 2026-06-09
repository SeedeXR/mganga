// Auto-update over Tauri's official updater, pointed at GitHub Releases.
//
// Behaviour (issue #5): check quietly in the background when the user allows
// it, tell them in one calm line when a new version is ready, and apply it when
// they click "Restart now" (or, since the installer is per-user, on their next
// launch). Every applied update leaves an audit receipt, like every other
// change Mganga makes.
//
// Soft by design: a failed or impossible check (offline, or a portable copy
// with no install context) must never alarm the user. It resolves to "nothing
// to do", not an error in their face.

use crate::{audit, settings};
use serde::Serialize;
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tauri_plugin_updater::UpdaterExt;

/// What the frontend needs to show the "new version ready" line.
#[derive(Serialize, Clone)]
pub struct UpdateInfo {
    pub version: String,
    pub current_version: String,
    pub notes: Option<String>,
}

/// Ask GitHub Releases whether a newer version exists. `Ok(None)` means up to
/// date (or no install context); `Err` is a real, surfaceable failure.
pub async fn check(app: &AppHandle) -> Result<Option<UpdateInfo>, String> {
    let updater = app.updater().map_err(|e| e.to_string())?;
    match updater.check().await {
        Ok(Some(update)) => Ok(Some(UpdateInfo {
            version: update.version.clone(),
            current_version: update.current_version.clone(),
            notes: update.body.clone(),
        })),
        Ok(None) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

/// Download and install the pending update, record it, then relaunch onto the
/// new version. Does not return on success: `restart()` replaces the process.
pub async fn apply(app: &AppHandle) -> Result<(), String> {
    let updater = app.updater().map_err(|e| e.to_string())?;
    let update = updater
        .check()
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "no-update".to_string())?;

    let from = update.current_version.clone();
    let to = update.version.clone();

    update
        .download_and_install(|_chunk, _total| {}, || {})
        .await
        .map_err(|e| e.to_string())?;

    // Receipt first, while we still control the process. Updates are not
    // undoable, so this record carries no hive/path: History renders it as a
    // plain informational line (no Undo button).
    let time_ms = audit::now_ms();
    let _ = audit::append(&audit::AuditRecord {
        id: audit::new_id(time_ms),
        time_ms,
        action: "update".to_string(),
        hive: String::new(),
        approved_path: String::new(),
        value_name: format!("to {to}"),
        old_value_hex: None,
        undoes: None,
        detail: Some(format!("from {from}")),
    });

    app.restart()
}

/// Background loop: a quiet check shortly after launch, then every few hours,
/// but only while the user keeps the switch on. A hit emits "update-available"
/// for the frontend to render its one calm line.
pub fn spawn_background_check(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        // Let the window paint first; never compete with startup.
        tokio::time::sleep(Duration::from_secs(8)).await;
        loop {
            if settings::load().auto_update_check {
                if let Ok(Some(info)) = check(&app).await {
                    let _ = app.emit("update-available", info);
                }
            }
            tokio::time::sleep(Duration::from_secs(6 * 60 * 60)).await;
        }
    });
}
