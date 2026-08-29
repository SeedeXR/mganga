import { useEffect, useRef, useState } from "react";
import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./App.css";
import lockup from "./assets/brand/mganga-lockup-dark.svg";

// ---- Dev instrumentation ----
// Every backend call is timed and kept here so the Dev tab can show what is
// slow and what failed. Vite compiles import.meta.env.DEV to false in real
// builds, so the branch and the tab drop out of what ships.
const DEV = import.meta.env.DEV;
const DEV_MAX_CALLS = 200;
const devCalls = [];

async function invoke(cmd, args) {
  if (!DEV) return tauriInvoke(cmd, args);
  const started = performance.now();
  try {
    const out = await tauriInvoke(cmd, args);
    devCalls.push({ cmd, ms: performance.now() - started, ok: true, at: Date.now() });
    return out;
  } catch (e) {
    devCalls.push({ cmd, ms: performance.now() - started, ok: false, err: String(e), at: Date.now() });
    throw e;
  } finally {
    if (devCalls.length > DEV_MAX_CALLS) devCalls.splice(0, devCalls.length - DEV_MAX_CALLS);
  }
}

// The autostart scan takes about five seconds: it shells out to schtasks and
// reads Authenticode signatures. Re-running it on every visit to a tab is what
// made the window stop responding. One cached result serves both screens that
// need it, and any change to an entry throws it away.
const SCAN_TTL_MS = 60_000;
let scanCache = null; // { at, entries }

async function scanAutostarts(force = false) {
  if (!force && scanCache && Date.now() - scanCache.at < SCAN_TTL_MS) {
    return scanCache.entries;
  }
  const entries = await invoke("scan_autostarts");
  scanCache = { at: Date.now(), entries };
  return entries;
}

// The Rust side returns short error codes. This is where they become human.
// Per the UI guide: friendly, direct, say the why, no jargon.
const BROKER_ERRORS = {
  "uac-declined":
    "You said no to the admin prompt, so the helper stayed off. Start it again whenever you are ready.",
  "broker-missing":
    "The helper program is missing from the app folder. Reinstalling Mganga should bring it back.",
  "connect-timeout":
    "The helper started but never picked up the line. Try starting it again.",
  "broker-gone":
    "The helper stopped unexpectedly. Start it again to keep going.",
  "broker-not-running":
    "That action needs the helper, and it is not running yet. Start it first.",
  protected:
    "That one is protected. Mganga will not touch the things that keep Windows or your security running.",
};

function friendlyBrokerError(code) {
  return BROKER_ERRORS[code] || `Something unexpected went wrong: ${code}`;
}

// Human labels for the scanner's entry kinds, in display order.
const KINDS = [
  { id: "run", label: "Registry startup entries" },
  { id: "folder", label: "Startup folder items" },
  { id: "task", label: "Logon scheduled tasks" },
  { id: "service", label: "Automatic services" },
];

// Hover translations for the jargon-ish source labels.
const SOURCE_HINTS = {
  run: "A list in the Windows registry where apps sign themselves up to launch when you log in.",
  folder: "A folder of shortcuts that Windows launches when you log in.",
  task: "Launched by Windows' task scheduler when you log in. A favorite hiding spot for updaters.",
  service: "A background service Windows starts automatically at boot, before you even log in.",
};

function humanDays(days) {
  if (days === 0) return "today";
  if (days === 1) return "yesterday";
  if (days < 14) return `${days} days ago`;
  if (days < 60) return `${Math.floor(days / 7)} weeks ago`;
  if (days < 365) return `${Math.floor(days / 30)} months ago`;
  if (days < 730) return "over a year ago";
  return `${Math.floor(days / 365)} years ago`;
}

// Verdict styling per the brand: near-monochrome, no traffic lights.
// Neutral paper for "safe to turn off" and "keep", caution yellow only for
// "your call", faint locked grey for "protected".
const VERDICTS = {
  "safe-to-disable": { label: "Safe to turn off", cls: "bg-paper/15 text-paper" },
  "your-call": { label: "Your call", cls: "bg-caution/15 text-caution" },
  keep: { label: "Keep", cls: "bg-paper/10 text-mute" },
  protected: { label: "\u{1F512} Protected", cls: "bg-paper/5 text-faint" },
};

// Categories the judge could not place. Only these entries show the Layer 1
// evidence block (signer, location), so known apps stay uncluttered.
const UNPLACED = new Set([
  "unknown",
  "suspicious-path",
  "third-party-service",
  "third-party-task",
]);

function EvidenceLines({ e }) {
  if (!UNPLACED.has(e.category)) return null;
  const ev = e.evidence || {};
  return (
    <div className="mt-1 space-y-0.5">
      {ev.signer ? (
        <div className={`text-xs ${ev.signature_valid ? "text-faint" : "text-caution"}`}>
          {ev.signature_valid
            ? `Signed by ${ev.signer}, signature valid`
            : `Signs as ${ev.signer}, but Windows could not verify it`}
        </div>
      ) : ev.checked_signature ? (
        <div className="text-xs text-faint">Not digitally signed</div>
      ) : null}
      {ev.location && <div className="text-xs text-faint">Located {ev.location}</div>}
    </div>
  );
}

function VerdictTag({ verdict }) {
  const v = VERDICTS[verdict] || VERDICTS["your-call"];
  return (
    <span className={`rounded-full px-2.5 py-0.5 text-xs font-medium whitespace-nowrap ${v.cls}`}>
      {v.label}
    </span>
  );
}

function ToggleSwitch({ enabled, busy, onChange }) {
  return (
    <button
      onClick={onChange}
      disabled={busy}
      title={
        enabled
          ? "Stop this from launching at startup. If it is running now, it stays running."
          : "Let it launch at startup again."
      }
      className={`relative h-5 w-9 rounded-full transition-colors disabled:opacity-50 ${
        enabled ? "bg-paper/50" : "bg-paper/15"
      }`}
    >
      <span
        className={`absolute top-0.5 h-4 w-4 rounded-full bg-paper transition-all ${
          enabled ? "left-[18px]" : "left-0.5"
        }`}
      />
    </button>
  );
}

function StatePill({ enabled }) {
  return enabled ? (
    <span className="rounded-full bg-paper/15 text-paper px-2 py-0.5 text-xs font-medium">
      On
    </span>
  ) : (
    <span className="rounded-full bg-paper/5 text-faint px-2 py-0.5 text-xs font-medium">
      Off
    </span>
  );
}

function StartupView({ initialFilter = "all", unblock, onRefreshUnblock }) {
  const [entries, setEntries] = useState(null);
  const [error, setError] = useState("");
  const [actionError, setActionError] = useState("");
  const [busy, setBusy] = useState(false);
  const [vFilter, setVFilter] = useState(initialFilter);

  async function refresh(force = false) {
    setError("");
    try {
      setEntries(await scanAutostarts(force));
    } catch (e) {
      setError(String(e));
    }
  }

  useEffect(() => {
    refresh();
  }, []);

  async function toggleEntry(entry) {
    const args = {
      hive: entry.toggle.hive,
      approvedPath: entry.toggle.approved_path,
      valueName: entry.toggle.value_name,
      enable: !entry.enabled,
    };
    setActionError("");
    setBusy(true);
    // Optimistic flip so the switch answers instantly; revert on failure.
    const flip = (on) =>
      setEntries((prev) =>
        prev.map((x) => (x === entry || (x.toggle === entry.toggle && x.name === entry.name) ? { ...x, enabled: on } : x))
      );
    flip(!entry.enabled);
    scanCache = null; // the inventory just changed, do not serve a stale copy
    try {
      await invoke("set_autostart_enabled", args);
    } catch (e) {
      if (String(e) === "broker-not-running") {
        // Machine-wide entry and the helper is not up: start it (one UAC
        // prompt), then retry once.
        try {
          await invoke("broker_start");
          await invoke("set_autostart_enabled", args);
        } catch (e2) {
          flip(entry.enabled);
          setActionError(friendlyBrokerError(String(e2)));
        }
      } else {
        flip(entry.enabled);
        setActionError(friendlyBrokerError(String(e)));
      }
    }
    setBusy(false);
  }

  if (error) {
    return <p className="text-glitch-red text-sm">{error}</p>;
  }
  if (!entries) {
    return <Loading label="Taking inventory of what starts with Windows..." />;
  }

  const offCount = entries.filter((e) => !e.enabled).length;
  const safeCount = entries.filter(
    (e) => e.verdict === "safe-to-disable" && e.enabled
  ).length;

  return (
    <div className="w-full max-w-4xl flex flex-col gap-6">
      <div className="flex items-end justify-between gap-6">
        <div>
          <p className="text-mute text-sm">
            {entries.length} things are set to start with Windows.{" "}
            {safeCount > 0
              ? `${safeCount} of them probably don't need to.`
              : "Nothing jumps out as unnecessary."}{" "}
            {offCount > 0 && `${offCount} are already turned off.`}
          </p>
          <p className="text-faint text-xs mt-1 max-w-2xl">
            This screen is about what launches itself at startup, not what is running
            right now. Turning something off here does not close it today, it stops it
            from starting by itself next time you log in. Nothing is deleted, every
            switch can be flipped back.
          </p>
        </div>
        <button
          onClick={refresh}
          className="rounded-lg bg-paper/10 hover:bg-paper/20 px-3 py-1.5 text-xs font-medium transition-colors"
        >
          Rescan
        </button>
      </div>

      {actionError && (
        <p className="rounded-md bg-paper/5 px-3 py-2 text-sm text-glitch-red">{actionError}</p>
      )}

      <div className="flex gap-1.5 flex-wrap">
        {[
          ["all", `All (${entries.length})`],
          ...Object.entries(VERDICTS)
            .map(([id, v]) => [
              id,
              `${v.label} (${entries.filter((e) => e.verdict === id).length})`,
            ])
            .filter(([id]) => entries.some((e) => e.verdict === id)),
        ].map(([id, label]) => (
          <button
            key={id}
            onClick={() => setVFilter(id)}
            className={`rounded-full px-3 py-1 text-xs font-medium transition-colors ${
              vFilter === id
                ? "bg-focus text-paper"
                : "bg-paper/5 text-mute hover:text-paper"
            }`}
          >
            {label}
          </button>
        ))}
      </div>

      <UnblockSection status={unblock} onRefresh={onRefreshUnblock} />

      {KINDS.map(({ id, label }) => {
        const group = entries.filter(
          (e) => e.kind === id && (vFilter === "all" || e.verdict === vFilter)
        );
        if (group.length === 0) return null;
        return (
          <section key={id} className="rounded-xl bg-paper/5 overflow-hidden">
            <h2 className="px-4 py-2.5 text-xs font-medium text-mute uppercase tracking-wide bg-paper/10">
              {label} ({group.length})
            </h2>
            <table className="w-full text-sm">
              <tbody>
                {group.map((e, i) => (
                  <tr
                    key={`${e.source_detail}|${e.name}|${i}`}
                    className="border-t border-paper/10"
                  >
                    <td className="px-4 py-2.5 align-top">
                      <div className="flex items-center gap-2 flex-wrap">
                        <span className="font-medium text-paper">{e.name}</span>
                        <VerdictTag verdict={e.verdict} />
                        {e.toggle && e.verdict !== "protected" ? (
                          <ToggleSwitch
                            enabled={e.enabled}
                            busy={busy}
                            onChange={() => toggleEntry(e)}
                          />
                        ) : (
                          <StatePill enabled={e.enabled} />
                        )}
                      </div>
                      <div className="text-xs text-faint mt-0.5">
                        {e.publisher || "Unknown publisher"}
                      </div>
                      <div className="text-xs text-mute mt-1 max-w-xl">{e.reason}</div>
                      <EvidenceLines e={e} />
                      {e.last_opened_days != null && (
                        <div className="text-xs text-faint mt-0.5">
                          You last opened this {humanDays(e.last_opened_days)}
                          {e.open_count != null && `, ${e.open_count} times in total`}
                        </div>
                      )}
                    </td>
                    <td className="px-2 py-2.5 align-top text-xs text-mute w-44">
                      <span title={SOURCE_HINTS[e.kind]} className="cursor-help">
                        {e.source}
                      </span>
                      <div className="text-faint">
                        {e.scope === "user" ? "just you" : "whole machine"}
                      </div>
                    </td>
                    <td className="px-4 py-2.5 align-top w-56">
                      <div
                        className="font-mono text-xs text-faint break-all line-clamp-2"
                        title={`${e.command}\n\nFrom: ${e.source_detail}`}
                      >
                        {e.command || "(no command recorded)"}
                      </div>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </section>
        );
      })}
    </div>
  );
}

function formatBytes(bytes) {
  const gb = bytes / 1024 ** 3;
  if (gb >= 1) return `${gb.toFixed(1)} GB`;
  return `${Math.round(bytes / 1024 ** 2)} MB`;
}

// The diagnosis: one honest sentence about how the machine is doing and,
// when it is strained, who is responsible. The reason Mganga exists.
// Returns { text, culprits } so the culprits render as a scannable list
// instead of being buried mid-sentence. Shared by Home and Running now.
function buildDiagnosis(snap) {
  const memPct = Math.round((snap.mem_used / snap.mem_total) * 100);
  const cpuPct = Math.round(snap.cpu_total);
  const topBy = (key) => [...snap.groups].sort((a, b) => b[key] - a[key]).slice(0, 3);
  const byCpu = () =>
    topBy("cpu").map((g) => ({ name: g.name, label: `${Math.round(g.cpu)}%`, raw: g.cpu }));
  const byMem = () =>
    topBy("memory").map((g) => ({ name: g.name, label: formatBytes(g.memory), raw: g.memory }));
  if (memPct >= 80 && cpuPct >= 60) {
    return {
      text: `Your machine is straining: ${cpuPct}% of the processor and ${memPct}% of memory are in use. The heaviest right now:`,
      culprits: byCpu(),
    };
  }
  if (memPct >= 80) {
    return {
      text: `Your machine is using ${memPct}% of its memory, which is why things feel slow. The biggest holders:`,
      culprits: byMem(),
    };
  }
  if (cpuPct >= 60) {
    return {
      text: `The processor is busy at ${cpuPct}%. The biggest reasons:`,
      culprits: byCpu(),
    };
  }
  if (memPct >= 65) {
    return {
      text: `Memory is filling up at ${memPct}%, not an emergency, but worth knowing. The biggest holders:`,
      culprits: byMem(),
    };
  }
  return {
    text: `Your machine looks comfortable right now. Nothing is hogging it.`,
    culprits: [],
  };
}

// The culprit list: one row per offender, name left, cost right, and a thin
// relative bar so magnitude reads without reading the numbers. Flame tone for
// "this is costing you", calm tone for the comfortable fallback.
function CulpritList({ culprits, tone = "flame" }) {
  if (culprits.length === 0) return null;
  const max = Math.max(...culprits.map((c) => c.raw), 1);
  const fill = tone === "flame" ? "bg-flame/70" : "bg-paper/40";
  const valueCls = tone === "flame" ? "text-flame" : "text-mute";
  return (
    <ul className="mt-2 flex flex-col gap-2">
      {culprits.map((c) => (
        <li key={c.name}>
          <div className="flex items-baseline justify-between gap-4 text-sm">
            <span className="text-paper truncate">{c.name}</span>
            <span className={`font-mono text-xs ${valueCls}`}>{c.label}</span>
          </div>
          <div className="h-1 rounded-full bg-paper/10 mt-1">
            <div
              className={`h-1 rounded-full ${fill}`}
              style={{ width: `${(c.raw / max) * 100}%` }}
            />
          </div>
        </li>
      ))}
    </ul>
  );
}

// Short plain-language note when a process group stands out. Quiet processes
// get no commentary.
function whyHeavy(group, memTotal) {
  const notes = [];
  if (group.cpu >= 20) notes.push("working the processor hard right now");
  else if (group.cpu >= 8) notes.push("keeping the processor busy");
  const memPct = (group.memory / memTotal) * 100;
  if (memPct >= 8) notes.push(`holding ${memPct.toFixed(0)}% of your memory`);
  if (group.count >= 5) notes.push(`${group.count} copies running, the total adds up`);
  return notes.join("; ");
}

// The loading state is the brand: the cowrie with an orbiting arc and its
// grooves breathing one after another. Same geometry as the lockup mark.
function Loading({ label, size = 80 }) {
  const grooves = [
    [55, 46, 65],
    [54, 54, 66],
    [53.5, 62, 66.5],
    [54, 70, 66],
    [55, 78, 65],
  ];
  return (
    <div className="flex flex-col items-center gap-3 py-8">
      <svg width={size} height={size} viewBox="0 0 120 120" aria-hidden="true">
        <circle
          className="mganga-orbit"
          cx="60"
          cy="60"
          r="52"
          fill="none"
          stroke="var(--color-paper)"
          strokeOpacity="0.25"
          strokeWidth="4"
          strokeLinecap="round"
          strokeDasharray="80 247"
        />
        {/* Shell scaled 1.25x about its center so it fills the orbit */}
        <g transform="translate(60 60) scale(1.25) translate(-60 -60)">
          <path
            fillRule="evenodd"
            fill="var(--color-paper)"
            d="M60 26 C72 30 80 46 80 60 C80 80 70 94 60 94 C50 94 40 80 40 60 C40 46 48 30 60 26 Z M60 36 Q67 60 60 84 Q53 60 60 36 Z"
          />
          <g stroke="var(--color-flame)" strokeWidth="2.4" strokeLinecap="round">
            {grooves.map(([x1, y, x2], i) => (
              <line
                key={y}
                className="mganga-breathe"
                style={{ animationDelay: `${i * 150}ms` }}
                x1={x1}
                y1={y}
                x2={x2}
                y2={y}
              />
            ))}
          </g>
        </g>
      </svg>
      {label && <span className="text-sm text-mute">{label}</span>}
    </div>
  );
}

// Inline SVG icons (stroke = currentColor so they recolor with the text;
// emoji can't do that). Used in the Running now summary card.
function CpuIcon() {
  return (
    <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" aria-hidden="true">
      <rect x="5" y="5" width="14" height="14" rx="2" />
      <rect x="9.5" y="9.5" width="5" height="5" rx="1" />
      <path d="M9 2v3M15 2v3M9 19v3M15 19v3M2 9h3M2 15h3M19 9h3M19 15h3" />
    </svg>
  );
}

function RamIcon() {
  return (
    <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" aria-hidden="true">
      <rect x="2" y="6" width="20" height="10" rx="1.5" />
      <path d="M7 9.5v3M12 9.5v3M17 9.5v3" />
      <path d="M6 16v3M10 16v3M14 16v3M18 16v3" />
    </svg>
  );
}

// A small hover target next to each action button: the visible cue that an
// explanation exists, kept outside the button so it reads as info, not action.
function InfoDot({ text }) {
  return (
    <span
      title={text}
      className="cursor-help select-none text-faint hover:text-mute text-[11px] leading-none"
    >
      ⓘ
    </span>
  );
}

// Process verdicts: what stopping it right now would cost. Hover for the why.
// Same brand scheme as autostart verdicts: neutral / caution / faint.
const PROC_VERDICTS = {
  "fine-to-stop": { label: "Fine to stop", cls: "bg-paper/15 text-paper" },
  "your-call": { label: "Your call", cls: "bg-caution/15 text-caution" },
  keep: { label: "Keep", cls: "bg-paper/10 text-mute" },
  protected: { label: "\u{1F512} Protected", cls: "bg-paper/5 text-faint" },
};

function ProcVerdictTag({ verdict, reason }) {
  const v = PROC_VERDICTS[verdict];
  if (!v) return null;
  return (
    <span
      title={reason}
      className={`rounded-full px-2 py-0.5 text-xs font-medium whitespace-nowrap cursor-help ${v.cls}`}
    >
      {v.label}
    </span>
  );
}

// Mganga's suggestions: the judgment engine says what is safe, the live
// numbers say what is costing you. Both must agree before Mganga suggests.
// Efficiency mode is the first medicine: busy, safe to slow, not yours-in-use.
const suggestThrottle = (g) =>
  !g.protected &&
  !g.throttled &&
  !g.suspended &&
  g.verdict !== "keep" &&
  g.verdict !== "protected" &&
  g.cpu >= 8;
// Pause is stronger medicine, so only for what the judge cleared entirely.
const suggestPause = (g) =>
  !g.protected && !g.suspended && g.verdict === "fine-to-stop" && g.cpu >= 2;

const CHIP_HINTS = {
  "suggest-throttle":
    "Busy right now, and nothing breaks by slowing them. Efficiency mode keeps them working, just gently.",
  "suggest-pause":
    "Nothing else depends on these, and they are using your processor. Pausing drops them to zero CPU until you resume.",
};

// The healer's order: gentle first, violent last. Hints from the UI guide.
const ACTION_HINTS = {
  throttle:
    "Tells Windows to run this slowly on its efficient cores so it stops hogging power. It keeps working, just quietly. Reversible.",
  unthrottle: "Lets it run at full speed again.",
  suspend:
    "Freezes the app where it is so it uses no CPU. Hit resume to wake it exactly where it left off.",
  resume: "Wakes it up exactly where it left off.",
  kill: "Force-closes it. Any unsaved work in it is lost.",
};

function RightNowView() {
  const [snap, setSnap] = useState(null);
  const [samples, setSamples] = useState([]);
  const [error, setError] = useState("");
  const [actionError, setActionError] = useState("");
  const [sortKey, setSortKey] = useState("cpu"); // "cpu" | "memory"
  const [showAll, setShowAll] = useState(false);
  const [filter, setFilter] = useState("all");
  const [confirmKill, setConfirmKill] = useState(null);
  const [busy, setBusy] = useState(false);
  // While the mouse is over the list, the display freezes so rows stop
  // shifting under the cursor. Polling resumes the moment the mouse leaves.
  const hoveringRef = useRef(false);
  const [hovering, setHovering] = useState(false);
  function setHover(v) {
    hoveringRef.current = v;
    setHovering(v);
  }

  async function act(group, action) {
    setActionError("");
    setBusy(true);
    const args = { pids: group.pids, name: group.name, action };
    try {
      let res = await invoke("process_action", args);
      if (res.needs_helper > 0) {
        // Some of its processes are elevated; summon the helper and retry.
        await invoke("broker_start");
        res = await invoke("process_action", args);
      }
      if (res.error) setActionError(`Partly done: ${res.error}`);
      const s = await invoke("get_processes");
      setSnap(s);
    } catch (e) {
      setActionError(friendlyBrokerError(String(e)));
    }
    setBusy(false);
  }

  useEffect(() => {
    let alive = true;
    async function poll() {
      try {
        const s = await invoke("get_processes");
        if (!alive) return;
        // The graph keeps its own time even while the list is frozen, so a
        // paused list never reads back as a minute of idleness.
        // Ring buffer: 30 samples at 2s = the last minute.
        setSamples((prev) => [
          ...prev.slice(-29),
          { cpu: s.cpu_total, mem: (s.mem_used / s.mem_total) * 100 },
        ]);
        // The list holds still while the mouse is over it so rows stop
        // shifting under the cursor. Re-checked after the await, because the
        // mouse may have arrived while this request was in flight.
        if (!hoveringRef.current) setSnap(s);
      } catch (e) {
        if (alive) setError(String(e));
      }
    }
    poll();
    const t = setInterval(poll, 2000);
    return () => {
      alive = false;
      clearInterval(t);
    };
  }, []);

  // When a suggestion filter runs dry (everything in it got treated), fall
  // back to All instead of stranding the user on an empty list.
  useEffect(() => {
    if (!snap) return;
    if (
      (filter === "suggest-throttle" && !snap.groups.some(suggestThrottle)) ||
      (filter === "suggest-pause" && !snap.groups.some(suggestPause))
    ) {
      setFilter("all");
    }
  }, [snap, filter]);

  if (error) return <p className="text-glitch-red text-sm">{error}</p>;
  if (!snap) return <Loading label="Taking the first measurement..." />;

  const memPct = Math.round((snap.mem_used / snap.mem_total) * 100);
  const cpuPct = Math.round(snap.cpu_total);
  const diagnosis = buildDiagnosis(snap);

  const sorted = [...snap.groups].sort((a, b) => b[sortKey] - a[sortKey]);
  const throttleCandidates = sorted.filter(suggestThrottle);
  const pauseCandidates = sorted.filter(suggestPause);
  const filtered =
    filter === "all"
      ? sorted
      : filter === "suggest-throttle"
        ? throttleCandidates
        : filter === "suggest-pause"
          ? pauseCandidates
          : sorted.filter((g) => g.verdict === filter);
  const visible = showAll || filter !== "all" ? filtered : filtered.slice(0, 30);

  const countFor = (id) => snap.groups.filter((g) => g.verdict === id).length;
  const chips = [
    ["all", `All (${snap.groups.length})`],
    ...(throttleCandidates.length > 0
      ? [["suggest-throttle", `Suggested: Efficiency mode (${throttleCandidates.length})`]]
      : []),
    ...(pauseCandidates.length > 0
      ? [["suggest-pause", `Suggested: Pause (${pauseCandidates.length})`]]
      : []),
    ...Object.entries(PROC_VERDICTS)
      .map(([id, v]) => [id, `${v.label} (${countFor(id)})`])
      .filter(([id]) => countFor(id) > 0),
  ];

  const sortHeader = (key, label) => (
    <button
      onClick={() => setSortKey(key)}
      className={`text-xs font-medium uppercase tracking-wide ${
        sortKey === key ? "text-paper" : "text-faint hover:text-mute"
      }`}
    >
      {label}
      {sortKey === key ? " ▾" : ""}
    </button>
  );

  return (
    <div className="w-full max-w-4xl flex flex-col gap-4">
      <div className="rounded-xl bg-paper/5 p-4 flex items-center gap-8">
        <div className="flex items-center gap-3">
          {/* The icon heats up to flame when that resource is strained, same
              thresholds as the diagnosis sentence. */}
          <span className={`rounded-lg bg-paper/10 p-2 ${cpuPct >= 60 ? "text-flame" : "text-mute"}`}>
            <CpuIcon />
          </span>
          <div>
            <div className="font-display text-2xl font-bold">{cpuPct}%</div>
            <div className="text-xs text-mute">processor in use</div>
          </div>
        </div>
        <div className="flex items-center gap-3">
          <span className={`relative rounded-lg bg-paper/10 p-2 ${memPct >= 80 ? "text-flame" : "text-mute"}`}>
            <RamIcon />
            {memPct >= 80 && (
              <span className="mganga-flicker absolute -top-2.5 -right-1.5 text-sm" aria-hidden="true">
                🔥
              </span>
            )}
          </span>
          <div>
            <div className="font-display text-2xl font-bold">{memPct}%</div>
            <div className="text-xs text-mute">
              memory in use, {formatBytes(snap.mem_used)} of {formatBytes(snap.mem_total)}
            </div>
          </div>
        </div>
        <div className="ml-2 flex-1">
          <p className="text-sm text-paper">
            {diagnosis.text}{" "}
            <InfoDot text="What is running right now and what it costs, updated every two seconds. Apps with many processes are shown as one row with the honest total." />
          </p>
          <CulpritList culprits={diagnosis.culprits} />
          {throttleCandidates.length > 0 && filter !== "suggest-throttle" && (
            <button
              onClick={() => setFilter("suggest-throttle")}
              className="mt-2 text-xs text-glitch-green hover:underline text-left"
            >
              Mganga suggests Efficiency mode for{" "}
              {throttleCandidates
                .slice(0, 2)
                .map((g) => g.name)
                .join(", ")}
              {throttleCandidates.length > 2 &&
                ` and ${throttleCandidates.length - 2} more`}{" "}
              →
            </button>
          )}
        </div>
      </div>

      <div className="rounded-xl bg-paper/5 px-4 pt-3 pb-2">
        <Sparkline samples={samples} />
        <div className="text-xs text-faint mt-1">
          the last minute · <span className="text-mute">processor</span> ·{" "}
          <span className="text-faint">memory</span>
        </div>
      </div>

      {actionError && (
        <p className="rounded-md bg-paper/5 px-3 py-2 text-sm text-glitch-red">{actionError}</p>
      )}

      <div className="flex gap-1.5 flex-wrap">
        {chips.map(([id, label]) => (
          <button
            key={id}
            onClick={() => setFilter(id)}
            title={CHIP_HINTS[id]}
            className={`rounded-full px-3 py-1 text-xs font-medium transition-colors ${
              filter === id
                ? "bg-focus text-paper"
                : id.startsWith("suggest-")
                  ? "bg-glitch-green/10 text-glitch-green hover:bg-glitch-green/20"
                  : "bg-paper/5 text-mute hover:text-paper"
            }`}
          >
            {label}
          </button>
        ))}
      </div>

      <section
        className="rounded-xl bg-paper/5 overflow-hidden"
        onMouseEnter={() => setHover(true)}
        onMouseLeave={() => setHover(false)}
      >
        <div className="grid grid-cols-[minmax(0,1fr)_60px_90px_auto] gap-2 px-4 py-2.5 bg-paper/10 items-center">
          <span className="text-xs font-medium text-faint uppercase tracking-wide">
            Name
            {hovering && (
              <span className="ml-2 normal-case font-normal text-faint">
                holding still while you aim
              </span>
            )}
          </span>
          <span className="text-right">{sortHeader("cpu", "CPU")}</span>
          <span className="text-right">{sortHeader("memory", "Memory")}</span>
          <span />
        </div>
        {visible.map((g) => {
          const note = whyHeavy(g, snap.mem_total);
          return (
            <div
              key={g.name}
              className="grid grid-cols-[minmax(0,1fr)_60px_90px_auto] gap-2 px-4 py-2 border-t border-paper/10 items-center"
            >
              <div>
                <span className="text-sm text-paper">{g.name}</span>
                {g.count > 1 && (
                  <span className="ml-2 rounded-full bg-paper/10 px-1.5 py-0.5 text-xs text-mute">
                    ×{g.count}
                  </span>
                )}
                {g.verdict && (
                  <span className="ml-2">
                    <ProcVerdictTag verdict={g.verdict} reason={g.reason} />
                  </span>
                )}
                {g.throttled && (
                  <span className="ml-2 rounded-full bg-glitch-green/15 px-1.5 py-0.5 text-xs text-glitch-green">
                    efficiency mode
                  </span>
                )}
                {g.suspended && (
                  <span className="ml-2 rounded-full bg-paper/10 px-1.5 py-0.5 text-xs text-mute">
                    paused
                  </span>
                )}
                {note && <div className="text-xs text-flame mt-0.5">{note}</div>}
              </div>
              <span className="text-right font-mono text-sm text-mute">
                {g.cpu.toFixed(1)}%
              </span>
              <span className="text-right font-mono text-sm text-mute">
                {formatBytes(g.memory)}
              </span>
              {g.protected ? (
                <span
                  className="text-right text-xs text-faint"
                  title="This keeps Windows running. Mganga won't touch it."
                >
                  🔒 protected
                </span>
              ) : (
                <div className="flex gap-2 justify-end items-center flex-wrap">
                  <span className="flex items-center gap-1">
                    <button
                      onClick={() => act(g, g.throttled ? "unthrottle" : "throttle")}
                      disabled={busy}
                      className="rounded-md bg-glitch-green hover:bg-glitch-green/85 text-ink disabled:opacity-40 px-2 py-1 text-xs font-medium transition-colors whitespace-nowrap"
                    >
                      {g.throttled ? "Full speed" : "Efficiency mode"}
                    </button>
                    <InfoDot text={ACTION_HINTS[g.throttled ? "unthrottle" : "throttle"]} />
                  </span>
                  <span className="flex items-center gap-1">
                    <button
                      onClick={() => act(g, g.suspended ? "resume" : "suspend")}
                      disabled={busy}
                      className="rounded-md bg-paper/10 hover:bg-paper/20 disabled:opacity-40 px-2 py-1 text-xs font-medium transition-colors whitespace-nowrap"
                    >
                      {g.suspended ? "▶ Resume" : "⏸ Pause"}
                    </button>
                    <InfoDot text={ACTION_HINTS[g.suspended ? "resume" : "suspend"]} />
                  </span>
                  <span className="flex items-center gap-1">
                    <button
                      onClick={() => setConfirmKill(g)}
                      disabled={busy}
                      className="rounded-md bg-transparent hover:bg-glitch-red/10 disabled:opacity-40 px-2 py-1 text-xs font-medium text-glitch-red/90 hover:text-glitch-red transition-colors whitespace-nowrap"
                    >
                      Stop
                    </button>
                    <InfoDot text={ACTION_HINTS.kill} />
                  </span>
                </div>
              )}
            </div>
          );
        })}
        {!showAll && filter === "all" && filtered.length > 30 && (
          <button
            onClick={() => setShowAll(true)}
            className="w-full px-4 py-2.5 text-xs text-faint hover:text-mute border-t border-paper/10 text-left"
          >
            Show the {filtered.length - 30} quieter ones too
          </button>
        )}
        {visible.length === 0 && (
          <p className="px-4 py-3 text-sm text-faint">
            Nothing running matches that group right now.
          </p>
        )}
      </section>

      {confirmKill && (
        <div className="fixed inset-0 bg-black/60 flex items-center justify-center z-50">
          <div className="rounded-xl bg-ink border border-glitch-red/40 p-6 max-w-sm flex flex-col gap-4 shadow-xl">
            <h3 className="font-display font-bold text-paper">
              Force-close {confirmKill.name}?
            </h3>
            <p className="text-sm text-mute">
              Any unsaved work in it will be lost.
              {confirmKill.count > 1 &&
                ` This closes all ${confirmKill.count} of its processes.`}{" "}
              If it only needs to calm down, Efficiency mode or Pause are kinder.
            </p>
            <div className="flex gap-2 justify-end">
              <button
                onClick={() => setConfirmKill(null)}
                className="rounded-lg bg-paper/10 hover:bg-paper/20 px-4 py-2 text-sm font-medium transition-colors"
              >
                Keep it running
              </button>
              <button
                onClick={() => {
                  const g = confirmKill;
                  setConfirmKill(null);
                  act(g, "kill");
                }}
                className="rounded-lg bg-glitch-red hover:bg-glitch-red/85 text-paper px-4 py-2 text-sm font-medium transition-colors"
              >
                Stop it
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

const ACTION_LABELS = {
  "disable-autostart": "Turned off",
  "enable-autostart": "Turned on",
  undo: "Undid a change to",
  "throttle-process": "Efficiency mode on for",
  "unthrottle-process": "Back to full speed:",
  "suspend-process": "Paused",
  "resume-process": "Resumed",
  "kill-process": "Stopped",
  update: "Updated Mganga",
};

function HistoryView() {
  const [records, setRecords] = useState(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);

  async function refresh() {
    try {
      setRecords(await invoke("list_audit_log"));
    } catch (e) {
      setError(String(e));
    }
  }

  useEffect(() => {
    refresh();
  }, []);

  async function undo(id) {
    setBusy(true);
    setError("");
    try {
      await invoke("undo_change", { id });
    } catch (e) {
      if (String(e) === "broker-not-running") {
        try {
          await invoke("broker_start");
          await invoke("undo_change", { id });
        } catch (e2) {
          setError(friendlyBrokerError(String(e2)));
        }
      } else {
        setError(friendlyBrokerError(String(e)));
      }
    }
    await refresh();
    setBusy(false);
  }

  if (!records) {
    return <Loading label="Reading the log..." />;
  }

  return (
    <div className="w-full max-w-2xl flex flex-col gap-4">
      <p className="text-mute text-sm">
        Every change Mganga makes is recorded here and can be undone. Nothing is ever
        deleted, only switched.
      </p>
      {error && (
        <p className="rounded-md bg-paper/5 px-3 py-2 text-sm text-glitch-red">{error}</p>
      )}
      {records.length === 0 ? (
        <p className="text-faint text-sm">No changes yet. The log starts when you flip your first switch.</p>
      ) : (
        <section className="rounded-xl bg-paper/5 overflow-hidden">
          <table className="w-full text-sm">
            <tbody>
              {records.map((r) => (
                <tr key={r.id} className="border-t border-paper/10 first:border-t-0">
                  <td className="px-4 py-2.5">
                    <span className="text-paper">
                      {ACTION_LABELS[r.action] || r.action}{" "}
                      <span className="font-medium text-paper">{r.value_name}</span>
                      {r.detail && <span className="text-mute"> ({r.detail})</span>}
                    </span>
                    <div className="text-xs text-faint">
                      {new Date(r.time_ms).toLocaleString()}
                      {r.approved_path &&
                        ` · ${r.hive === "HKCU" ? "just you" : "whole machine"}`}
                    </div>
                  </td>
                  <td className="px-4 py-2.5 text-right">
                    {r.approved_path && (
                      <button
                        onClick={() => undo(r.id)}
                        disabled={busy}
                        className="rounded-lg bg-paper/10 hover:bg-paper/20 disabled:opacity-50 px-3 py-1.5 text-xs font-medium transition-colors"
                      >
                        Undo
                      </button>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>
      )}
    </div>
  );
}

// Hand-rolled sparkline: two polylines in a stretched viewBox, no chart
// library. vector-effect keeps the stroke width honest while the svg scales.
function Sparkline({ samples }) {
  const pts = (key) =>
    samples
      .map(
        (s, i) =>
          `${(i / Math.max(samples.length - 1, 1)) * 100},${40 - (Math.min(s[key], 100) / 100) * 40}`
      )
      .join(" ");
  return (
    <svg viewBox="0 0 100 40" preserveAspectRatio="none" className="w-full h-10 block">
      <polyline
        points={pts("mem")}
        fill="none"
        stroke="var(--color-faint)"
        strokeWidth="1.5"
        vectorEffect="non-scaling-stroke"
      />
      <polyline
        points={pts("cpu")}
        fill="none"
        stroke="var(--color-paper)"
        strokeWidth="1.5"
        vectorEffect="non-scaling-stroke"
      />
    </svg>
  );
}

// Verdict bar segment colors, matching the verdict tag scheme.
const BAR_COLORS = {
  "safe-to-disable": "bg-paper/60",
  "your-call": "bg-caution/70",
  keep: "bg-paper/25",
  protected: "bg-faint/60",
};

// The landing screen: Mganga's two questions answered at a glance, each card
// a door into the full screen. Charts stay small; the sentences carry it.
// TODO(owner): the honest limit, in your words. Shown when someone hovers
// "what this cannot do". One or two sentences on the kind of block this does
// NOT fix, so nobody expects it to unblock everything. Draft below, overwrite it.
const UNBLOCK_LIMIT =
  "This only gets past one kind of block, where your provider reads the site name as the connection opens. A site blocked by address, or by the site itself, will not be helped by this.";

// Brick 8a: the connection screen, read-only. Its tab only exists when such a
// service is installed, so people who have never needed one never see it, and
// Mganga never suggests installing one.
//
// Nothing here names a particular site. The sites come from the service's own
// list on this machine, so whoever is unblocking Telegram sees Telegram.
// Spec: mganga-docs/docs/brick-8-connection-shield.md
// The unblocker is an automatic Windows service, so it is already a row in the
// list above. This is that row's explanation, folded in here rather than given
// its own tab: a screen most machines would never show.
function UnblockSection({ status, onRefresh }) {
  const [open, setOpen] = useState(false);

  // Re-read on arrival: the service can be stopped or started from outside.
  useEffect(() => {
    onRefresh();
  }, []);

  if (!status || !status.installed) return null;

  const headline = !status.running
    ? "Connection unblocker is off"
    : status.start_type === "auto"
      ? "Connection unblocker is on"
      : "It is on, but not at startup";
  const reason = !status.running
    ? "The sites on your list will not load until you turn this back on."
    : status.start_type === "auto"
      ? "Your provider blocks some sites by reading their name as the connection opens. This splits that first message so the name cannot be read. Only the sites on your list are affected, everything else goes out untouched."
      : "It is running now, but it will not come back after you restart.";

  return (
    <section className="rounded-xl bg-paper/5 overflow-hidden">
      <button
        onClick={() => setOpen((v) => !v)}
        className="w-full flex items-center gap-3 px-5 py-3.5 text-left hover:bg-paper/5 transition-colors"
      >
        <span className="text-xs text-faint w-3">{open ? "▾" : "▸"}</span>
        <span className="font-medium text-paper">{headline}</span>
        <StatePill enabled={status.running} />
        <span className="ml-auto text-xs text-mute">
          {open ? "hide" : "what is this?"}
        </span>
      </button>

      {open && (
        <div className="px-5 pb-5 pt-1 flex flex-col gap-5 border-t border-paper/10">
        <p className="text-sm text-mute max-w-prose pt-4">{reason}</p>

        {/* Scope is claimed only when the list was actually read. */}
        {status.domain_count > 0 && (
          <div>
            <p className="text-xs font-medium text-mute uppercase tracking-wide">
              Sites getting through ({status.domain_count})
            </p>
            <ul className="flex flex-wrap gap-1.5 mt-2">
              {status.domains.map((d) => (
                <li
                  key={d}
                  className="rounded-md bg-paper/10 px-2 py-1 font-mono text-xs text-paper"
                >
                  {d}
                </li>
              ))}
            </ul>
            {status.domain_count > status.domains.length && (
              <p className="text-xs text-faint mt-1.5">
                and {status.domain_count - status.domains.length} more on the list
              </p>
            )}
          </div>
        )}

        <div className="flex gap-6 flex-wrap text-xs text-mute">
          <span>
            Starts with Windows:{" "}
            <span className="text-paper">{status.start_type === "auto" ? "yes" : "no"}</span>
          </span>
          <span>
            Windows service: <span className="text-paper">GoodbyeDPI</span>
          </span>
        </div>
        <div className="flex flex-col gap-3 border-t border-paper/10 pt-4">
        <h2 className="text-xs font-medium text-mute uppercase tracking-wide">How this works</h2>
        <p className="text-sm text-mute max-w-prose">
          The tool doing this is <span className="text-paper">GoodbyeDPI</span>, a free open
          source program running as a Windows service. When your computer opens a secure
          connection, it has to send the site name in the clear before the encryption starts.
          Your provider reads the name at that moment and drops the connection.
        </p>
        <p className="text-sm text-mute max-w-prose">
          GoodbyeDPI splits that first message into pieces, so the name is never sitting there
          in one readable chunk. The site you are visiting reassembles it normally and never
          notices. Your traffic still goes straight to the site, so nothing is rerouted through
          another country and there is no speed cost. This is not a VPN.
        </p>
        <div>
          <p className="text-xs font-medium text-mute uppercase tracking-wide">
            What this cannot do
          </p>
          <p className="text-sm text-mute mt-1.5 max-w-prose">{UNBLOCK_LIMIT}</p>
        </div>
        {status.config && (
          <div>
            <p className="text-xs font-medium text-mute uppercase tracking-wide">
              The exact command Windows runs
            </p>
            <p className="font-mono text-[11px] text-faint mt-1.5 break-all">{status.config}</p>
          </div>
        )}
        </div>
        </div>
      )}
    </section>
  );
}

// Dev builds only: what the frontend asked the backend, and how long each call
// took. This is the probe that found the five second autostart scan freezing
// the window, so it stays.
function DevView() {
  const [calls, setCalls] = useState([]);

  useEffect(() => {
    const tick = () => setCalls(devCalls.slice(-60).reverse());
    tick();
    const t = setInterval(tick, 1000);
    return () => clearInterval(t);
  }, []);

  // Per command: how many, how slow at worst. The worst case is what the user
  // actually feels, so it leads.
  const byCmd = {};
  for (const c of devCalls) {
    const s = (byCmd[c.cmd] ||= { n: 0, worst: 0, total: 0, failed: 0 });
    s.n += 1;
    s.total += c.ms;
    s.worst = Math.max(s.worst, c.ms);
    if (!c.ok) s.failed += 1;
  }
  const rows = Object.entries(byCmd).sort((a, b) => b[1].worst - a[1].worst);
  const slow = (ms) => (ms >= 1000 ? "text-glitch-red" : ms >= 250 ? "text-flame" : "text-mute");

  return (
    <div className="w-full max-w-4xl flex flex-col gap-4">
      <section className="rounded-xl bg-paper/5 p-5 flex flex-col gap-3">
        <div className="flex items-center justify-between">
          <h2 className="text-xs font-medium text-mute uppercase tracking-wide">
            Backend calls, worst first
          </h2>
          <button
            onClick={() => {
              devCalls.length = 0;
              setCalls([]);
            }}
            className="rounded-md bg-paper/10 hover:bg-paper/20 px-3 py-1 text-xs transition-colors"
          >
            Clear
          </button>
        </div>
        {rows.length === 0 ? (
          <p className="text-sm text-mute">Nothing called yet.</p>
        ) : (
          <table className="w-full text-sm">
            <thead>
              <tr className="text-xs text-faint text-left">
                <th className="font-medium pb-1">command</th>
                <th className="font-medium pb-1 text-right">calls</th>
                <th className="font-medium pb-1 text-right">avg</th>
                <th className="font-medium pb-1 text-right">worst</th>
                <th className="font-medium pb-1 text-right">failed</th>
              </tr>
            </thead>
            <tbody>
              {rows.map(([cmd, s]) => (
                <tr key={cmd} className="border-t border-paper/5">
                  <td className="py-1 font-mono text-xs text-paper">{cmd}</td>
                  <td className="py-1 text-right text-mute">{s.n}</td>
                  <td className="py-1 text-right text-mute">{Math.round(s.total / s.n)} ms</td>
                  <td className={`py-1 text-right ${slow(s.worst)}`}>{Math.round(s.worst)} ms</td>
                  <td className={`py-1 text-right ${s.failed ? "text-glitch-red" : "text-faint"}`}>
                    {s.failed}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
        <p className="text-xs text-faint">
          Anything past 250 ms is warm, past 1000 ms the window can feel stuck. A sync Tauri
          command runs on the main thread, so slow ones need `#[tauri::command(async)]`.
        </p>
      </section>

      <section className="rounded-xl bg-paper/5 p-5 flex flex-col gap-2">
        <h2 className="text-xs font-medium text-mute uppercase tracking-wide">
          Last {calls.length} calls, newest first
        </h2>
        <ul className="flex flex-col gap-0.5 font-mono text-xs max-h-80 overflow-y-auto">
          {calls.map((c, i) => (
            <li key={i} className="flex gap-3">
              <span className="text-faint w-20 shrink-0">
                {new Date(c.at).toLocaleTimeString()}
              </span>
              <span className={`w-16 shrink-0 text-right ${slow(c.ms)}`}>
                {Math.round(c.ms)} ms
              </span>
              <span className={c.ok ? "text-paper" : "text-glitch-red"}>
                {c.cmd}
                {!c.ok && ` : ${c.err}`}
              </span>
            </li>
          ))}
        </ul>
      </section>
    </div>
  );
}

// Home is not a menu of previews any more. It answers the live question in
// full (RightNowView below) and carries the startup question as a banner, so
// there is one screen per question instead of a landing page in front of them.
function HomeView({ onGo }) {
  const [entries, setEntries] = useState(null);
  const [scanError, setScanError] = useState("");

  useEffect(() => {
    scanAutostarts().then(setEntries, (e) => setScanError(String(e)));
  }, []);

  const verdictCounts = entries
    ? Object.keys(VERDICTS)
        .map((id) => [id, entries.filter((e) => e.verdict === id).length])
        .filter(([, n]) => n > 0)
    : [];
  // The named entries behind the "probably don't need to" claim live on the
  // startup screen, next to their switches. The banner only counts them.
  const suggestions = entries
    ? entries.filter((e) => e.enabled && e.verdict === "safe-to-disable")
    : [];

  return (
    <div className="w-full max-w-4xl flex flex-col gap-4">
      <section className="rounded-xl bg-paper/5 px-5 py-4 flex items-center justify-between gap-6">
        {scanError ? (
          <p className="text-sm text-glitch-red">{scanError}</p>
        ) : !entries ? (
          <Loading size={40} label="Taking inventory of what starts with Windows..." />
        ) : (
          <>
            <div className="flex-1 min-w-0">
              <p className="text-sm text-paper">
                {entries.length} things are set to start with Windows.{" "}
                {suggestions.length > 0
                  ? `${suggestions.length} probably don't need to.`
                  : "Nothing jumps out as unnecessary."}
              </p>
              <div className="flex h-2 rounded-full overflow-hidden bg-paper/5 mt-2.5 max-w-md">
                {verdictCounts.map(([id, n]) => (
                  <span
                    key={id}
                    className={BAR_COLORS[id]}
                    style={{ width: `${(n / entries.length) * 100}%` }}
                  />
                ))}
              </div>
              <div className="flex gap-3 flex-wrap mt-2">
                {verdictCounts.map(([id, n]) => (
                  <span key={id} className="flex items-center gap-1.5 text-xs text-mute">
                    <span className={`h-2 w-2 rounded-full ${BAR_COLORS[id]}`} />
                    {VERDICTS[id].label} ({n})
                  </span>
                ))}
              </div>
            </div>
            <button
              onClick={() =>
                suggestions.length > 0 ? onGo("startup", "safe-to-disable") : onGo("startup")
              }
              className="shrink-0 rounded-lg bg-paper/10 hover:bg-paper/20 px-4 py-2 text-sm font-medium transition-colors"
            >
              {suggestions.length === 0
                ? "Manage startup →"
                : suggestions.length === 1
                  ? "Review it →"
                  : `Review these ${suggestions.length} →`}
            </button>
          </>
        )}
      </section>

      <RightNowView />
    </div>
  );
}

function SettingsView() {
  const [settings, setSettings] = useState(null);
  const [version, setVersion] = useState("");
  const [busy, setBusy] = useState(false);
  // null | "checking" | "current" | { update } | { error }
  const [checkState, setCheckState] = useState(null);

  useEffect(() => {
    (async () => {
      setSettings(await invoke("get_settings"));
      setVersion(await invoke("app_version"));
    })();
  }, []);

  async function toggleAuto() {
    const next = !settings.auto_update_check;
    setSettings({ ...settings, auto_update_check: next }); // optimistic
    try {
      await invoke("set_auto_update_check", { enabled: next });
    } catch {
      setSettings({ ...settings, auto_update_check: !next }); // revert on failure
    }
  }

  async function checkNow() {
    setCheckState("checking");
    try {
      const update = await invoke("check_for_update");
      setCheckState(update ? { update } : "current");
    } catch (e) {
      setCheckState({ error: String(e) });
    }
  }

  async function installNow() {
    setBusy(true);
    try {
      await invoke("install_update"); // relaunches on success, so never returns
    } catch (e) {
      setCheckState({ error: String(e) });
      setBusy(false);
    }
  }

  if (!settings) return <Loading label="Opening settings..." />;

  return (
    <div className="w-full max-w-2xl flex flex-col gap-5">
      <section className="rounded-xl bg-paper/5 p-5">
        <div className="flex items-start justify-between gap-6">
          <div>
            <h2 className="text-paper font-medium">Check for updates automatically</h2>
            <p className="text-mute text-sm mt-1">
              Mganga gets smarter every release. With this on, it quietly asks GitHub
              now and then whether a newer version exists. It sends no telemetry and no
              list of your software. Turn it off to stay fully offline.
            </p>
          </div>
          <div className="pt-1">
            <ToggleSwitch enabled={settings.auto_update_check} busy={false} onChange={toggleAuto} />
          </div>
        </div>
      </section>

      <section className="rounded-xl bg-paper/5 p-5 flex items-center justify-between gap-4">
        <div className="text-sm">
          <div className="text-paper">This is Mganga {version}</div>
          {checkState === "current" && (
            <div className="text-faint text-xs mt-1">You are on the latest version.</div>
          )}
          {checkState && checkState.update && (
            <div className="text-paper text-xs mt-1">
              Version {checkState.update.version} is ready.
            </div>
          )}
          {checkState && checkState.error && (
            <div className="text-faint text-xs mt-1">Could not check just now.</div>
          )}
        </div>
        {checkState && checkState.update ? (
          <button
            onClick={installNow}
            disabled={busy}
            className="rounded-lg bg-focus text-paper hover:opacity-90 disabled:opacity-50 px-4 py-1.5 text-sm font-medium transition-opacity"
          >
            {busy ? "Installing..." : "Install and restart"}
          </button>
        ) : (
          <button
            onClick={checkNow}
            disabled={checkState === "checking"}
            className="rounded-lg bg-paper/10 hover:bg-paper/20 disabled:opacity-50 px-4 py-1.5 text-sm font-medium transition-colors"
          >
            {checkState === "checking" ? "Checking..." : "Check for updates"}
          </button>
        )}
      </section>
    </div>
  );
}

function App() {
  const [tab, setTab] = useState("home");
  // A ready-to-install update, surfaced by the background check as a calm line.
  const [update, setUpdate] = useState(null);
  const [installing, setInstalling] = useState(false);
  // Home can deep-link into the startup screen with a verdict filter already
  // applied ("review these"). Clicking the nav tab itself resets to "all".
  const [startupFilter, setStartupFilter] = useState("all");
  // Read once here and handed to the startup screen, where the unblocker shows
  // up as one of the automatic services. Machines without one render nothing:
  // Mganga explains what it finds, it never advertises a tool.
  const [unblock, setUnblock] = useState(null);
  const refreshUnblock = () =>
    invoke("get_unblock_status").then(setUnblock, () => setUnblock(null));
  const go = (id, filter) => {
    if (id === "startup") setStartupFilter(filter || "all");
    setTab(id);
  };

  useEffect(() => {
    refreshUnblock();
  }, []);

  // The background check (Rust side) emits this when a newer version is ready.
  useEffect(() => {
    let unlisten;
    listen("update-available", (e) => setUpdate(e.payload)).then((fn) => {
      unlisten = fn;
    });
    return () => {
      if (unlisten) unlisten();
    };
  }, []);

  async function installUpdate() {
    setInstalling(true);
    try {
      await invoke("install_update"); // relaunches on success, so never returns
    } catch {
      setInstalling(false); // stay put; the Settings tab can retry/diagnose
    }
  }

  return (
    <main className="min-h-screen bg-ink text-paper flex flex-col items-center gap-6 p-8">
      <div className="w-full max-w-4xl flex items-center justify-between">
        <img src={lockup} alt="Mganga" className="h-14 w-auto -my-2" />
        <nav className="flex gap-1 rounded-lg bg-paper/5 p-1">
          {[
            ["home", "Home"],
            ["startup", "Starts with Windows"],
            ["history", "History"],
            ["settings", "Settings"],
            ...(DEV ? [["dev", "Dev"]] : []),
          ].map(([id, label]) => (
            <button
              key={id}
              onClick={() => go(id)}
              className={`rounded-md px-4 py-1.5 text-sm font-medium transition-colors ${
                tab === id ? "bg-focus text-paper" : "text-mute hover:text-paper"
              }`}
            >
              {label}
            </button>
          ))}
        </nav>
      </div>

      {update && (
        <div className="w-full max-w-4xl flex items-center justify-between gap-4 rounded-lg border border-focus/30 bg-focus/15 px-4 py-2.5">
          <span className="text-sm text-paper">
            A new version of Mganga is ready ({update.version}). It installs when you restart.
          </span>
          <button
            onClick={installUpdate}
            disabled={installing}
            className="rounded-md bg-focus px-3 py-1.5 text-sm font-medium text-paper transition-opacity hover:opacity-90 disabled:opacity-50"
          >
            {installing ? "Installing..." : "Restart now"}
          </button>
        </div>
      )}

      {tab === "home" && <HomeView onGo={go} />}
      {tab === "startup" && (
        <StartupView
          initialFilter={startupFilter}
          unblock={unblock}
          onRefreshUnblock={refreshUnblock}
        />
      )}
      {tab === "history" && <HistoryView />}
      {tab === "settings" && <SettingsView />}
      {tab === "dev" && DEV && <DevView />}
    </main>
  );
}

export default App;
