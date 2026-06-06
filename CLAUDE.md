# Mganga Project — session context

Single-root layout: the app lives at this root, the spec in `mganga-docs/`.

- **Read `mganga-docs/CLAUDE.md` first** — the always-on context (what Mganga is, the
  broker architecture, the guardrails, how to work brick by brick). Deeper docs in
  `mganga-docs/docs/`.
- **Read `PROGRESS.md`** before resuming work — build status, pending review gates,
  file map, quirks.

All 8 bricks of the build plan are built as of 2026-06-06. Three user-run gates are
pending (listed in PROGRESS.md). Follow the working rules in `mganga-docs/CLAUDE.md`:
one brick at a time, explain before building, stop at review gates, keep it lean.

Run the app: `npm run tauri dev` from this root (also builds the elevated broker).
Tests: `cargo test --lib` from `src-tauri/` (or `cargo test -p mganga --lib` from root).
The root `Cargo.toml` is just a workspace pointer so IDEs detect Rust here.
