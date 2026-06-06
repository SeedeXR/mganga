# Contributing to Mganga

Thanks for wanting to help heal some PCs. This guide gets you from clone to
merged PR.

## The one law

**A healer does not poison the patient.** Every contribution is judged against
this first. Concretely:

1. **Nothing destructive without a confirm**, and nothing irreversible without
   a very good reason. Toggles flip bytes back and forth; they do not delete.
2. **The protected lists are sacred.** `guard.rs` (autostarts) and
   `proc_control.rs` (processes) define what Mganga refuses to touch. PRs that
   weaken them need an extraordinary case.
3. **Plain language in the UI.** No jargon without a hover explainer. Write
   sentences a non-technical person can act on. See the voice in `App.jsx`.
4. **Every machine-wide write goes through the broker**, and the broker
   re-validates it. Never bypass the pipe, never widen the whitelist casually.

## Easiest first contribution: teach Mganga an app

`src-tauri/src/known_apps.json` maps apps to categories and categories to
verdicts with reasons. Adding a rule for an app Mganga does not recognize is a
pure-JSON PR, no code. Include in the PR description what the app is and why
the verdict fits.

## Getting set up

Prerequisites: Windows 10/11, [Node.js](https://nodejs.org) 20+,
[Rust](https://rustup.rs) stable with the MSVC toolchain.

```
git clone https://github.com/SeedeXR/mganga
cd mganga
npm install
npm run tauri dev
```

`tauri dev` also compiles `mganga-broker.exe`. The app runs without the broker;
features that need it (machine-wide toggles, elevated processes) summon it with
one UAC prompt.

## Map of the codebase

```
src/App.jsx                 the whole frontend, one file by design
src/App.css                 Tailwind v4 @theme design tokens (the brand)
src-tauri/src/lib.rs        Tauri commands + state (Broker, ProcState, ProcCtl)
src-tauri/src/autostart.rs  scanner: registry keys, folders, tasks, services
src-tauri/src/judge.rs      verdict engine (autostarts + processes)
src-tauri/src/known_apps.json  the editable rules data
src-tauri/src/usage.rs      UserAssist reader (last-opened evidence)
src-tauri/src/processes.rs  live snapshots, grouped per app
src-tauri/src/proc_control.rs  EcoQoS / suspend / kill + protected processes
src-tauri/src/actions.rs    reversible StartupApproved writes
src-tauri/src/audit.rs      JSONL audit log + undo data
src-tauri/src/guard.rs      shared safety: whitelists, protected autostarts
src-tauri/src/bin/broker.rs the elevated helper (named pipe server)
src-tauri/src/broker_client.rs  GUI side: launch, connect, call
mganga-docs/                the design spec (read before big changes)
PROGRESS.md                 build history and quirks worth knowing
```

`guard.rs` and `proc_control.rs` are compiled into both the GUI and the broker.
If you change a rule, both sides change together. Keep it that way.

## UI conventions

- Design tokens only (`ink`, `paper`, `flame`, `caution`, `focus`, `mute`,
  `faint`, glitch colors), no hardcoded hexes. They live in `src/App.css`.
- Flame is reserved for "this is costing you". Green is the gentle throttle
  action. Red appears only on Stop and genuine errors.
- No emoji in the UI; use inline SVGs with `stroke="currentColor"`.
- No em-dashes in copy; use a comma or colon.
- Windows-native vocabulary where it exists ("Efficiency mode", not a made-up
  term).

## Tests

```
cargo test -p mganga --lib
```

Tests run against the live machine on purpose: the registry roundtrip writes
and restores a dummy value, the process lifecycle spawns its own victim
process. New scanners or actions should follow the same pattern: prove it on
the real OS, leave no trace.

## PR checklist

- [ ] `cargo test -p mganga --lib` passes
- [ ] `npm run build` passes
- [ ] UI changes follow the conventions above (and read calm, not alarming)
- [ ] Anything destructive is confirmed, reversible, and audited
- [ ] New jargon has a hover explainer

Maintainers can add the `build-exe` label to a PR to get a CI-built installer
artifact for manual testing.

## Reporting issues

A good report includes: what you expected, what happened, your Windows version,
and if it is a verdict you disagree with, the app name and why. Verdict
disagreements are data, we want them.

## License

By contributing you agree your contributions are licensed under
[GPL-3.0](LICENSE), the project license.
