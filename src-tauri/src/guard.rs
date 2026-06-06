// Shared guard logic, included by BOTH the GUI (lib.rs) and the broker
// binary (via #[path] include). The broker is the security boundary: it
// validates with these same functions and never trusts the caller. The GUI
// also checks, but only to fail fast with a nicer message.
//
// Std-only on purpose so the broker stays small.

/// The only registry paths a StartupApproved write may ever touch. Anything
/// else is refused, no matter who asks.
pub const ALLOWED_APPROVED_PATHS: &[&str] = &[
    r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run",
    r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run32",
    r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\StartupFolder",
];

pub fn is_allowed_approved_path(path: &str) -> bool {
    ALLOWED_APPROVED_PATHS
        .iter()
        .any(|allowed| path.eq_ignore_ascii_case(allowed))
}

/// Autostart value names that must never be toggled: the pieces of Windows'
/// own protection that live in Run keys. Substring match, case-insensitive.
const PROTECTED_AUTOSTART_PATTERNS: &[&str] = &["securityhealth", "windowsdefender", "windows defender"];

pub fn is_protected_autostart(value_name: &str) -> bool {
    let lower = value_name.to_lowercase();
    PROTECTED_AUTOSTART_PATTERNS
        .iter()
        .any(|p| lower.contains(p))
}

/// The 12-byte StartupApproved value. Enabled is 0x02 with zeroed trailing
/// bytes; disabled is 0x03 followed at offset 4 by a FILETIME of when it was
/// disabled. This mirrors what Task Manager writes.
pub fn make_approved_bytes(enable: bool) -> Vec<u8> {
    let mut bytes = vec![0u8; 12];
    if enable {
        bytes[0] = 0x02;
    } else {
        bytes[0] = 0x03;
        let filetime = now_filetime();
        bytes[4..12].copy_from_slice(&filetime.to_le_bytes());
    }
    bytes
}

fn now_filetime() -> u64 {
    const FILETIME_UNIX_EPOCH: u64 = 116_444_736_000_000_000;
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => FILETIME_UNIX_EPOCH + d.as_nanos() as u64 / 100,
        Err(_) => 0,
    }
}

pub fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn from_hex(hex: &str) -> Option<Vec<u8>> {
    if hex.len() % 2 != 0 {
        return None;
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
        .collect()
}
