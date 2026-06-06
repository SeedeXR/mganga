// Brick 6: process control primitives, shared source between the GUI and the
// broker (via #[path] include, like guard.rs). Both sides enforce the
// protected list HERE, inside every operation, by resolving the process name
// themselves. If the name cannot be resolved, the operation is refused: not
// knowing what something is means not touching it.
//
// Native notes (see mganga-docs/docs/windows-internals.md section 3):
// - Efficiency mode is EcoQoS via SetProcessInformation(ProcessPowerThrottling)
//   plus IDLE_PRIORITY_CLASS, exactly what Task Manager's leaf means. It can
//   be set but not read back, so callers track their own state.
// - Suspend/resume are NtSuspendProcess/NtResumeProcess from ntdll, the same
//   mechanism System Informer uses. Plain suspend, no freeze objects needed.

use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, ERROR_ACCESS_DENIED, HANDLE};
use windows::Win32::System::Threading::{
    GetPriorityClass, OpenProcess, QueryFullProcessImageNameW, SetPriorityClass,
    SetProcessInformation, TerminateProcess, IDLE_PRIORITY_CLASS, NORMAL_PRIORITY_CLASS,
    PROCESS_ACCESS_RIGHTS, PROCESS_NAME_WIN32, PROCESS_POWER_THROTTLING_CURRENT_VERSION,
    PROCESS_POWER_THROTTLING_EXECUTION_SPEED, PROCESS_POWER_THROTTLING_STATE,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_INFORMATION, PROCESS_SUSPEND_RESUME,
    PROCESS_TERMINATE, ProcessPowerThrottling,
};

#[link(name = "ntdll")]
extern "system" {
    fn NtSuspendProcess(processhandle: HANDLE) -> i32;
    fn NtResumeProcess(processhandle: HANDLE) -> i32;
}

/// Processes that keep the session, boot, or the user's security alive.
/// Killing or suspending any of these breaks Windows out from under the user.
/// Mganga and its broker are here too: the healer does not operate on itself.
const PROTECTED_PROCESSES: &[&str] = &[
    "system", "registry", "memory compression", "smss", "csrss", "wininit",
    "winlogon", "services", "lsass", "svchost", "dwm", "fontdrvhost",
    "sihost", "explorer", "ctfmon", "audiodg", "ntoskrnl",
    // Windows Defender / Security
    "msmpeng", "nissrv", "mpdefendercoreservice", "securityhealthservice",
    "securityhealthsystray",
    // ourselves
    "mganga", "mganga-broker",
];

pub fn is_protected_process(name: &str) -> bool {
    PROTECTED_PROCESSES.contains(&name)
}

/// Lowercase executable base name without .exe, resolved from the live
/// process, not taken from the caller.
pub fn process_name(pid: u32) -> Result<String, String> {
    unsafe {
        let handle = open(pid, PROCESS_QUERY_LIMITED_INFORMATION)?;
        let mut buf = vec![0u16; 1024];
        let mut len = buf.len() as u32;
        let result =
            QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len);
        let _ = CloseHandle(handle);
        result.map_err(|e| format!("name of pid {pid}: {e}"))?;
        let full = String::from_utf16_lossy(&buf[..len as usize]);
        let base = full.rsplit('\\').next().unwrap_or(&full).to_lowercase();
        Ok(base.trim_end_matches(".exe").to_string())
    }
}

/// Refuse to act on protected processes. Failing to resolve the name also
/// refuses: unknown means untouchable.
fn guard_protected(pid: u32) -> Result<(), String> {
    let name = process_name(pid).map_err(|_| "protected".to_string())?;
    if is_protected_process(&name) {
        return Err("protected".into());
    }
    Ok(())
}

/// Efficiency mode on/off: EcoQoS execution-speed throttling plus idle
/// priority, matching Task Manager's behavior. Cannot be read back; the
/// caller tracks what it throttled.
pub fn set_efficiency(pid: u32, on: bool) -> Result<(), String> {
    guard_protected(pid)?;
    unsafe {
        let handle = open(pid, PROCESS_SET_INFORMATION | PROCESS_QUERY_LIMITED_INFORMATION)?;
        let state = PROCESS_POWER_THROTTLING_STATE {
            Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
            ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
            StateMask: if on { PROCESS_POWER_THROTTLING_EXECUTION_SPEED } else { 0 },
        };
        let throttle = SetProcessInformation(
            handle,
            ProcessPowerThrottling,
            &state as *const _ as *const _,
            std::mem::size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
        );
        let priority = SetPriorityClass(
            handle,
            if on { IDLE_PRIORITY_CLASS } else { NORMAL_PRIORITY_CLASS },
        );
        let _ = CloseHandle(handle);
        throttle.map_err(|e| format!("throttle pid {pid}: {e}"))?;
        priority.map_err(|e| format!("priority pid {pid}: {e}"))
    }
}

pub fn suspend(pid: u32) -> Result<(), String> {
    guard_protected(pid)?;
    unsafe {
        let handle = open(pid, PROCESS_SUSPEND_RESUME)?;
        let status = NtSuspendProcess(handle);
        let _ = CloseHandle(handle);
        if status < 0 {
            return Err(format!("suspend pid {pid}: NTSTATUS {status:#x}"));
        }
        Ok(())
    }
}

pub fn resume(pid: u32) -> Result<(), String> {
    guard_protected(pid)?;
    unsafe {
        let handle = open(pid, PROCESS_SUSPEND_RESUME)?;
        let status = NtResumeProcess(handle);
        let _ = CloseHandle(handle);
        if status < 0 {
            return Err(format!("resume pid {pid}: NTSTATUS {status:#x}"));
        }
        Ok(())
    }
}

pub fn kill(pid: u32) -> Result<(), String> {
    guard_protected(pid)?;
    unsafe {
        let handle = open(pid, PROCESS_TERMINATE)?;
        let result = TerminateProcess(handle, 1);
        let _ = CloseHandle(handle);
        result.map_err(|e| format!("kill pid {pid}: {e}"))
    }
}

/// Current priority class, used by tests to verify the idle-priority half of
/// Efficiency mode (the EcoQoS half cannot be read back).
pub fn priority_class(pid: u32) -> Result<u32, String> {
    unsafe {
        let handle = open(pid, PROCESS_QUERY_LIMITED_INFORMATION)?;
        let class = GetPriorityClass(handle);
        let _ = CloseHandle(handle);
        if class == 0 {
            return Err("GetPriorityClass failed".into());
        }
        Ok(class)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::process::CommandExt;

    /// The full control arc on a sacrificial child process: throttle,
    /// un-throttle, suspend, resume, kill. Priority class verifies the
    /// readable half of Efficiency mode.
    #[test]
    fn lifecycle_on_disposable_child() {
        let mut child = std::process::Command::new("cmd")
            .args(["/C", "ping -n 30 127.0.0.1 > nul"])
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
            .spawn()
            .expect("spawn sacrificial child");
        let pid = child.id();

        assert_eq!(process_name(pid).unwrap(), "cmd");

        set_efficiency(pid, true).unwrap();
        assert_eq!(priority_class(pid).unwrap(), IDLE_PRIORITY_CLASS.0);
        set_efficiency(pid, false).unwrap();
        assert_eq!(priority_class(pid).unwrap(), NORMAL_PRIORITY_CLASS.0);

        suspend(pid).unwrap();
        resume(pid).unwrap();

        kill(pid).unwrap();
        let status = child.wait().expect("child reaped");
        assert_eq!(status.code(), Some(1), "killed with exit code 1");
    }

    /// The guard refuses protected processes by live name resolution.
    #[test]
    fn guard_refuses_protected_process() {
        assert!(is_protected_process("lsass"));
        assert!(is_protected_process("mganga-broker"));
        assert!(!is_protected_process("spotify"));

        // Find a real protected process and confirm every op refuses it.
        let sys = sysinfo::System::new_all();
        let explorer = sys
            .processes()
            .iter()
            .find(|(_, p)| p.name().to_string_lossy().eq_ignore_ascii_case("explorer.exe"))
            .map(|(pid, _)| pid.as_u32());
        if let Some(pid) = explorer {
            assert_eq!(suspend(pid), Err("protected".to_string()));
            assert_eq!(kill(pid), Err("protected".to_string()));
            assert_eq!(set_efficiency(pid, true), Err("protected".to_string()));
        }
    }
}

/// "access-denied" is a distinct error so the GUI knows to retry through the
/// elevated broker.
fn open(pid: u32, access: PROCESS_ACCESS_RIGHTS) -> Result<HANDLE, String> {
    unsafe {
        OpenProcess(access, false, pid).map_err(|e| {
            if e.code() == ERROR_ACCESS_DENIED.to_hresult() {
                "access-denied".to_string()
            } else {
                format!("open pid {pid}: {e}")
            }
        })
    }
}
