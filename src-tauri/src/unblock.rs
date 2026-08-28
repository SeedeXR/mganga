// Brick 8a: read-only view of a connection unblocker service (GoodbyeDPI).
//
// Scope is read from the machine, never assumed. The service's command line
// points at a blacklist file listing the sites it covers, so a user shielding
// Telegram sees Telegram, and nothing here mentions any particular site.
//
// Runs unelevated on purpose: SERVICE_QUERY_STATUS and SERVICE_QUERY_CONFIG are
// granted to normal users, so the card never needs the broker or a UAC prompt.
// Writes come in 8b and go through the broker.
// Spec: mganga-docs/docs/brick-8-connection-shield.md

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::ERROR_SERVICE_DOES_NOT_EXIST;
use windows::Win32::System::Services::{
    CloseServiceHandle, OpenSCManagerW, OpenServiceW, QueryServiceConfigW, QueryServiceStatusEx,
    QUERY_SERVICE_CONFIGW, SC_MANAGER_CONNECT, SC_STATUS_PROCESS_INFO, SERVICE_AUTO_START,
    SERVICE_DEMAND_START, SERVICE_DISABLED, SERVICE_QUERY_CONFIG, SERVICE_QUERY_STATUS,
    SERVICE_RUNNING, SERVICE_STATUS_PROCESS,
};

pub const UNBLOCK_SERVICE: &str = "GoodbyeDPI";

/// How many domain names travel to the UI. Keeps the hover list readable and
/// stops a pathological blacklist from bloating every Home render.
/// ponytail: fixed cap, paginate only if someone actually ships a huge list.
const MAX_DOMAINS_SHOWN: usize = 50;

#[derive(serde::Serialize)]
pub struct UnblockStatus {
    pub installed: bool,
    pub running: bool,
    pub start_type: String, // "auto" | "manual" | "disabled" | "unknown"
    /// The service command line, so the card can show the truth verbatim
    /// instead of asking the user to trust a label.
    pub config: Option<String>,
    /// Sites this machine is actually unblocking, read from the service's own
    /// blacklist file. Empty means we could not read one, and the card then
    /// makes no claim about scope at all.
    pub domains: Vec<String>,
    /// Total found, which can exceed `domains.len()` because of the cap above.
    pub domain_count: usize,
    pub healthy: bool, // installed && running && start_type == "auto"
}

impl UnblockStatus {
    fn absent() -> Self {
        UnblockStatus {
            installed: false,
            running: false,
            start_type: "unknown".into(),
            config: None,
            domains: Vec::new(),
            domain_count: 0,
            healthy: false,
        }
    }
}

pub fn status() -> Result<UnblockStatus, String> {
    query(UNBLOCK_SERVICE)
}

/// Pull every `--blacklist <path>` out of a service command line. The flag may
/// appear more than once, so all of them count.
///
/// Splitting on whitespace is correct here rather than merely convenient:
/// GoodbyeDPI cannot load a blacklist whose path contains a space (it fails to
/// load the file and exits at once), so a service that is actually running
/// cannot have one.
fn blacklist_paths(cmdline: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut tokens = cmdline.split_whitespace();
    while let Some(t) = tokens.next() {
        if t.eq_ignore_ascii_case("--blacklist") {
            if let Some(path) = tokens.next() {
                out.push(path.to_string());
            }
        }
    }
    out
}

/// Read the domain names out of the blacklist files. An unreadable file is not
/// an error: the card simply says nothing about scope.
fn read_domains(paths: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for p in paths {
        let Ok(text) = std::fs::read_to_string(p) else {
            continue;
        };
        for line in text.lines() {
            let d = line.trim();
            if !d.is_empty() && !d.starts_with('#') {
                out.push(d.to_string());
            }
        }
    }
    out
}

fn query(name: &str) -> Result<UnblockStatus, String> {
    unsafe {
        let scm = OpenSCManagerW(PCWSTR::null(), PCWSTR::null(), SC_MANAGER_CONNECT)
            .map_err(|e| format!("service manager: {e}"))?;

        let service = match OpenServiceW(
            scm,
            &HSTRING::from(name),
            SERVICE_QUERY_STATUS | SERVICE_QUERY_CONFIG,
        ) {
            Ok(h) => h,
            Err(e) => {
                let _ = CloseServiceHandle(scm);
                // Not installed is a state the card shows, not an error.
                return if e.code() == ERROR_SERVICE_DOES_NOT_EXIST.to_hresult() {
                    Ok(UnblockStatus::absent())
                } else {
                    Err(format!("open service {name}: {e}"))
                };
            }
        };

        let mut needed = 0u32;
        let mut buf = vec![0u8; std::mem::size_of::<SERVICE_STATUS_PROCESS>()];
        let running =
            QueryServiceStatusEx(service, SC_STATUS_PROCESS_INFO, Some(&mut buf), &mut needed)
                .map(|_| {
                    (*(buf.as_ptr() as *const SERVICE_STATUS_PROCESS)).dwCurrentState
                        == SERVICE_RUNNING
                })
                .unwrap_or(false);

        // Two-call pattern: first call reports the needed size, second fills it.
        let mut start_type = "unknown".to_string();
        let mut config = None;
        let mut cfg_needed = 0u32;
        let _ = QueryServiceConfigW(service, None, 0, &mut cfg_needed);
        if cfg_needed > 0 {
            let mut cfg_buf = vec![0u8; cfg_needed as usize];
            if QueryServiceConfigW(
                service,
                Some(cfg_buf.as_mut_ptr() as *mut QUERY_SERVICE_CONFIGW),
                cfg_needed,
                &mut cfg_needed,
            )
            .is_ok()
            {
                let cfg = &*(cfg_buf.as_ptr() as *const QUERY_SERVICE_CONFIGW);
                start_type = match cfg.dwStartType {
                    SERVICE_AUTO_START => "auto",
                    SERVICE_DEMAND_START => "manual",
                    SERVICE_DISABLED => "disabled",
                    _ => "unknown",
                }
                .to_string();
                if !cfg.lpBinaryPathName.is_null() {
                    config = Some(cfg.lpBinaryPathName.to_string().unwrap_or_default());
                }
            }
        }

        let _ = CloseServiceHandle(service);
        let _ = CloseServiceHandle(scm);

        let all_domains = match config.as_deref() {
            Some(c) => read_domains(&blacklist_paths(c)),
            None => Vec::new(),
        };
        let domain_count = all_domains.len();
        let domains = all_domains.into_iter().take(MAX_DOMAINS_SHOWN).collect();

        let healthy = running && start_type == "auto";
        Ok(UnblockStatus {
            installed: true,
            running,
            start_type,
            config,
            domains,
            domain_count,
            healthy,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_service_is_a_state_not_an_error() {
        let s = query("MgangaNoSuchService").expect("absent service must not error");
        assert!(!s.installed);
        assert!(!s.running);
        assert!(!s.healthy);
        assert!(s.config.is_none());
        assert!(s.domains.is_empty());
    }

    #[test]
    fn finds_every_blacklist_path_and_ignores_the_rest() {
        let one = blacklist_paths(
            r"C:\GoodbyeDPI\goodbyedpi.exe -e 2 --port 2053 --blacklist C:\GoodbyeDPI\d.txt",
        );
        assert_eq!(one, vec![r"C:\GoodbyeDPI\d.txt".to_string()]);

        let two = blacklist_paths(r"x.exe --blacklist a.txt -e 2 --blacklist b.txt");
        assert_eq!(two, vec!["a.txt".to_string(), "b.txt".to_string()]);

        // No list means no claim about scope.
        assert!(blacklist_paths(r"C:\GoodbyeDPI\goodbyedpi.exe -5").is_empty());
        // A trailing flag with no value must not panic.
        assert!(blacklist_paths("x.exe --blacklist").is_empty());
    }

    #[test]
    fn reads_domains_skipping_blanks_and_comments() {
        let dir = std::env::temp_dir().join("mganga-unblock-test");
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("domains.txt");
        std::fs::write(&f, "discord.com\n\n# a comment\n  discord.gg  \n").unwrap();

        let got = read_domains(&[f.to_string_lossy().to_string()]);
        assert_eq!(got, vec!["discord.com".to_string(), "discord.gg".to_string()]);

        // A missing file is silence, not a failure.
        assert!(read_domains(&["C:\\nope\\missing.txt".to_string()]).is_empty());
        let _ = std::fs::remove_file(&f);
    }

    // The 8a gate probe. Run by hand and compare against `sc query GoodbyeDPI`
    // and `sc qc GoodbyeDPI`:
    //   cargo test --lib unblock -- --ignored --nocapture
    #[test]
    #[ignore]
    fn dump_live_unblock_status() {
        let s = status().expect("live query failed");
        println!(
            "installed={} running={} start_type={} healthy={} domains={} of {} config={:?}",
            s.installed,
            s.running,
            s.start_type,
            s.healthy,
            s.domains.len(),
            s.domain_count,
            s.config
        );
        println!("sites: {:?}", s.domains);
    }
}
