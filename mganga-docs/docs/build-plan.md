# Build plan

Build one brick at a time, in order. Each brick has a plain-English goal, what to build, a
definition of done, and a review gate. **At every review gate: stop, show what works, get
approval before starting the next brick.** Do not jump ahead. Do not add anything not in
the current brick.

Before writing code for a brick, write the logic in plain English first and confirm it.

---

## Brick 0 — Skeleton that runs

**Goal:** a Tauri v2 + Vite + React (plain JS) app that opens a window and proves the
frontend can call Rust and get an answer back.

**Build:** scaffold the project. One Tauri command in Rust (e.g. `ping`) returning a
string. One React screen that calls it and shows the result. Tailwind wired up.

**Done when:** `cargo tauri dev` opens a window, the button calls Rust, the answer shows.

**Gate:** confirm the stack runs before anything else is added.

---

## Brick 1 — The broker

**Goal:** the unelevated GUI can ask an elevated helper to do privileged work.

**Build:** a second small Rust binary, the broker. The GUI launches it elevated once
(ShellExecute with the `runas` verb → one UAC prompt). GUI and broker talk over a local
named pipe with a tiny JSON protocol: `{ "op": "...", "args": {...} }` →
`{ "ok": true/false, "result": ..., "error": ... }`. Start with a single privileged no-op
op (e.g. read an HKLM value) to prove the channel.

**Done when:** the GUI sends a request, the broker (elevated) handles it and replies, and
the round trip is visible in the UI. The broker exits cleanly when the GUI closes.

**Gate:** the privilege boundary works end to end. This is the spine; do not move on until
it is solid.

---

## Brick 2 — Autostart scanner (read-only)

**Goal:** a complete, honest inventory of what launches at boot, with true on/off state.

**Build:** enumerate every source in `windows-internals.md` section 1. Merge each Run entry
with its StartupApproved state (section 2) to get true enabled/disabled. Output a clean list
of entries: name, source (which key/folder/task/service), command/path, publisher if
available, enabled state, scope (user vs machine). Read-only. No actions yet. HKLM and
system reads go through the broker.

**Done when:** the list shows at least everything Task Manager's Startup tab shows, plus the
items it hides (RunOnce, 32-bit mirror, logon tasks, auto services), each with correct
enabled/disabled state.

**Gate:** completeness check against Task Manager and Autoruns. If Mganga misses a place
things start from, it is lying to the user. Verify before continuing.

---

## Brick 3 — The judgment engine (the mganga's brain)

**Goal:** turn the raw inventory into a verdict plus a plain-English why, offline.

**Build:** a bundled known-apps list (publisher / executable name → category + recommended
verdict + reason) covering the common offenders (updaters, sync clients, launchers, RGB and
vendor utilities, remote-access tools). Plus heuristics for unknowns: unsigned, in a temp or
user-writable path, generic name, etc. Each entry gets:
- a **verdict**: `safe to disable`, `keep`, `your call`, or `protected`
- a **reason**: one human sentence ("This is a printer updater. It only needs to run when
  you actually update, not at every boot.")

Keep the list as a plain data file (JSON) so it is easy to extend without touching code.

**Done when:** every inventory entry has a verdict and a reason. Protected items are marked
protected and never get a "disable" suggestion.

**Gate:** spot-check the verdicts against Einstein's real machine (TeamViewer, MEGAsync,
Epson updater, JetBrains Toolbox, Armoury Crate, etc.). The reasons must be true and useful.

---

## Brick 4 — Autostart actions (reversible)

**Goal:** turn an autostarter off and back on, safely, with a record.

**Build:** disable/enable by flipping the StartupApproved first byte (02 ↔ 03) per
`windows-internals.md`. HKCU directly, HKLM through the broker. Enforce the protected list
in the broker. Append every change to the audit log (what, when, old value, undo). Add a
"restore" path that reads the log and reverses a change.

**Done when:** toggling an entry persists, survives a reboot, appears in the audit log, and
can be undone from the log. Protected entries cannot be toggled.

**Gate:** test the full disable → reboot → still disabled → re-enable → reboot → back loop
on one real entry. Confirm nothing was deleted, only the state byte changed.

---

## Brick 5 — Live process view

**Goal:** show what is running and what it actually costs, right now.

**Build:** poll `sysinfo` on an interval. A sortable table: name, PID, CPU %, memory, and a
short "why heavy" hint when something stands out (high CPU, high memory, many instances).
Group multi-process apps (Chrome, Steam helpers) so the total is honest. This is the
"right now there is 1, 2, 3 and that is why it lags" screen.

**Done when:** the table updates live, sorts by CPU and by memory, and groups obvious
multi-process apps.

**Gate:** sanity check the numbers against Task Manager on the real machine.

---

## Brick 6 — Process control

**Goal:** act on a running process, gently first.

**Build:** per-process actions, offered in this order in the UI:
1. **Efficiency mode** (EcoQoS + idle priority) — the gentle default for "heavy but fine"
2. **Suspend / resume** — pause without losing state
3. **Kill** — last resort, behind a scary confirm

Privileged targets go through the broker. Protected list enforced in the broker. Mganga
tracks its own EcoQoS/suspend state per PID (remember: EcoQoS cannot be read back). Log
state-changing actions.

**Done when:** each action works on a normal process, protected processes are locked, the
gentle options are presented before kill, and kill requires a plain-language confirm.

**Gate:** full control loop verified on a safe target (e.g. throttle then un-throttle, then
suspend then resume Spotify or a browser tab process).

---

## Brick 7 — The why and the polish (only after the above works)

**Goal:** make it intuitive and explanatory per `docs/ui-guide.md`.

**Build:** the inline hint explainers, the verdict styling, the "right now" lag summary at
the top, the throttle-before-kill nudge, empty/edge states, the audit-log viewer.

**Done when:** a semi-technical user can open Mganga, understand why their machine is slow,
and act with confidence without looking anything up.

**Gate:** Einstein uses it on his own machine and it explains his 81%-memory situation
clearly.
