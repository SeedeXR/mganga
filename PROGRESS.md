# Mganga — build progress

Last updated: 2026-06-06. All 8 bricks of `mganga-docs/docs/build-plan.md` are built.
The app runs with `npm run tauri dev` from the project root.

> Layout note: the project was flattened on 2026-06-06. The former `mganga/` subfolder's
> contents now live at the root, and a workspace `Cargo.toml` at the root makes RustRover
> detect the Rust project automatically. Open the "Mganga Project" folder in the IDE.

## Brick status

| Brick | What | Status |
|---|---|---|
| 0 | Tauri v2 + Vite + React (JS) + Tailwind skeleton, `ping` round trip | Done, gate passed |
| 1 | Elevated broker over named pipe (`\\.\pipe\mganga-broker`), JSON protocol | Done, gate passed (incl. clean exit check) |
| 2 | Autostart scanner: 6 Run/RunOnce keys, Startup folders, logon tasks, auto services, StartupApproved merge | Done, verified against independent PowerShell/CIM/WMI enumeration (21/21, 4/4, 43/43, services superset) |
| 3 | Judgment engine: protected list + `known_apps.json` (~45 rules) + heuristics + **usage layer** (UserAssist last-opened evidence) | Done, spot-checked on this machine |
| 4 | Reversible toggles: StartupApproved byte flip (02/03), HKCU direct, HKLM via broker, audit log + undo | Built, tests pass. **GATE PENDING: reboot loop** (see below) |
| 5 | Live process view: 2s polling, grouped by exe, Task-Manager-compatible numbers | Done, memory % matched WMI exactly |
| 6 | Process control: Ease off (EcoQoS+idle), Pause/Resume (NtSuspendProcess), Stop (confirm) | Built, lifecycle tests pass. **GATE PENDING: Spotify loop** (see below) |
| 7 | Polish: diagnosis sentence, verdict filters, jargon hover-explainers, process verdicts | Built. **GATE PENDING: "does it explain the slowness" read** |

## Pending gates (user actions, in order of effort)

1. **Brick 6, two minutes:** on the Right now tab, pick Spotify (or similar):
   Ease off → Full speed → Pause (CPU hits 0, audio freezes) → Resume. Stop something
   trivial and judge the confirm wording. Check svchost/explorer show the lock.
2. **Brick 7, one minute:** read the diagnosis sentence at the top of Right now.
   Pass = it explains why the machine is slow without needing to look anything up.
3. **Brick 4, next reboot:** Steam was toggled OFF in Mganga (2026-06-06). After reboot:
   Steam must NOT auto-start, Mganga must still show it Off, Task Manager Startup apps
   must agree. Then flip it back on, and try History → Undo on one change.

## Where everything lives

```
Mganga Project/            <- open this folder in RustRover
├── PROGRESS.md            <- this file
├── MINDMAP.md             <- how everything connects (architecture + data flows, Mermaid)
├── CLAUDE.md              <- session context pointer
├── Cargo.toml             <- workspace pointer (members = ["src-tauri"]) for IDE detection
├── mganga-docs/           <- the spec (read CLAUDE.md + docs/ before changing anything)
├── package.json, vite.config.js, index.html
├── src/App.jsx            <- whole frontend (tabs: Right now / Startup / History / Plumbing)
├── src/App.css            <- just the Tailwind import
└── src-tauri/
    ├── tauri.conf.json        <- beforeDevCommand also builds the broker exe
    ├── src/lib.rs             <- all Tauri commands + state (Broker, ProcState, ProcCtl)
    ├── src/bin/broker.rs      <- the elevated helper (named pipe server)
    ├── src/broker_client.rs   <- GUI side: launch (runas), connect, call
    ├── src/guard.rs           <- SHARED (also compiled into broker): path whitelist,
    │                             protected autostart names, 12-byte builder, hex
    ├── src/proc_control.rs    <- SHARED: EcoQoS/suspend/kill + protected process list
    ├── src/autostart.rs       <- Brick 2 scanner + toggle coordinates
    ├── src/judge.rs           <- verdicts: autostart (judge) + process (judge_process)
    ├── src/known_apps.json    <- the editable rules data (category -> verdict+reason)
    ├── src/usage.rs           <- UserAssist reader (last-opened evidence)
    ├── src/processes.rs       <- live snapshot, grouped + summed
    ├── src/actions.rs         <- HKCU StartupApproved writes (+ roundtrip tests)
    └── src/audit.rs           <- JSONL log at %LOCALAPPDATA%\Mganga\audit-log.jsonl

Build output goes to /target at the root (workspace), not src-tauri/target.
```

## Quirks worth remembering

- **Node is 22.0.0**, just under Vite 7's floor, so Vite is pinned to ^6. Don't bump
  Vite without bumping Node.
- **windows crate 0.62:** `LocalFree`/`HLOCAL` live in `Win32::Foundation`;
  `ConnectNamedPipe` needs the `Win32_System_IO` feature.
- **EcoQoS cannot be read back** from Windows; `ProcCtl` in lib.rs is the only record of
  what Mganga throttled/suspended (pruned as processes die, lost on app restart).
- **UserAssist run counters are often 0 on Windows 11**; the last-run FILETIME at offset
  0x3C is the trusted signal.
- **HKLM `ProductName` says "Windows 10 Pro" on Windows 11.** Known Windows quirk, not a bug.
- The schtasks logon filter matches the English word "logon" (fine on this machine;
  the COM Task Scheduler API is the proper fix for localized Windows).
- Tests double as probes: `cargo test --lib` in `src-tauri/` runs the registry roundtrip,
  process lifecycle (spawns its own victim), and dumps `target/scan-dump.json`.

## Deferred (noted, not started)

- Prefetch / SRUM usage enrichment via the broker (researched; links in memory notes)
- Toggling scheduled tasks and services (read-only today, deliberately)
- .lnk shortcut target resolution (folder items show no publisher)
- Production bundling: broker as signed sidecar, installer, icons
- Project is **not a git repo** yet; `git init` recommended before further work
