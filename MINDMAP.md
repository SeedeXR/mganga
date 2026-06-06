# Mganga — the mind map

How the pieces connect and why. Diagrams are Mermaid; RustRover and GitHub render them
in Markdown preview. For build status see `PROGRESS.md`; for the rules of the project
see `mganga-docs/CLAUDE.md`.

---

## 1. The big picture: one app, two questions

Mganga answers two different questions about the same machine, and every screen belongs
to one of them:

```mermaid
mindmap
  root((Mganga<br/>the healer))
    Why is my machine slow RIGHT NOW?
      Right now tab
        live process table, 2s poll
        diagnosis sentence at the top
        actions: Ease off / Pause / Stop
    What launches itself AT BOOT?
      Startup tab
        full autostart inventory
        verdict + plain reason per entry
        reversible On/Off toggles
    The receipts
      History tab
        audit log of every change
        per-change Undo
    The proof bench
      Plumbing tab
        Brick 0 ping, Brick 1 broker demo
```

The guiding rule from the docs: **a healer does not poison the patient.** Everything
below exists to make actions safe, reversible, and explained.

---

## 2. Process architecture: who runs with what power

Three processes at runtime. The GUI never has admin rights; one tiny audited binary does.

```mermaid
flowchart LR
    subgraph unelevated["Unelevated (your normal user)"]
        FE["WebView UI<br/>src/App.jsx<br/>(React + Tailwind)"]
        RUST["Rust core<br/>src-tauri/src/lib.rs<br/>commands + state"]
        FE <-->|"Tauri IPC<br/>invoke('command', args)"| RUST
    end

    subgraph elevated["Elevated (admin, after one UAC prompt)"]
        BROKER["mganga-broker.exe<br/>src-tauri/src/bin/broker.rs"]
    end

    RUST <-->|"named pipe \\.\pipe\mganga-broker<br/>one JSON object per line"| BROKER
    RUST -.->|"launches once via<br/>ShellExecuteW('runas')"| BROKER

    BROKER -->|writes| HKLM[("HKLM registry<br/>machine-wide")]
    BROKER -->|acts on| ELEVPROC["elevated / other-user<br/>processes"]
    RUST -->|writes| HKCU[("HKCU registry<br/>just you")]
    RUST -->|acts on| OWNPROC["your own<br/>processes"]
```

Key decisions:

- **The broker is the security boundary.** It re-validates every request itself
  (path whitelist, protected lists, name resolution) and never trusts the GUI.
- **Same rules, one source.** `guard.rs` and `proc_control.rs` are compiled into BOTH
  binaries via `#[path]` include, so GUI and broker can never drift apart.
- **Dying politely.** The broker exits on pipe EOF (GUI closed) and also watches the
  GUI's PID as a backup. No orphaned admin processes.

---

## 3. Module map: what depends on what

```mermaid
flowchart TD
    APP["src/App.jsx<br/>all four tabs"] -->|invoke| LIB["lib.rs<br/>command layer + state:<br/>Broker, ProcState, ProcCtl"]

    LIB --> AUTO["autostart.rs<br/>Brick 2: the scanner"]
    LIB --> PROC["processes.rs<br/>Brick 5: live snapshot"]
    LIB --> ACT["actions.rs<br/>Brick 4: HKCU writes"]
    LIB --> AUD["audit.rs<br/>the receipts (JSONL)"]
    LIB --> BC["broker_client.rs<br/>launch / connect / call"]
    LIB --> PCTL["proc_control.rs (shared)<br/>Brick 6: EcoQoS, suspend, kill"]
    LIB --> GRD["guard.rs (shared)<br/>whitelists + byte builder"]

    AUTO --> JUDGE["judge.rs<br/>Brick 3: verdicts"]
    AUTO --> USE["usage.rs<br/>UserAssist evidence"]
    JUDGE --> KNOWN[("known_apps.json<br/>~45 editable rules")]

    BC -.->|named pipe| BROKER["bin/broker.rs"]
    BROKER --> GRD
    BROKER --> PCTL

    style GRD fill:#1e3a2f,color:#fff
    style PCTL fill:#1e3a2f,color:#fff
    style KNOWN fill:#3a2f1e,color:#fff
```

Green nodes are the **shared safety code** (one source, two binaries). The amber node is
the **editable knowledge**: add an app to `known_apps.json` and both the Startup verdict
and the process verdict learn it, no code changes.

---

## 4. Data flow: the Startup tab (scan → judge → show → act)

```mermaid
flowchart TD
    subgraph sources["1) Cast the full net (autostart.rs)"]
        RK["6 Run/RunOnce keys<br/>HKCU + HKLM + WOW6432Node"]
        SF["2 Startup folders<br/>user + all-users"]
        ST["schtasks CSV<br/>logon-triggered tasks"]
        SV["registry Services<br/>Start==2, real services only"]
    end

    sources --> MERGE["2) Merge true state<br/>StartupApproved key:<br/>missing=on, 02=on, 03=off"]
    MERGE --> ENRICH["3) Enrich<br/>publisher from version info,<br/>usage from UserAssist (usage.rs)"]
    ENRICH --> JUDGE2["4) Judge (judge.rs)<br/>layer 1: protected list (in code)<br/>layer 2: known_apps.json, first match wins<br/>layer 3: heuristics for unknowns<br/>layer 4: usage evidence sharpens"]
    JUDGE2 --> UI2["5) Show<br/>verdict tag + reason + toggle"]

    UI2 -->|"toggle flips"| WRITE{"which hive?"}
    WRITE -->|HKCU| LOCAL["GUI writes 12-byte value<br/>02=on / 03+timestamp=off<br/>(actions.rs)"]
    WRITE -->|HKLM| VIA["broker writes it<br/>(after re-checking everything)"]
    LOCAL --> LOG["audit.rs appends:<br/>what, when, exact old bytes"]
    VIA --> LOG
    LOG --> UNDO["History tab Undo:<br/>restore exact old bytes,<br/>or delete if it never existed"]
```

The honesty trick: a Run entry existing does not mean it runs. Windows keeps the on/off
state in a separate `StartupApproved` key, and disabling **never deletes** the entry,
only flips the first byte. That is what makes every change reversible.

---

## 5. Data flow: the Right now tab (poll → group → flag → act)

```mermaid
flowchart TD
    POLL["UI polls every 2s<br/>(frozen while mouse hovers the list)"] --> SNAP["processes.rs snapshot<br/>persistent sysinfo System<br/>(CPU% is a delta between looks)"]
    SNAP --> GROUP["group by exe name, SUM costs<br/>7x steamwebhelper = one honest row<br/>CPU divided by core count = Task Manager scale"]
    GROUP --> FLAGS["lib.rs decorates each group:<br/>protected? (proc_control list)<br/>throttled/suspended? (ProcCtl memory)<br/>verdict? (judge_process: stop-cost)"]
    FLAGS --> SHOW["UI: diagnosis sentence,<br/>filter chips, why-heavy notes,<br/>Ease off / Pause / Stop"]

    SHOW -->|action on a group| FILT["drop Mganga's own descendants<br/>(our webview shares a name<br/>with other apps' webviews)"]
    FILT --> TRY["try unelevated, per PID"]
    TRY -->|ok| MEM["ProcCtl remembers it<br/>(EcoQoS cannot be read back,<br/>this memory IS the state)"]
    TRY -->|access denied| BRK["retry via broker<br/>(it re-resolves the name and<br/>re-checks protected itself)"]
    TRY -->|protected| NO["refused, everywhere"]
    BRK --> MEM
    MEM --> LOG2["audit log entry"]
```

The three actions, in the healer's order:

| Action | Mechanism | What the user is told |
|---|---|---|
| 🍃 Ease off | EcoQoS + idle priority (`SetProcessInformation`) | keeps working, just quietly; reversible |
| ⏸ Pause | `NtSuspendProcess` | frozen where it is; resume continues exactly there |
| Stop | `TerminateProcess`, behind a confirm | unsaved work is lost; gentler options suggested |

---

## 6. The safety stack (why a bug can't hurt the machine)

Defense in depth, shallowest to deepest. Each layer assumes the ones above it failed.

```mermaid
flowchart TD
    L1["1 UI: protected things show a lock, not a button<br/>locked items explain why"] --> L2
    L2["2 GUI command layer: guard checks, fail fast<br/>(allowed paths, protected names, own-descendant filter)"] --> L3
    L3["3 Broker: re-validates EVERYTHING itself<br/>path whitelist, live name resolution, protected lists"] --> L4
    L4["4 Fail closed: can't resolve a process name? refuse.<br/>unknown app? no verdict claim at all"] --> L5
    L5["5 Reversibility: nothing is deleted, ever<br/>byte flips + audit log with exact old bytes + Undo"]
```

And the named pipe itself is locked to **your user's SID** via an explicit security
descriptor, because a pipe made by an elevated process is not otherwise writable by an
unelevated one (and should not be writable by anyone else at all).

---

## 7. Where knowledge lives (and how to extend it)

| Knowledge | Lives in | Change it by |
|---|---|---|
| App verdicts + reasons (both tabs) | `src-tauri/src/known_apps.json` | editing JSON, no code |
| Protected services (startup) | `judge.rs` `PROTECTED_SERVICES` | code change, deliberately |
| Protected processes (live) | `proc_control.rs` `PROTECTED_PROCESSES` | code change, deliberately |
| Protected autostart names | `guard.rs` patterns | code change, deliberately |
| What Mganga did (throttle/suspend) | `ProcCtl` in lib.rs, in memory | lost on app restart, by design |
| Every change made to the machine | `%LOCALAPPDATA%\Mganga\audit-log.jsonl` | append-only, History tab reads it |
| User's deliberate app launches | Windows' own UserAssist key | read-only evidence |

The split is intentional: **safe-to-edit knowledge is data** (a wrong JSON edit gives a
wrong suggestion), **dangerous knowledge is code** (changing the protected list should
require a rebuild and a diff someone can review).

---

## 8. Runtime lifecycles, in one breath each

- **App start:** window opens unelevated → Right now tab polls → no UAC, no broker yet.
- **First machine-wide action:** broker launched via `runas` (one UAC prompt) → pipe
  connect with retries → stays for the session.
- **App close:** pipe drops → broker reads EOF → exits. Broker also watches the GUI PID
  in case of a crash. Nothing elevated survives the window.
- **Reboot:** disabled autostarters stay disabled (that's the registry byte, persistent);
  throttle/suspend memory resets (those processes restarted anyway).
