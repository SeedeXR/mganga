// Issue #2: the uninstall suggestion.
//
// Mganga can say an autostart is safe to turn off. For a program nobody has
// opened in a year the honest answer is stronger: you could uninstall it and
// get the space back. The healer's rules stay intact: Mganga points, the user
// decides, and Windows' own uninstaller does the work. Nothing here removes
// software; launch() opens the program's registered uninstaller, which
// elevates itself if it needs to, so no broker is involved.
//
// The bar is deliberately high. A suggestion needs ALL of:
//   1. the judge already says safe-to-disable (never keep or protected)
//   2. the entry's exe maps to EXACTLY ONE installed program, by install
//      location, and that location is not a shared root like Program Files
//   3. usage says long-unused: last opened 180+ days ago, or no record at all
//      AND the install is 180+ days old
// Anything less and Mganga says nothing.

use serde::Serialize;
use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
use winreg::{RegKey, HKEY};

/// One entry from the registry's Uninstall keys: what Settings > Apps shows.
pub struct InstalledProgram {
    pub name: String,
    /// Registry path of the entry, e.g. `HKLM\Software\...\Uninstall\Foo`.
    /// This is the handle the UI hands back to launch(), never a command line.
    pub key: String,
    /// Normalized (lowercase, no trailing separator). Empty when not recorded.
    pub install_location: String,
    pub size_kb: Option<u32>,
    /// Days since the recorded InstallDate. None when absent or unreadable.
    pub installed_days: Option<u32>,
}

/// What the UI renders on a qualifying entry.
#[derive(Serialize, Clone)]
pub struct UninstallSuggestion {
    pub program: String,
    pub key: String,
    /// The honest one-liner, worded by what the evidence actually is.
    pub line: String,
}

const UNINSTALL_ROOTS: [(HKEY, &str, &str); 3] = [
    (HKEY_LOCAL_MACHINE, "HKLM", r"Software\Microsoft\Windows\CurrentVersion\Uninstall"),
    (HKEY_LOCAL_MACHINE, "HKLM", r"Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall"),
    (HKEY_CURRENT_USER, "HKCU", r"Software\Microsoft\Windows\CurrentVersion\Uninstall"),
];

const UNUSED_DAYS: u32 = 180;

/// Everything Settings > Apps would list, minus what it hides.
pub fn scan() -> Vec<InstalledProgram> {
    let today = days_since_epoch_now();
    let mut out = Vec::new();
    for (hive, label, path) in UNINSTALL_ROOTS {
        let Ok(root) = RegKey::predef(hive).open_subkey(path) else { continue };
        for sub in root.enum_keys().flatten() {
            let Ok(k) = root.open_subkey(&sub) else { continue };
            let system: u32 = k.get_value("SystemComponent").unwrap_or(0);
            let name: String = k.get_value("DisplayName").unwrap_or_default();
            let uninstall: String = k.get_value("UninstallString").unwrap_or_default();
            if system == 1 || name.trim().is_empty() || uninstall.trim().is_empty() {
                continue;
            }
            let location: String = k.get_value("InstallLocation").unwrap_or_default();
            let date: String = k.get_value("InstallDate").unwrap_or_default();
            out.push(InstalledProgram {
                name: name.trim().to_string(),
                key: format!("{label}\\{path}\\{sub}"),
                install_location: normalize_path(&location),
                size_kb: k.get_value::<u32, _>("EstimatedSize").ok().filter(|&s| s > 0),
                installed_days: parse_install_date(&date)
                    .and_then(|d| today.checked_sub(d))
                    .map(|d| d as u32),
            });
        }
    }
    out
}

/// The one installed program whose install location contains this exe, if
/// there is exactly one. Zero means unknown; two or more means a shared
/// component, and a shared component is never suggested.
pub fn owner<'a>(programs: &'a [InstalledProgram], exe: &str) -> Option<&'a InstalledProgram> {
    let exe = normalize_path(exe);
    let mut hits = programs.iter().filter(|p| {
        !p.install_location.is_empty()
            && !is_shared_root(&p.install_location)
            && exe.starts_with(&format!("{}\\", p.install_location))
    });
    let first = hits.next()?;
    if hits.next().is_some() {
        return None;
    }
    Some(first)
}

/// The suggestion for one autostart entry, or None. See the bar at the top.
pub fn suggest(
    programs: &[InstalledProgram],
    exe: Option<&str>,
    verdict: &str,
    last_opened_days: Option<u32>,
) -> Option<UninstallSuggestion> {
    if verdict != "safe-to-disable" {
        return None;
    }
    let program = owner(programs, exe?)?;
    let humanize = crate::judge::humanize_days;
    // Proven: Windows recorded the last time the user opened it. Inferred: no
    // record at all, but the install itself is old. Different claims,
    // different words; the second never says "unused".
    let opening = match (last_opened_days, program.installed_days) {
        (Some(days), _) if days >= UNUSED_DAYS => format!("Unused for about {}.", humanize(days)),
        (None, Some(age)) if age >= UNUSED_DAYS => format!(
            "Installed about {} ago, with no record of you opening it.",
            humanize(age)
        ),
        _ => return None,
    };
    let line = match program.size_kb {
        Some(kb) => format!("{opening} Uninstalling frees about {}.", format_size(kb)),
        None => opening,
    };
    Some(UninstallSuggestion {
        program: program.name.clone(),
        key: program.key.clone(),
        line,
    })
}

/// Open the program's own uninstaller (the `UninstallString` Windows
/// registered for it). `key` must be an Uninstall entry Mganga itself listed:
/// it is re-read here, so the frontend never supplies a command line.
/// Returns (what opened, program name): "uninstaller", or "settings" when the
/// uninstaller would not start and Windows Settings > Apps was opened instead.
pub fn launch(key: &str) -> Result<(&'static str, String), String> {
    let (hive, path) = parse_key(key).ok_or("not an uninstall entry")?;
    let k = RegKey::predef(hive)
        .open_subkey(path)
        .map_err(|_| "that program is no longer installed".to_string())?;
    let name: String = k.get_value("DisplayName").unwrap_or_default();
    let command: String = k.get_value("UninstallString").unwrap_or_default();
    if command.trim().is_empty() {
        return Err("that program has no uninstaller registered".into());
    }
    let (exe, args) = split_command(&msi_uninstall_verb(&command));
    if shell_open(&exe, &args) {
        return Ok(("uninstaller", name));
    }
    if shell_open("ms-settings:appsfeatures", "") {
        return Ok(("settings", name));
    }
    Err("could not open the uninstaller or Windows Settings".into())
}

// ------------------------------------------------------------------ helpers

/// ShellExecute "open": honours the target's own manifest, so an uninstaller
/// that needs admin rights raises its own UAC prompt. True on success.
fn shell_open(file: &str, params: &str) -> bool {
    use windows::core::HSTRING;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let result = unsafe {
        ShellExecuteW(
            None,
            &HSTRING::from("open"),
            &HSTRING::from(file),
            &HSTRING::from(params),
            None,
            SW_SHOWNORMAL,
        )
    };
    // A fake HINSTANCE; values above 32 mean it started.
    result.0 as isize > 32
}

/// MSI entries register the maintenance verb (`/I`); `/X` is uninstall, which
/// is what Settings > Apps runs. Anything else passes through untouched.
fn msi_uninstall_verb(command: &str) -> String {
    let lower = command.to_lowercase();
    match lower.find("msiexec.exe /i") {
        // The slice is only safe when lowercasing kept every byte in place.
        Some(i) if lower.len() == command.len() => {
            let mut fixed = command.to_string();
            fixed.replace_range(i + 12..i + 14, "/X");
            fixed
        }
        _ => command.to_string(),
    }
}

/// Split an UninstallString into the program and its arguments, tolerating
/// a quoted or bare path. Bare paths end at ".exe".
fn split_command(command: &str) -> (String, String) {
    let c = command.trim();
    if let Some(rest) = c.strip_prefix('"') {
        if let Some(end) = rest.find('"') {
            return (rest[..end].to_string(), rest[end + 1..].trim().to_string());
        }
        return (rest.to_string(), String::new());
    }
    match c.to_lowercase().find(".exe") {
        Some(i) => (c[..i + 4].to_string(), c[i + 4..].trim().to_string()),
        None => (c.to_string(), String::new()),
    }
}

/// Only keys directly under the three Uninstall roots are accepted, and only
/// one level deep, so a key string can never point anywhere else.
fn parse_key(key: &str) -> Option<(HKEY, &str)> {
    for (hive, label, root) in UNINSTALL_ROOTS {
        let prefix = format!("{label}\\{root}\\");
        if let Some(sub) = key.strip_prefix(&prefix) {
            if !sub.is_empty() && !sub.contains('\\') {
                return Some((hive, &key[label.len() + 1..]));
            }
        }
    }
    None
}

fn normalize_path(p: &str) -> String {
    p.trim()
        .trim_matches('"')
        .replace('/', "\\")
        .to_lowercase()
        .trim_end_matches('\\')
        .to_string()
}

/// Locations that are containers for many programs, not one program. A
/// program that claims one of these as its install location owns nothing.
fn is_shared_root(loc: &str) -> bool {
    // A bare drive ("c:") owns everything and therefore nothing. A folder
    // directly under the drive ("c:\pentabletdriver") is a normal home for
    // a program, so depth alone is not the test; the named containers are.
    if !loc.contains('\\') {
        return true;
    }
    [
        "ProgramFiles",
        "ProgramFiles(x86)",
        "ProgramData",
        "CommonProgramFiles",
        "CommonProgramFiles(x86)",
        "LOCALAPPDATA",
        "APPDATA",
        "USERPROFILE",
        "SystemRoot",
    ]
    .iter()
    .filter_map(|v| std::env::var(v).ok())
    .flat_map(|root| {
        let parent = std::path::Path::new(&root)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        [
            normalize_path(&root),
            normalize_path(&format!("{root}\\Programs")),
            normalize_path(&parent), // e.g. C:\Users above a profile
        ]
    })
    .any(|root| root == loc)
}

/// Registry InstallDate is "YYYYMMDD". Returns days since the Unix epoch.
fn parse_install_date(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.len() != 8 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let y: i64 = s[..4].parse().ok()?;
    let m: u32 = s[4..6].parse().ok()?;
    let d: u32 = s[6..].parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    // Howard Hinnant's days_from_civil: proleptic Gregorian to epoch days.
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp as i64 + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

fn days_since_epoch_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| (d.as_secs() / 86_400) as i64)
        .unwrap_or(0)
}

fn format_size(kb: u32) -> String {
    if kb >= 1_048_576 {
        format!("{:.1} GB", kb as f64 / 1_048_576.0)
    } else if kb >= 1024 {
        format!("{} MB", kb / 1024)
    } else {
        format!("{kb} KB")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn program(name: &str, loc: &str, size_kb: Option<u32>, installed_days: Option<u32>) -> InstalledProgram {
        InstalledProgram {
            name: name.into(),
            key: format!("HKLM\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{name}"),
            install_location: normalize_path(loc),
            size_kb,
            installed_days,
        }
    }

    #[test]
    fn owner_needs_exactly_one_deep_match() {
        let programs = [
            program("Foo", r"C:\Program Files\Foo\", None, None),
            program("Bar", r"C:\Program Files\Bar", None, None),
            program("Bar Helper", r"C:\Program Files\Bar", None, None),
            program("Greedy", r"C:\Program Files", None, None),
            program("Greedier", r"C:\", None, None),
            program("Tablet", r"C:\PenTabletDriver\", None, None),
        ];
        // A folder straight under the drive is a normal home for a program.
        assert_eq!(owner(&programs, r"C:\PenTabletDriver\TabletDriver.exe").map(|p| &p.name[..]), Some("Tablet"));
        assert_eq!(owner(&programs, r"C:\Program Files\Foo\foo.exe").map(|p| &p.name[..]), Some("Foo"));
        // Two programs claim Bar's folder: shared, so silence.
        assert!(owner(&programs, r"C:\Program Files\Bar\bar.exe").is_none());
        // Only the shallow claimants match this one, and they never count.
        assert!(owner(&programs, r"C:\Program Files\Other\x.exe").is_none());
        // Prefix must be a whole folder, not a string prefix.
        assert!(owner(&programs, r"C:\Program Files\Foobar\x.exe").is_none());
    }

    #[test]
    fn suggestion_bar_and_wording() {
        let programs = [
            program("Old Thing", r"C:\Program Files\Old", Some(1_258_291), Some(400)),
            program("Sizeless", r"C:\Program Files\Sizeless", None, Some(20)),
        ];
        let old = Some(r"C:\Program Files\Old\old.exe");
        // Verdict gate.
        assert!(suggest(&programs, old, "your-call", Some(400)).is_none());
        // Proven: a last-opened record 180+ days back.
        let s = suggest(&programs, old, "safe-to-disable", Some(400)).unwrap();
        assert_eq!(s.line, "Unused for about a year. Uninstalling frees about 1.2 GB.");
        // Recently opened: no suggestion even if the install is old.
        assert!(suggest(&programs, old, "safe-to-disable", Some(30)).is_none());
        // Inferred: no record, old install. Hedged wording, no "unused".
        let s = suggest(&programs, old, "safe-to-disable", None).unwrap();
        assert!(s.line.starts_with("Installed about a year ago, with no record of you opening it."));
        // No record and a young install: silence.
        let young = Some(r"C:\Program Files\Sizeless\s.exe");
        assert!(suggest(&programs, young, "safe-to-disable", None).is_none());
        // No size known: the line stops after the evidence.
        let s = suggest(&programs, young, "safe-to-disable", Some(200)).unwrap();
        assert_eq!(s.line, "Unused for about 6 months.");
    }

    #[test]
    fn keys_commands_and_dates_parse_strictly() {
        assert!(parse_key(r"HKLM\Software\Microsoft\Windows\CurrentVersion\Uninstall\Foo").is_some());
        assert!(parse_key(r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\Foo").is_some());
        assert!(parse_key(r"HKLM\Software\Microsoft\Windows\CurrentVersion\Uninstall\Foo\Bar").is_none());
        assert!(parse_key(r"HKLM\Software\Microsoft\Windows\CurrentVersion\Run\Foo").is_none());
        assert!(parse_key(r"HKLM\Software\Microsoft\Windows\CurrentVersion\Uninstall\").is_none());

        assert_eq!(
            split_command(r#""C:\Program Files\Foo\unins000.exe" /SILENT"#),
            (r"C:\Program Files\Foo\unins000.exe".to_string(), "/SILENT".to_string())
        );
        assert_eq!(
            split_command(r"C:\Program Files\Foo\uninstall.exe --quiet"),
            (r"C:\Program Files\Foo\uninstall.exe".to_string(), "--quiet".to_string())
        );

        assert_eq!(
            msi_uninstall_verb("MsiExec.exe /I{6F2E1A3B-0000-4000-8000-000000000001}"),
            "MsiExec.exe /X{6F2E1A3B-0000-4000-8000-000000000001}"
        );
        assert_eq!(msi_uninstall_verb("MsiExec.exe /X{ABC}"), "MsiExec.exe /X{ABC}");
        assert_eq!(msi_uninstall_verb(r"C:\Foo\unins.exe /I"), r"C:\Foo\unins.exe /I");

        assert_eq!(parse_install_date("19700101"), Some(0));
        assert_eq!(parse_install_date("20000301"), Some(11_017));
        assert!(parse_install_date("2024-01-01").is_none());
        assert!(parse_install_date("20241301").is_none());

        assert_eq!(format_size(1_258_291), "1.2 GB");
        assert_eq!(format_size(348_160), "340 MB");
    }

    /// A probe, not an assertion on content: the live Uninstall keys parse.
    #[test]
    fn probe_installed_programs() {
        let programs = scan();
        println!("{} installed programs with an uninstaller", programs.len());
        for p in programs.iter().take(5) {
            println!("  {} | {} | {:?} KB | {:?} days", p.name, p.install_location, p.size_kb, p.installed_days);
        }
        assert!(!programs.is_empty());
    }
}
