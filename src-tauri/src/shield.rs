// Brick 8a: read-only view of the Discord connection shield (the GoodbyeDPI
// service). Runs unelevated on purpose: SERVICE_QUERY_STATUS and
// SERVICE_QUERY_CONFIG are granted to normal users, so the card never needs
// the broker or a UAC prompt. Writes come in 8b and go through the broker.
// Spec: mganga-docs/docs/brick-8-connection-shield.md

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::ERROR_SERVICE_DOES_NOT_EXIST;
use windows::Win32::System::Services::{
    CloseServiceHandle, OpenSCManagerW, OpenServiceW, QueryServiceConfigW,
    QueryServiceStatusEx, QUERY_SERVICE_CONFIGW, SC_MANAGER_CONNECT, SC_STATUS_PROCESS_INFO,
    SERVICE_AUTO_START, SERVICE_DEMAND_START, SERVICE_DISABLED, SERVICE_QUERY_CONFIG,
    SERVICE_QUERY_STATUS, SERVICE_RUNNING, SERVICE_STATUS_PROCESS,
};

pub const SHIELD_SERVICE: &str = "GoodbyeDPI";

#[derive(serde::Serialize)]
pub struct ShieldStatus {
    pub installed: bool,
    pub running: bool,
    pub start_type: String, // "auto" | "manual" | "disabled" | "unknown"
    /// The service's command line, so the card can show the scope (which
    /// domains are shielded) instead of asking the user to trust a label.
    pub config: Option<String>,
    pub healthy: bool, // installed && running && start_type == "auto"
}

impl ShieldStatus {
    fn absent() -> Self {
        ShieldStatus {
            installed: false,
            running: false,
            start_type: "unknown".into(),
            config: None,
            healthy: false,
        }
    }
}

pub fn status() -> Result<ShieldStatus, String> {
    query(SHIELD_SERVICE)
}

fn query(name: &str) -> Result<ShieldStatus, String> {
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
                    Ok(ShieldStatus::absent())
                } else {
                    Err(format!("open service {name}: {e}"))
                };
            }
        };

        let mut needed = 0u32;
        let mut buf = vec![0u8; std::mem::size_of::<SERVICE_STATUS_PROCESS>()];
        let running = QueryServiceStatusEx(service, SC_STATUS_PROCESS_INFO, Some(&mut buf), &mut needed)
            .map(|_| (*(buf.as_ptr() as *const SERVICE_STATUS_PROCESS)).dwCurrentState == SERVICE_RUNNING)
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

        let healthy = running && start_type == "auto";
        Ok(ShieldStatus {
            installed: true,
            running,
            start_type,
            config,
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
    }

    // The 8a gate probe. Run by hand and compare against `sc query GoodbyeDPI`
    // and `sc qc GoodbyeDPI`:
    //   cargo test --lib shield -- --ignored --nocapture
    #[test]
    #[ignore]
    fn dump_live_shield_status() {
        let s = status().expect("live query failed");
        println!(
            "installed={} running={} start_type={} healthy={} config={:?}",
            s.installed, s.running, s.start_type, s.healthy, s.config
        );
    }
}
