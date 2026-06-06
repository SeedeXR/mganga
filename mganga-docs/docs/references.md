# References: what to clone, what to borrow

No single open-source project does what Mganga does (control plus vetting plus
explanation). So there are two roles: the **skeleton you clone and run**, and the
**encyclopedias you read from**. All three below are open source. Fetch any of them when
you need to see how something is really done, rather than guessing.

---

## Clone this: Pachtop (the skeleton)

https://github.com/pacholoamit/pachtop  — MIT license, Rust + Tauri + React + Vite.

This is Mganga's stack, running today. Clone it, run it, read it end to end. It is a
**monitor only** (no control, no autostart), so it does not give you Mganga's hard parts,
but it gives you the wiring for free.

Borrow from it:
- How Tauri commands are defined in Rust and called from React (the IPC pattern)
- The `sysinfo` polling loop and how live metrics are pushed to the frontend
- The process-table UI and live-updating chart components (lift these for Brick 5)
- Project layout for a Vite + React + Tauri app

Alternate UI reference if you want a second opinion on the process table:
NeoHtop (Rust + Tauri + Svelte process monitor): https://abdenasser.github.io/neohtop

---

## Read this: System Informer (the encyclopedia)

https://github.com/winsiderss/systeminformer  — MIT license, written in C. The successor
to Process Hacker. The definitive reference for how Windows process and service control is
really done at the native level. Too large and too C to clone-and-adapt, but when you are
unsure how a native operation works, this is ground truth.

Borrow knowledge from it for:
- Suspend / resume / freeze (it uses `NtSuspendProcess`; freeze uses state-change objects)
- Killing protected processes and process trees
- Reading and changing services beyond what `services.msc` exposes
- How it enumerates autostart entries (its own Autoruns equivalent)

Since it is MIT, you may also adapt small pieces of logic with attribution. Translate the
idea into Rust, do not paste C.

---

## Read this: AutoRunManager (the startup specialist)

https://github.com/Free-Utilities-for-Windows/AutoRunManager  — a focused Windows startup
manager that reads and writes the registry to manage startup programs, with backup,
restore, and timestamped logging.

Borrow from it for:
- The registry read/write flow for Run-key startup entries
- The backup / restore / audit-log pattern (this is exactly Mganga's reversibility and
  audit story; see how they structure the backup before a change)

---

## Doc links worth fetching

Tauri:
- Tauri v2 (frontend setup, config, commands): https://v2.tauri.app/
- Vite frontend with Tauri: https://v2.tauri.app/start/frontend/vite/
- Elevation / broker discussion (the "separate elevated helper" pattern): https://github.com/tauri-apps/tauri/discussions/4201
- Embedding a requireAdministrator manifest via WindowsAttributes (for the broker binary): https://github.com/tauri-apps/tauri/issues/11844
- The autostart plugin (only for self-launch, NOT for managing other apps): https://v2.tauri.app/plugin/autostart/

Windows internals (also listed in windows-internals.md):
- Autoruns / ASEP: https://learn.microsoft.com/en-us/sysinternals/downloads/autoruns
- StartupApproved byte format: https://www.nutanix.com/en_sg/blog/windows-os-optimization-essentials-part-4-startup-items
- StartupApproved behavior notes: http://windowsir.blogspot.com/2022/07/startupapprovedrun-pt-ii.html
- SetProcessInformation: https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-setprocessinformation
- PROCESS_POWER_THROTTLING_STATE: https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/ns-processthreadsapi-process_power_throttling_state
- EcoQoS background: https://devblogs.microsoft.com/performance-diagnostics/introducing-ecoqos/

Rust crates:
- sysinfo (process list, metrics): https://docs.rs/sysinfo/latest/sysinfo/
- windows crate (Win32/NT bindings): https://docs.rs/windows/latest/windows/
- winreg (registry): https://docs.rs/winreg/latest/winreg/

---

## License note

Pachtop, System Informer, and Tauri are all MIT (Tauri is MIT/Apache-2.0). You can learn
freely from all of them. If you adapt a non-trivial chunk of code, keep the attribution.
Translate ideas into idiomatic Rust rather than copying C verbatim.
