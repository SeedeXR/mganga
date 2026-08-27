# Brick 8 — Connection shield

Status: **approved, building.** Written 2026-08-05 from a session on the Discord
unblock work. Follows the house rule: explain the logic first, get a thumbs up, then build.

> **Decisions (owner, 2026-08-27):**
> - **Scope: option (b), a card on Home.** No Connection tab. The card links to the
>   Startup tab for the service row.
> - **Network: none, ever, without asking.** Stricter than the default-off toggle this
>   spec proposed: Mganga makes no network call unless the user says yes at the moment
>   a feature genuinely needs one. The Phase 2 probe, if built, asks per use instead of
>   being an always-on background setting.
> - The zero-code `known_apps.json` entry below was applied the same day.

## What this is

Discord is blocked on this machine's connection by a name filter (the ISP reads the hostname
during connection setup and silently drops it). The fix already running on this machine is
**GoodbyeDPI**, installed as a Windows service called `GoodbyeDPI`, which splits the
connection setup so the filter cannot read the name. It is scoped to Discord domains only.

Right now that service is invisible. If it stops, Discord silently dies and nothing explains
why. This brick gives Mganga a small section that shows whether the shield is up, says in
plain language what it does, and lets the user turn it off and back on, reversibly and with
a receipt in the audit log.

The fit is real: Mganga already vets things that start with Windows and already controls
running things. The shield is both.

**Verified: the `GoodbyeDPI` service already shows up in the "Starts with Windows" tab
today.** `scan_services` in `autostart.rs` reads `HKLM\SYSTEM\CurrentControlSet\Services`
and keeps entries with `Start == 2` and `Type & 0x30 != 0`. The live service has `Start: 2`
and `Type: 16`, so it passes both filters. It is listed, it is not toggleable, and the code
comment says why: *"StartupApproved; RunOnce, tasks, and services are not (yet)."*

So this brick is not adding a thing from nothing. It is giving an entry Mganga already sees a
proper home, a real explanation, and, for the first time, the ability to act on a service.

## Zero-code first step (do this regardless)

Before any of the below, one entry in `src-tauri/src/known_apps.json` makes the existing
Startup row stop looking like an anonymous third-party service. No Rust, no UI, no build
plan needed:

```json
{
  "category": "connection",
  "exe": ["goodbyedpi.exe"],
  "name_contains": ["goodbyedpi"],
  "publisher_contains": [],
  "verdict": "keep",
  "reason": "This is what makes Discord load on this connection. Your provider blocks Discord by name, and this splits the connection setup so the block cannot read it. Turn it off and Discord stops working."
}
```

**One entry covers both tabs.** `judge` lowercases the entry name and does a `contains` match,
so `"goodbyedpi"` matches the service row in "Starts with Windows". `judge_process` reuses the
same rule list, matching on `exe` and `name_contains`, so the running process also gets this
verdict and reason in "Running now" instead of falling through to a heuristic.

That alone delivers most of the "track and trace" value the feature is for. Everything after
this is about making it nicer and making it toggleable. Ship this first, then decide whether
the rest is worth it.

---

## Two decisions to make before any code

**1. Does this belong in Mganga at all?**

Honest read: it is a stretch of the stated mission. Mganga is "a Windows resource healer",
and this is network censorship circumvention. It is not about resource cost. Three options:

- **(a) Full section.** A "Connection" tab with the shield card. Most useful, widest scope drift.
- **(b) A card on Home.** One tile that shows shield state and links to the Startup tab.
  Smallest change, keeps the mission tight. **Recommended if you want to stay lean.**
- **(c) Generalise to "Watched services."** A whitelist of services the user cares about, each
  with a health card. Discord shield is just the first entry. Most honest fit with Mganga's
  identity, most work, and speculative until there is a second entry.

The rest of this spec assumes **(a)**, because that is what was asked for. Dropping to (b) is
easy: build only the status read plus the card, skip the tab.

**2. The offline principle.**

`mganga-docs/CLAUDE.md` says: *"The judgment is offline. No network, no API key."* A shield
card that reports "Discord is reachable" needs the network. That is a direct tension and it
is your call, not mine.

There is precedent for a middle path: `updater.rs` already talks to GitHub, and it is gated
behind a single setting the user can switch off, with copy that says "Turn it off to stay
fully offline." The same shape works here.

Recommendation: **Phase 1 ships with zero network.** Service state alone (up, down, not
installed) is genuinely useful and fully offline. The network reachability probe is Phase 2,
opt-in, default **off**. That keeps the principle intact and gets value out sooner.

---

## What already exists and gets reused

| Piece | Where | How it is reused |
|---|---|---|
| Broker + named pipe | `bin/broker.rs`, `broker_client.rs` | One new op for service start/stop |
| Path whitelist pattern | `guard.rs` `ALLOWED_APPROVED_PATHS` | Same shape, for service names |
| Audit log + undo | `audit.rs`, `undo_change` in `lib.rs` | New action names, new undo arm |
| `ToggleSwitch`, `StatePill`, `VerdictTag` | `App.jsx` | Card controls, no new components |
| Settings toggle pattern | `settings.rs`, `SettingsView` | The opt-in network switch |
| Auto-service scanning | `autostart.rs` | Already lists `GoodbyeDPI` today |

## What does not exist yet

**Service control is the real work.** `PROGRESS.md` lists under Deferred: *"Toggling scheduled
tasks and services (read-only today, deliberately)."* This brick is the first service write in
the codebase. That is a meaningful expansion of what the broker can do, so treat it with the
same care as Brick 4.

Needed:

- `Win32_System_Services` added to the `windows` crate feature list in `src-tauri/Cargo.toml`.
  This is a **feature flag, not a new dependency**. The `windows` crate 0.62 is already there.
- A service-name whitelist in `guard.rs`, enforced in the broker.
- Two new broker ops.

**Good news on privilege:** reading service state needs only `SERVICE_QUERY_STATUS`, which a
normal user has. So the card shows status with **no UAC prompt**. Only the toggle needs the
broker. That mirrors the existing HKCU-direct versus HKLM-through-broker split exactly.

---

## Design

### Data model

```rust
// src-tauri/src/shield.rs
#[derive(serde::Serialize)]
pub struct ShieldStatus {
    pub installed: bool,
    pub running: bool,
    pub start_type: String,   // "auto" | "manual" | "disabled" | "unknown"
    pub config: Option<String>, // the service's command line, so the card can show the scope
    pub healthy: bool,        // installed && running && start_type == "auto"
}
```

`config` matters: the whole value of the fix is that it is scoped to Discord only. Showing the
command line lets the user see that with their own eyes rather than trusting a label.

### New file: `src-tauri/src/shield.rs`

Read-only, unelevated, std plus `windows` only.

- `OpenSCManagerW(None, None, SC_MANAGER_CONNECT)`
- `OpenServiceW(scm, "GoodbyeDPI", SERVICE_QUERY_STATUS | SERVICE_QUERY_CONFIG)`
- `QueryServiceStatusEx` for running state, `QueryServiceConfigW` for start type and binary path
- Service missing is **not an error**. Return `installed: false` and let the UI say so plainly.

Close every handle. Follow the existing style in `proc_control.rs` for unsafe blocks.

### `guard.rs` additions

```rust
/// The only services Mganga may start or stop. Anything else is refused,
/// no matter who asks. Same rule as ALLOWED_APPROVED_PATHS.
pub const ALLOWED_SERVICES: &[&str] = &["GoodbyeDPI"];

pub fn is_allowed_service(name: &str) -> bool {
    ALLOWED_SERVICES.iter().any(|s| name.eq_ignore_ascii_case(s))
}
```

This is the security boundary for the whole brick. A generic "stop any service" op would be a
serious widening of the broker's power, and it is exactly what the guardrails exist to prevent.
Keep the list literal and short.

### `broker.rs` additions

One new op. Note it returns the **previous** state so the audit log can undo it.

```rust
"set_service_running" => {
    let name = args["name"].as_str().unwrap_or("");
    let run  = args["run"].as_bool().unwrap_or(false);
    if !guard::is_allowed_service(name) {
        return json!({ "ok": false, "error": "not-allowed" });
    }
    match set_service_running(name, run) {
        Ok(was_running) => json!({ "ok": true, "result": { "was_running": was_running } }),
        Err(e) => json!({ "ok": false, "error": e }),
    }
}
```

`set_service_running` uses `StartServiceW` or `ControlService(SERVICE_CONTROL_STOP)`, opened
with `SERVICE_START | SERVICE_STOP | SERVICE_QUERY_STATUS`. Poll `QueryServiceStatusEx` until
the state settles or a short timeout expires, so the UI never reports success on a service
that is still spinning up. Treat "already in the requested state" as success, not an error.

### `lib.rs` additions

```rust
#[tauri::command]
fn get_shield_status() -> shield::ShieldStatus            // no broker, no UAC

#[tauri::command]
fn set_shield_enabled(state: State<Broker>, enable: bool) -> Result<(), String>
```

`set_shield_enabled` follows the exact shape of `set_autostart_enabled`: fail fast on the
guard check in the GUI, call the broker, then append an audit record. Register both in
`invoke_handler`.

### Audit log and undo

The existing `AuditRecord` fields are registry-shaped. Rather than bend them, map cleanly:

| Field | Value |
|---|---|
| `action` | `"stop-shield"` or `"start-shield"` |
| `hive` | `"SERVICE"` (the discriminator `undo_change` matches on) |
| `approved_path` | `""` |
| `value_name` | `"GoodbyeDPI"` |
| `old_value_hex` | `Some("running")` or `Some("stopped")` |
| `detail` | `Some("Discord shield")` |

Then add a `"SERVICE"` arm to `undo_change` that reads `old_value_hex` and calls the broker to
restore that state. Undo stays a first-class feature, as the guardrails require.

Do not reuse `"HKCU"`/`"HKLM"`. A new hive value keeps old records parsing unchanged.

### `settings.rs` additions

```rust
pub shield_probe_network: bool,   // default FALSE, Phase 2 only
```

Serde needs `#[serde(default)]` on new fields so existing `settings.json` files on disk keep
loading. This is the same reason `detail` carries `#[serde(default)]` in `AuditRecord`.

---

## Phase 2 (optional, later): the reachability probe

Only if you accept the network tension above. Default off, one settings switch, copy that says
what it does and that it can be turned off.

**Use the documented unauthenticated endpoint**, not a websocket:

```
GET https://discord.com/api/v10/gateway   ->   {"url":"wss://gateway.discord.gg"}
```

Discord's docs mark this endpoint as requiring no authentication. It is blocked by the exact
same name filter as everything else, so it is a true test: JSON back means the shield is
working, a timeout means it is not.

**Dependency note, already checked:** `reqwest` 0.13.4, `rustls` and `hyper` are **already in
`Cargo.lock`**, pulled in by `tauri-plugin-updater`. Adding `reqwest` as a direct dependency
costs one line in `Cargo.toml` and no new compile weight. A websocket probe would need
`tokio-tungstenite`, which is a genuinely new dependency. Do not do that. The REST probe
answers the same question.

**Never use the account token.** Discord's policy: *"Automating normal user accounts (generally
called 'self-bots') outside of the OAuth2/bot API is forbidden, and can result in an account
termination."* Nothing in this feature should ever read, store, or send a Discord token. The
endpoint above needs none.

Optional extra, same phase: `https://discordstatus.com/api/v2/summary.json` distinguishes
"your shield is down" from "Discord is having an outage". That distinction turned out to be
the single most useful thing during the original debugging session, because the symptoms are
identical from the user's chair.

---

## The card

One section, one card. Reuse `ToggleSwitch` and `StatePill`. States and copy:

| State | Headline | Reason line | Control |
|---|---|---|---|
| Running, auto | Discord shield is on | "Your provider blocks Discord by name. This splits the connection setup so the block cannot read it. Only Discord traffic is touched." | Toggle on |
| Stopped | Discord shield is off | "Discord will not load until you turn this back on." | Toggle off |
| Installed, start type not auto | Shield is on, but not at startup | "It is running now, but it will not come back after you restart." | Toggle plus a fix hint |
| Not installed | No shield installed | "Nothing is protecting Discord on this machine. If Discord works anyway, your connection is not filtered." | Toggle hidden |

Detail rows under the headline: service state, starts with Windows (yes or no), and the
scope line built from `config` ("Protecting: discord.com, discord.gg, discordapp.com, and 4 more").

**Scary confirm on turning it off**, as the guardrails require. State the consequence, do not
ask a generic question:

> Turn the Discord shield off? Discord will stop loading on this machine until you turn it
> back on. Everything else keeps working.

Microcopy follows `ui-guide.md`: plain, short, no em-dashes, always say the why. Put the
technical terms ("deep packet inspection", "SNI") only behind a hint explainer, never naked in
the main view.

---

## Guardrails checklist

- **Protected list.** Enforced as `ALLOWED_SERVICES` in `guard.rs`, checked inside the broker,
  which never trusts the GUI. Only `GoodbyeDPI` can ever be touched.
- **Reversible.** Start and stop are inherently reversible. Nothing is uninstalled or deleted.
- **Audit log.** Every toggle logged with the previous state, and undoable from History.
- **Throttle before kill.** Not applicable, there is no violent option here.
- **Scary confirm.** On turning the shield off, with the real consequence stated.

---

## Build order

Three small bricks, each with a gate, per the house rule.

**8a. Read-only status.** `shield.rs`, `get_shield_status`, the card rendering all four states.
No broker, no writes, no network.
*Gate:* the card matches reality. Compare against `sc query GoodbyeDPI` in a terminal. Stop the
service by hand and confirm the card follows.

**8b. The toggle.** Guard whitelist, broker op, `set_shield_enabled`, audit record, undo arm,
confirm dialog.
*Gate:* toggle off, confirm Discord actually stops loading, toggle back on, confirm it returns.
Then History shows both entries and Undo works on one. Also try the negative case: a hand-crafted
broker call with a different service name must be refused with `not-allowed`.

**8c. Network probe (only if you accepted decision 2).** Settings switch defaulted off, the REST
probe, "Discord is reachable" line, plus the status-page check.
*Gate:* with the shield off, the probe reports unreachable. With it on, reachable. With the
setting off, no network call is made at all (verify with the app's own network view or a sniffer).

---

## Tests

Match the existing style, `cargo test --lib` in `src-tauri/`:

- `is_allowed_service` accepts `GoodbyeDPI` and `goodbyedpi`, rejects `Spooler`, `""`, and
  `GoodbyeDPI2`. This is the security-critical one, so test it directly.
- `get_shield_status` on a machine where the service is absent returns `installed: false`
  rather than an error. Do not spawn a real service in tests.
- An audit record with `hive: "SERVICE"` round-trips through serialize and deserialize, and
  old registry-shaped records still parse (guards against the serde change).

---

## Deliberately left out

Per "do not add a feature that is not in the current brick":

- Installing or updating GoodbyeDPI from inside Mganga. Out of scope, and it means shipping a
  packet driver in an installer. Mganga observes and toggles what is already there.
- Editing the blacklist domain list from the UI. Read and display only.
- A general service manager. The whitelist is one entry on purpose. Add the second entry only
  when a real second case exists, and revisit option (c) then.
- Live polling of shield state. Read on tab open and after a toggle. A 2 second poll like the
  process view is not warranted for something that changes maybe twice a month.

---

## Reference material

The original debugging session produced working reference implementations, in
`C:\Users\Seede Sr\WebstormProjects\Discord Fix Mission\`:

- `discord-doctor.ps1` — the status read plus reachability plus outage check, and the
  local-versus-Discord verdict logic. The closest thing to a prototype of this card.
- `SOLUTION.md` — what the config is, why byte 2 is the split point, why `--reverse-frag` and
  `--wrong-seq` break it, and the fragility assessment.
- `unblock-log.md` — the full evidence trail, including two false positives worth reading
  before trusting any single measurement on this connection.

The live service config, for reference:

```
C:\GoodbyeDPI\goodbyedpi.exe -e 2 --blacklist C:\GoodbyeDPI\discord-domains.txt
```

One trap worth knowing: a space anywhere in the blacklist path makes goodbyedpi print
`Can't load blacklist from file!` and exit instantly. If the card ever reports the service as
stopped right after a start, that is the first thing to check.
