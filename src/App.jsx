import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./App.css";
import lockup from "./assets/brand/mganga-lockup-dark.svg";

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

function StartupView({ initialFilter = "all" }) {
  const [entries, setEntries] = useState(null);
  const [error, setError] = useState("");
  const [actionError, setActionError] = useState("");
  const [busy, setBusy] = useState(false);
  const [vFilter, setVFilter] = useState(initialFilter);

  async function refresh() {
    setError("");
    try {
      setEntries(await invoke("scan_autostarts"));
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
      if (hoveringRef.current) return; // frozen while the user aims
      try {
        const s = await invoke("get_processes");
        // Re-check after the await: the mouse may have arrived while this
        // request was in flight, and a late update would shift rows anyway.
        if (alive && !hoveringRef.current) setSnap(s);
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
function HomeView({ onGo }) {
  const [snap, setSnap] = useState(null);
  const [samples, setSamples] = useState([]);
  const [entries, setEntries] = useState(null);
  const [scanError, setScanError] = useState("");

  useEffect(() => {
    let alive = true;
    async function poll() {
      try {
        const s = await invoke("get_processes");
        if (!alive) return;
        setSnap(s);
        // Ring buffer: 30 samples at 2s = the last minute.
        setSamples((prev) => [
          ...prev.slice(-29),
          { cpu: s.cpu_total, mem: (s.mem_used / s.mem_total) * 100 },
        ]);
      } catch {
        // Home stays calm; the Running now screen reports polling errors.
      }
    }
    poll();
    const t = setInterval(poll, 2000);
    return () => {
      alive = false;
      clearInterval(t);
    };
  }, []);

  useEffect(() => {
    invoke("scan_autostarts").then(setEntries, (e) => setScanError(String(e)));
  }, []);

  const memPct = snap ? Math.round((snap.mem_used / snap.mem_total) * 100) : 0;
  const cpuPct = snap ? Math.round(snap.cpu_total) : 0;
  const diag = snap ? buildDiagnosis(snap) : null;
  // When the machine is comfortable the diagnosis names nobody, so the card
  // falls back to the busiest few in a calm tone.
  const busiest = snap
    ? [...snap.groups]
        .sort((a, b) => b.cpu - a.cpu)
        .slice(0, 3)
        .map((g) => ({ name: g.name, label: `${Math.round(g.cpu)}%`, raw: g.cpu }))
    : [];

  const safeCount = entries
    ? entries.filter((e) => e.verdict === "safe-to-disable" && e.enabled).length
    : 0;
  const verdictCounts = entries
    ? Object.keys(VERDICTS)
        .map((id) => [id, entries.filter((e) => e.verdict === id).length])
        .filter(([, n]) => n > 0)
    : [];
  // The named suggestions behind the "probably don't need to" claim: still
  // launching at every boot and judged safe to turn off. Longest-unused first,
  // because that is the most convincing evidence.
  const suggestions = entries
    ? entries
        .filter((e) => e.enabled && e.verdict === "safe-to-disable")
        .sort((a, b) => (b.last_opened_days ?? -1) - (a.last_opened_days ?? -1))
    : [];

  return (
    <div className="w-full max-w-4xl grid md:grid-cols-2 gap-4 items-start">
      <section className="rounded-xl bg-paper/5 p-5 flex flex-col gap-4">
        <h2 className="text-xs font-medium text-mute uppercase tracking-wide">Right now</h2>
        {!snap ? (
          <Loading size={56} label="Taking the first measurement..." />
        ) : (
          <>
            <div>
              <p className="text-sm text-paper">{diag.text}</p>
              {diag.culprits.length > 0 ? (
                <CulpritList culprits={diag.culprits} />
              ) : (
                busiest.length > 0 && (
                  <>
                    <p className="text-xs text-mute mt-3">Busiest right now:</p>
                    <CulpritList culprits={busiest} tone="calm" />
                  </>
                )
              )}
            </div>
            <div className="flex items-center gap-6">
              <div className="flex items-center gap-2.5">
                <span className={`rounded-lg bg-paper/10 p-2 ${cpuPct >= 60 ? "text-flame" : "text-mute"}`}>
                  <CpuIcon />
                </span>
                <div>
                  <div className="font-display text-xl font-bold">{cpuPct}%</div>
                  <div className="text-xs text-mute">processor</div>
                </div>
              </div>
              <div className="flex items-center gap-2.5">
                <span className={`relative rounded-lg bg-paper/10 p-2 ${memPct >= 80 ? "text-flame" : "text-mute"}`}>
                  <RamIcon />
                  {memPct >= 80 && (
                    <span className="mganga-flicker absolute -top-2.5 -right-1.5 text-sm" aria-hidden="true">
                      🔥
                    </span>
                  )}
                </span>
                <div>
                  <div className="font-display text-xl font-bold">{memPct}%</div>
                  <div className="text-xs text-mute">memory</div>
                </div>
              </div>
            </div>
            <div>
              <Sparkline samples={samples} />
              <div className="text-xs text-faint mt-1">
                the last minute · <span className="text-mute">processor</span> ·{" "}
                <span className="text-faint">memory</span>
              </div>
            </div>
          </>
        )}
        <button
          onClick={() => onGo("rightnow")}
          className="self-start rounded-lg bg-paper/10 hover:bg-paper/20 px-4 py-2 text-sm font-medium transition-colors"
        >
          See everything running →
        </button>
      </section>

      <section className="rounded-xl bg-paper/5 p-5 flex flex-col gap-4">
        <h2 className="text-xs font-medium text-mute uppercase tracking-wide">At startup</h2>
        {scanError ? (
          <p className="text-sm text-glitch-red">{scanError}</p>
        ) : !entries ? (
          <Loading size={56} label="Taking inventory of what starts with Windows..." />
        ) : (
          <>
            <p className="text-sm text-paper">
              {entries.length} things are set to start with Windows.{" "}
              {safeCount > 0
                ? `These ${safeCount} probably don't need to:`
                : "Nothing jumps out as unnecessary."}
            </p>
            {suggestions.length > 0 && (
              <ul className="flex flex-col gap-1.5">
                {suggestions.slice(0, 5).map((e, i) => (
                  <li
                    key={`${e.source_detail}|${e.name}|${i}`}
                    className="flex items-baseline justify-between gap-4 text-sm"
                  >
                    <span className="text-paper truncate">{e.name}</span>
                    <span
                      className="text-xs text-mute whitespace-nowrap cursor-help"
                      title={e.reason}
                    >
                      {e.last_opened_days != null
                        ? `last opened ${humanDays(e.last_opened_days)}`
                        : "no recent use on record"}
                    </span>
                  </li>
                ))}
                {suggestions.length > 5 && (
                  <li className="text-xs text-faint">
                    and {suggestions.length - 5} more on the startup screen
                  </li>
                )}
              </ul>
            )}
            <div>
              <div className="flex h-2 rounded-full overflow-hidden bg-paper/5">
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
          </>
        )}
        <button
          onClick={() =>
            suggestions.length > 0 ? onGo("startup", "safe-to-disable") : onGo("startup")
          }
          className="self-start rounded-lg bg-paper/10 hover:bg-paper/20 px-4 py-2 text-sm font-medium transition-colors"
        >
          {suggestions.length === 0
            ? "Manage startup →"
            : suggestions.length === 1
              ? "Review it →"
              : `Review these ${suggestions.length} →`}
        </button>
      </section>
    </div>
  );
}

function App() {
  const [tab, setTab] = useState("home");
  // Home can deep-link into the startup screen with a verdict filter already
  // applied ("review these"). Clicking the nav tab itself resets to "all".
  const [startupFilter, setStartupFilter] = useState("all");
  const go = (id, filter) => {
    if (id === "startup") setStartupFilter(filter || "all");
    setTab(id);
  };

  return (
    <main className="min-h-screen bg-ink text-paper flex flex-col items-center gap-6 p-8">
      <div className="w-full max-w-4xl flex items-center justify-between">
        <img src={lockup} alt="Mganga" className="h-14 w-auto -my-2" />
        <nav className="flex gap-1 rounded-lg bg-paper/5 p-1">
          {[
            ["home", "Home"],
            ["rightnow", "Running now"],
            ["startup", "Starts with Windows"],
            ["history", "History"],
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

      {tab === "home" && <HomeView onGo={go} />}
      {tab === "rightnow" && <RightNowView />}
      {tab === "startup" && <StartupView initialFilter={startupFilter} />}
      {tab === "history" && <HistoryView />}
    </main>
  );
}

export default App;
