// The audit log: every state-changing action leaves a receipt. One JSON
// object per line in %LOCALAPPDATA%\Mganga\audit-log.jsonl: what changed,
// when, the exact old bytes, so any change can be reversed from the log.
// Restore-from-log is a feature, not an afterthought.

use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone)]
pub struct AuditRecord {
    /// Unique id, also the sort key (unix millis plus a counter suffix).
    pub id: String,
    /// Unix milliseconds, for display.
    pub time_ms: u64,
    /// "disable-autostart" | "enable-autostart" | "undo"
    pub action: String,
    /// "HKCU" | "HKLM"
    pub hive: String,
    pub approved_path: String,
    pub value_name: String,
    /// Hex of the value bytes before the change. None means the value did
    /// not exist, so undo deletes it.
    pub old_value_hex: Option<String>,
    /// The id of the record this one undid, if action == "undo".
    pub undoes: Option<String>,
    /// Free-form extra, e.g. "7 processes" for process actions.
    #[serde(default)]
    pub detail: Option<String>,
}

fn log_path() -> Result<PathBuf, String> {
    let base = std::env::var("LOCALAPPDATA").map_err(|_| "no LOCALAPPDATA".to_string())?;
    let dir = PathBuf::from(base).join("Mganga");
    std::fs::create_dir_all(&dir).map_err(|e| format!("create log dir: {e}"))?;
    Ok(dir.join("audit-log.jsonl"))
}

pub fn append(record: &AuditRecord) -> Result<(), String> {
    let line = serde_json::to_string(record).map_err(|e| format!("serialize: {e}"))?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path()?)
        .map_err(|e| format!("open log: {e}"))?;
    writeln!(file, "{line}").map_err(|e| format!("write log: {e}"))
}

/// All records, oldest first. Unparseable lines are skipped, not fatal: a
/// damaged log line must not take the whole history down with it.
pub fn read_all() -> Result<Vec<AuditRecord>, String> {
    let path = log_path()?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = std::fs::read_to_string(&path).map_err(|e| format!("read log: {e}"))?;
    Ok(text
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect())
}

pub fn new_id(time_ms: u64) -> String {
    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    format!("{time_ms}-{}", COUNTER.fetch_add(1, Ordering::Relaxed))
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
