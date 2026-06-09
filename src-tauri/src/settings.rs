// User settings: a tiny JSON file next to the audit log in
// %LOCALAPPDATA%\Mganga\settings.json. Today it holds one switch, whether
// Mganga is allowed to check for updates. A missing or damaged file is not an
// error: we fall back to defaults, because the app must run offline and clean
// on a first launch with no file at all.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone)]
pub struct Settings {
    /// When true (the default), Mganga checks GitHub Releases for a newer
    /// version in the background. The user's one off switch lives here.
    pub auto_update_check: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            auto_update_check: true,
        }
    }
}

fn settings_path() -> Result<PathBuf, String> {
    let base = std::env::var("LOCALAPPDATA").map_err(|_| "no LOCALAPPDATA".to_string())?;
    let dir = PathBuf::from(base).join("Mganga");
    std::fs::create_dir_all(&dir).map_err(|e| format!("create settings dir: {e}"))?;
    Ok(dir.join("settings.json"))
}

/// Read settings, falling back to defaults on any problem. Never fails, so the
/// caller always has something usable.
pub fn load() -> Settings {
    let path = match settings_path() {
        Ok(p) => p,
        Err(_) => return Settings::default(),
    };
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
        Err(_) => Settings::default(),
    }
}

pub fn save(settings: &Settings) -> Result<(), String> {
    let text = serde_json::to_string_pretty(settings).map_err(|e| format!("serialize: {e}"))?;
    std::fs::write(settings_path()?, text).map_err(|e| format!("write settings: {e}"))
}
