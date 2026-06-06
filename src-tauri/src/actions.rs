// Brick 4: the HKCU side of toggling autostarters. The GUI may write its own
// user's StartupApproved values directly, no elevation needed. HKLM writes go
// through the broker, never from here.

use crate::guard;
use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_BINARY};
use winreg::{RegKey, RegValue};

/// Flip one HKCU StartupApproved value. Returns the previous bytes as hex,
/// or None if the value did not exist (a Run entry with no state yet).
pub fn set_enabled_hkcu(
    approved_path: &str,
    value_name: &str,
    enable: bool,
) -> Result<Option<String>, String> {
    write_hkcu_value(approved_path, value_name, Some(guard::make_approved_bytes(enable)))
}

/// Restore a value to exactly what it was: raw old bytes, or delete it if it
/// did not exist before the change.
pub fn restore_hkcu(
    approved_path: &str,
    value_name: &str,
    old_value_hex: Option<&str>,
) -> Result<Option<String>, String> {
    let bytes = match old_value_hex {
        Some(hex) => Some(guard::from_hex(hex).ok_or("bad hex in audit record")?),
        None => None,
    };
    write_hkcu_value(approved_path, value_name, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";

    /// Full disable -> enable -> restore-to-missing loop on a dummy value.
    /// An orphan StartupApproved value (no matching Run entry) is inert, and
    /// the test removes it again, so this is safe on a real machine.
    #[test]
    fn hkcu_write_and_restore_roundtrip() {
        let name = "MgangaSelfTest";

        // Disable: value should not exist yet, first byte becomes 0x03.
        let old = set_enabled_hkcu(RUN, name, false).unwrap();
        assert_eq!(old, None, "leftover test value from a previous run?");
        let raw = read_raw(name).expect("value should exist after disable");
        assert_eq!(raw[0], 0x03);
        assert_eq!(raw.len(), 12);

        // Enable: first byte becomes 0x02, old bytes are reported back.
        let old = set_enabled_hkcu(RUN, name, true).unwrap();
        assert!(old.unwrap().starts_with("03"));
        assert_eq!(read_raw(name).unwrap()[0], 0x02);

        // Restore to "did not exist": the value disappears.
        restore_hkcu(RUN, name, None).unwrap();
        assert!(read_raw(name).is_none());
    }

    #[test]
    fn guard_refuses_protected_and_bad_paths() {
        assert_eq!(
            set_enabled_hkcu(RUN, "SecurityHealthSystray", false),
            Err("protected".to_string())
        );
        assert_eq!(
            set_enabled_hkcu(r"Software\Wrong\Key", "Anything", false),
            Err("bad-path".to_string())
        );
    }

    fn read_raw(name: &str) -> Option<Vec<u8>> {
        RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey(RUN)
            .ok()?
            .get_raw_value(name)
            .ok()
            .map(|v| v.bytes)
    }
}

/// Shared write: Some(bytes) sets the value, None deletes it. Always returns
/// the previous bytes (hex) so every change can be audited.
fn write_hkcu_value(
    approved_path: &str,
    value_name: &str,
    new_bytes: Option<Vec<u8>>,
) -> Result<Option<String>, String> {
    if !guard::is_allowed_approved_path(approved_path) {
        return Err("bad-path".into());
    }
    if guard::is_protected_autostart(value_name) {
        return Err("protected".into());
    }

    let (key, _) = RegKey::predef(HKEY_CURRENT_USER)
        .create_subkey_with_flags(approved_path, KEY_READ | KEY_WRITE)
        .map_err(|e| format!("open {approved_path}: {e}"))?;

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
            // Deleting a value that is already gone is success, not an error.
            if old.is_some() {
                key.delete_value(value_name)
                    .map_err(|e| format!("delete {value_name}: {e}"))?;
            }
        }
    }
    Ok(old)
}
