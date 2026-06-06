# Mganga

A Windows resource healer. Mganga inspects what launches at boot and what is running
right now, tells you in plain language why each thing costs you, and lets you act:
disable an autostarter, throttle a hog, suspend it, or kill it. The name is Swahili for
a healer. The guiding rule follows from that: a healer does not poison the patient.

This file is the always-on context. Keep it short. Deeper detail lives in `docs/`.
Read those when the task touches them.

- `docs/windows-internals.md` — how every native operation actually works (the source of truth)
- `docs/build-plan.md` — the brick-by-brick build order, with a stop-and-review gate after each
- `docs/references.md` — the repos to clone and borrow from, plus fetchable doc links
- `docs/ui-guide.md` — how the interface should look and talk

## What we are building

Two jobs in one window:

1. **Vet autostarters.** Find everything that launches at boot (far more than Task Manager
   shows), judge whether each one deserves to, and explain why. Turn them off reversibly.
2. **Control live processes.** Show what is running with its real cost, and let the user
   throttle, suspend, or kill, with the gentle option offered before the violent one.

The judgment is offline. No network, no API key. A bundled known-apps list plus heuristics.

## Stack

- **Shell:** Tauri v2
- **Backend:** Rust. Crates: `sysinfo` (process list, CPU, memory), the official `windows`
  crate (native Win32/NT calls), `winreg` or the `windows` registry APIs (autostart keys).
- **Frontend:** Vite + React, **plain JavaScript, no TypeScript.** Tailwind for styling.
- **Privilege model:** broker architecture (see below). Not a single elevated app.

## Architecture: the broker

The GUI runs **unelevated**. A small separate **elevated broker** process does the
privileged work (writing HKLM, changing services, touching processes owned by other users
or protected processes). The GUI talks to the broker over a local named pipe with a small
JSON request/response protocol.

Why not just elevate the whole app: elevating the WebView app means a UAC prompt every
launch, breaks drag-and-drop from Explorer, and hits a WebView2 data-directory bug under
newer Windows 11 Administrator Protection. The broker keeps the GUI clean and shrinks the
trusted surface to one tiny audited binary.

The broker is the security boundary. **The protected-list check is enforced inside the
broker**, not only in the GUI. The GUI may also check, to fail fast, but the broker never
trusts the caller.

## The guardrails (non-negotiable)

- **Protected list.** Mganga refuses to kill, suspend, or disable anything that breaks the
  session or boot: core Windows processes (csrss, wininit, services, lsass, smss, the
  logon/shell chain), the active antivirus, and services marked critical. These show in the
  UI but are locked, with a one-line why. The list lives in code and is enforced in the broker.
- **Reversible by default.** Disabling an autostarter flips a state byte, it does not delete
  the entry (see `windows-internals.md`). Every change is undoable.
- **Audit log.** Every state-changing action is appended to a local log: what, when, the old
  value, how to undo. Restore-from-log is a feature, not an afterthought.
- **Throttle before kill.** When a process is heavy but not dangerous, the UI offers
  Efficiency mode first. Killing is the last option, never the default.
- **Scary confirm.** Any risky or irreversible action requires a confirm that states the
  plain-language consequence. No silent destructive actions.

## How to work on this

- Follow `docs/build-plan.md` in order. Build **one brick at a time.**
- Before writing code for a brick, explain the logic in plain English and wait for approval.
- At each brick's review gate, stop, show what works, get a thumbs up before the next brick.
- Keep it lean. Simple over complex, complex over complicated. No premature abstraction,
  no framework where a function does. Do not add a feature that is not in the current brick.
- When you hit a native call you are unsure about, fetch the source listed in
  `docs/references.md` rather than guessing.
- User-facing copy: friendly, direct, plain language. No em-dashes. Explain the why.
