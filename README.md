# Mganga

A Windows resource healer. Shows what launches at boot and what is running right now,
explains in plain language what each thing costs, and lets you act: toggle autostarters
off (reversibly), or ease off / pause / stop running processes, gentle option first.
The name is Swahili for a healer; the rule that follows is that a healer does not
poison the patient.

The full spec lives in `mganga-docs/` and the build status in `PROGRESS.md`.

## Run it

```
npm install        # once
npm run tauri dev  # builds the broker, starts Vite, opens the window
```

The app runs unelevated. Machine-wide changes go through `mganga-broker.exe`, a small
elevated helper started on demand (one UAC prompt) that talks over a named pipe and
validates every request itself.

## Tests

```
cd src-tauri
cargo test --lib
```

These double as probes on the live machine: a registry write/restore roundtrip on a
dummy value, a process-control lifecycle on a self-spawned child, and scan dumps
written to `target/scan-dump.json` for eyeballing.

## Layout

- `src/App.jsx` — the whole frontend: Right now / Startup / History / Plumbing tabs
- `src-tauri/src/lib.rs` — Tauri commands and state
- `src-tauri/src/bin/broker.rs` — the elevated helper
- `src-tauri/src/guard.rs`, `proc_control.rs` — shared safety code, compiled into both
  binaries so GUI and broker enforce identical rules
- `src-tauri/src/known_apps.json` — the editable verdict rules (add apps here, no code)

## Versions pinned for a reason

Node here is 22.0.0, which is below Vite 7's requirement, so Vite is pinned to ^6 in
`package.json`. Bump Node before bumping Vite.
