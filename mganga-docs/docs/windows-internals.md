# Windows internals reference

This is the source of truth for every native operation Mganga performs. Every claim here
came from documentation or reverse-engineering writeups, linked at the bottom so you can
verify or go deeper. Do not invent native behavior. If something here is unclear, fetch
the source.

---

## 1. Finding autostarters (the scanner, read-only)

Task Manager's Startup tab shows a small subset. The real picture is spread across the
registry, the file system, scheduled tasks, and services. Cast the full net:

**Registry Run keys** (and their one-shot RunOnce siblings, and the 32-bit mirrors):

- `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`
- `HKCU\Software\Microsoft\Windows\CurrentVersion\RunOnce`
- `HKLM\Software\Microsoft\Windows\CurrentVersion\Run`
- `HKLM\Software\Microsoft\Windows\CurrentVersion\RunOnce`
- `HKLM\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Run`  (32-bit apps on 64-bit Windows)
- `HKLM\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\RunOnce`

The 32-bit mirror matters here. Several of Einstein's autostarters are 32-bit
(Figma Agent, AsusCertService, IDM), so a scanner that skips WOW6432Node will miss them.

**Startup folders** (drop a shortcut here and it runs at logon):

- User:   `%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup`
- All users: `%PROGRAMDATA%\Microsoft\Windows\Start Menu\Programs\Startup`

**Scheduled tasks** with a logon trigger. This is where updaters hide (Epson, JetBrains,
Google). Enumerate via the Task Scheduler COM API (`ITaskService`) or, more simply for a
first pass, parse `schtasks /query /v /fo CSV` and filter for logon triggers.

**Services** set to start automatically. Start type Automatic (2) or Automatic-Delayed.
Enumerate through the Service Control Manager (the `windows` crate's Services API). These
are heavier to judge and easier to break, so treat them as a separate, more cautious tab.

> Admin note: some of these locations (HKLM, all-users startup, system tasks, services)
> are only fully readable and writable with elevation. The scanner can read most things
> unelevated, but route writes and the deeper reads through the broker.

---

## 2. Enable / disable cleanly: the StartupApproved key

This is the most important detail in the whole project. **Do not disable an autostarter by
deleting its Run value.** Windows itself does not do that. It keeps the on/off state in a
separate key and leaves the Run entry untouched:

- `HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run`
- `HKLM\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run`
- `...\StartupApproved\Run32`         (the 32-bit / WOW6432Node counterpart)
- `...\StartupApproved\StartupFolder`  (state for items that live in a Startup folder)

The value is a 12-byte binary blob. Only the **first byte** matters to us:

- `02 00 00 00 00 00 00 00 00 00 00 00` → **enabled**
- `03 00 00 00 ...` (03 then a timestamp in the trailing bytes) → **disabled**

Rules learned from testing:

- If a Run entry has **no** matching StartupApproved value, it is treated as enabled.
- Task Manager auto-creates the StartupApproved value (with `02`) the first time you toggle.

So Mganga's logic:

- **Read true state:** for each Run entry, look up its StartupApproved value. Missing → enabled.
  First byte `02` → enabled. `03` → disabled.
- **Disable:** write a 12-byte value with first byte `03` (preserve/refresh the trailing
  timestamp bytes). Leave the Run value alone.
- **Enable:** set the first byte back to `02`.
- HKLM writes go through the broker. HKCU can be done unelevated.

This is reversible, it is exactly what Task Manager does, and it destroys nothing.

For Startup-folder items, the analogous move is the `StartupFolder` subkey state, or
moving the shortcut to a Mganga-owned "disabled" folder and recording it in the audit log.

---

## 3. Process control

**Enumerate.** `sysinfo` gives the process list with name, PID, parent, CPU %, memory,
run time. Poll it on an interval for the live view. This is unprivileged and cheap.

**Kill.** `sysinfo`'s `Process::kill()` works for the simple case. For full control
(kill tree, protected targets) use the `windows` crate: `OpenProcess` with
`PROCESS_TERMINATE`, then `TerminateProcess`. Killing processes owned by another user or
elevated processes requires the broker.

**Suspend / resume.** The standard mechanism is `NtSuspendProcess` / `NtResumeProcess`
from `ntdll` (declare them via the `windows` crate's NT bindings or `ntapi`). Open the
process with `PROCESS_SUSPEND_RESUME` access. There is also a stronger "freeze" using
state-change objects that cannot be undone by a plain resume; we do **not** need freeze for
v1, plain suspend/resume is enough. Suspend is the polite middle ground: the process stops
eating CPU but keeps its state, and resume brings it back instantly.

**Efficiency mode (EcoQoS).** This is Mganga's gentlest weapon and worth getting right.
Windows 11's "Efficiency mode" is EcoQoS, applied with `SetProcessInformation`:

```
PROCESS_POWER_THROTTLING_STATE state;
state.Version     = PROCESS_POWER_THROTTLING_CURRENT_VERSION; // = 1
state.ControlMask = PROCESS_POWER_THROTTLING_EXECUTION_SPEED;
state.StateMask   = PROCESS_POWER_THROTTLING_EXECUTION_SPEED; // on
SetProcessInformation(hProcess, ProcessPowerThrottling, &state, sizeof(state));
```

To match Task Manager's "Efficiency mode" fully, also drop the process priority class to
`IDLE_PRIORITY_CLASS`. To turn EcoQoS off, call again with `StateMask = 0`. To let Windows
decide, call with both masks `0`.

Important caveat: **you can Set EcoQoS but you cannot reliably Get it.** The read side of
that information class is not supported and returns an error. So Mganga must track its own
"I throttled this PID" state in memory and reflect that in the UI, rather than asking
Windows what the current state is.

Applying EcoQoS to a process Mganga does not own needs `PROCESS_SET_INFORMATION` access,
which for other-user or elevated targets means going through the broker.

---

## 4. Services

Enumerate and read config through the Service Control Manager (the `windows` crate Services
API: `OpenSCManager`, `EnumServicesStatusEx`, `QueryServiceConfig`). Changing a service's
start type (`ChangeServiceConfig`) or stopping it requires admin, so it is broker-only.
Services are the highest-risk tab. Lean hard on the protected list and the scary confirm
here. Many "auto" services are load-bearing.

---

## 5. The privilege boundary (why the broker exists)

Unprivileged (GUI can do directly):
- Read HKCU and most HKLM read paths
- Enumerate processes and read their stats
- Read the user Startup folder
- Toggle HKCU StartupApproved
- Kill / suspend / throttle processes the user owns and that are not protected

Privileged (must go through the elevated broker):
- Write HKLM (including HKLM StartupApproved)
- Change or stop services
- Touch processes owned by other users, or elevated, or protected (after the protected
  check passes, which it will not for the truly protected set)
- Disable/enable system scheduled tasks

Note: `tauri-plugin-autostart` is **not** for this. It only makes Mganga itself launch at
startup. It cannot read or manage other apps' autostart. The scanner is hand-rolled.

---

## Sources (fetch these to verify or go deeper)

- Autoruns / ASEP overview: https://learn.microsoft.com/en-us/sysinternals/downloads/autoruns
- StartupApproved byte format (02/03, Run/Run32): https://www.nutanix.com/en_sg/blog/windows-os-optimization-essentials-part-4-startup-items
- StartupApproved behavior (missing = enabled, auto-populate): http://windowsir.blogspot.com/2022/07/startupapprovedrun-pt-ii.html
- SetProcessInformation: https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-setprocessinformation
- PROCESS_POWER_THROTTLING_STATE: https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/ns-processthreadsapi-process_power_throttling_state
- EcoQoS explainer: https://devblogs.microsoft.com/performance-diagnostics/introducing-ecoqos/
- sysinfo Process API: https://docs.rs/sysinfo/latest/sysinfo/struct.Process.html
- windows crate PROCESS_SUSPEND_RESUME: https://microsoft.github.io/windows-docs-rs/doc/windows/Win32/System/Threading/constant.PROCESS_SUSPEND_RESUME.html
