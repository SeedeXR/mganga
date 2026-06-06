// Usage evidence from UserAssist, the registry key where Explorer counts
// every program the user launches on purpose (Start menu, taskbar, double
// click). Auto-started runs are NOT counted here, which is exactly what makes
// it the right signal for "do you actually use this app, or does it just
// start itself".
//
// Format notes (verified against forensic documentation and Eric Zimmerman's
// parser): value names are ROT13 encoded paths; the data blob holds the run
// count at offset 0x04 (u32 LE) and the last execution FILETIME at offset
// 0x3C (u64 LE). One GUID subkey tracks .exe launches, another tracks .lnk
// shortcut launches.

use std::collections::HashMap;
use winreg::enums::HKEY_CURRENT_USER;
use winreg::RegKey;

/// Deliberate-use evidence for one program.
pub struct Usage {
    pub run_count: u32,
    /// Days since the user last opened it. None if the timestamp is missing.
    pub last_run_days: Option<u32>,
}

const EXE_GUID: &str = "{CEBFF5CD-ACE2-4F4F-9178-9926F41749EA}";
const LNK_GUID: &str = "{F4E57C4B-2036-45F0-A9AB-443BCFE33D9F}";

/// Map from a lowercase key to usage. Keys are exe file names ("steam.exe")
/// for the exe GUID and shortcut stems ("megasync") for the lnk GUID.
pub fn collect() -> HashMap<String, Usage> {
    let mut map: HashMap<String, Usage> = HashMap::new();

    for guid in [EXE_GUID, LNK_GUID] {
        let path = format!(
            r"Software\Microsoft\Windows\CurrentVersion\Explorer\UserAssist\{guid}\Count"
        );
        let Ok(key) = RegKey::predef(HKEY_CURRENT_USER).open_subkey(&path) else {
            continue;
        };
        for (name, value) in key.enum_values().flatten() {
            let decoded = rot13(&name).to_lowercase();
            let base = decoded.rsplit('\\').next().unwrap_or(&decoded).to_string();

            let map_key = if base.ends_with(".exe") {
                base
            } else if base.ends_with(".lnk") {
                base.trim_end_matches(".lnk").to_string()
            } else {
                continue;
            };

            if value.bytes.len() < 0x3C + 8 {
                continue;
            }
            let run_count = u32::from_le_bytes(value.bytes[4..8].try_into().unwrap());
            let filetime = u64::from_le_bytes(value.bytes[0x3C..0x3C + 8].try_into().unwrap());
            let usage = Usage {
                run_count,
                last_run_days: filetime_days_ago(filetime),
            };

            // Several UserAssist entries can point at the same exe (different
            // install paths, shortcut plus exe). Keep the strongest evidence:
            // the most recent last-run, and the highest count.
            map.entry(map_key)
                .and_modify(|existing| {
                    existing.run_count = existing.run_count.max(usage.run_count);
                    existing.last_run_days = match (existing.last_run_days, usage.last_run_days) {
                        (Some(a), Some(b)) => Some(a.min(b)),
                        (a, b) => a.or(b),
                    };
                })
                .or_insert(usage);
        }
    }
    map
}

#[cfg(test)]
mod tests {
    #[test]
    fn probe_usage() {
        let map = super::collect();
        println!("usage map has {} entries", map.len());
        for key in ["steam.exe", "idman.exe", "ms-teams.exe", "epicgameslauncher.exe"] {
            match map.get(key) {
                Some(u) => println!("{key}: count={} last_days={:?}", u.run_count, u.last_run_days),
                None => println!("{key}: NOT FOUND"),
            }
        }
    }
}

fn rot13(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'a'..='z' => (((c as u8 - b'a' + 13) % 26) + b'a') as char,
            'A'..='Z' => (((c as u8 - b'A' + 13) % 26) + b'A') as char,
            other => other,
        })
        .collect()
}

/// FILETIME (100ns ticks since 1601) to "days ago". None for zero or bogus
/// timestamps, including ones in the future.
fn filetime_days_ago(filetime: u64) -> Option<u32> {
    if filetime == 0 {
        return None;
    }
    const FILETIME_UNIX_EPOCH: u64 = 116_444_736_000_000_000;
    if filetime < FILETIME_UNIX_EPOCH {
        return None;
    }
    let unix_secs = (filetime - FILETIME_UNIX_EPOCH) / 10_000_000;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    if unix_secs > now {
        return None;
    }
    Some(((now - unix_secs) / 86_400) as u32)
}
