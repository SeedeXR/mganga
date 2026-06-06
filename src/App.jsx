import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./App.css";

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

// Verdict styling per the UI guide: calm colors, plain words, no alarm-red.
const VERDICTS = {
  "safe-to-disable": { label: "Safe to turn off", cls: "bg-emerald-900/50 text-emerald-300" },
  "your-call": { label: "Your call", cls: "bg-amber-900/40 text-amber-300" },
  keep: { label: "Keep", cls: "bg-slate-700 text-slate-300" },
  protected: { label: "\u{1F512} Protected", cls: "bg-slate-700 text-slate-400" },
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
        enabled ? "bg-emerald-600" : "bg-slate-600"
      }`}
    >
      <span
        className={`absolute top-0.5 h-4 w-4 rounded-full bg-white transition-all ${
          enabled ? "left-[18px]" : "left-0.5"
        }`}
      />
    </button>
  );
}

function StatePill({ enabled }) {
  return enabled ? (
    <span className="rounded-full bg-emerald-900/60 text-emerald-300 px-2 py-0.5 text-xs font-medium">
      On
    </span>
  ) : (
    <span className="rounded-full bg-slate-700 text-slate-400 px-2 py-0.5 text-xs font-medium">
      Off
    </span>
  );
}

function StartupView() {
  const [entries, setEntries] = useState(null);
  const [error, setError] = useState("");
  const [actionError, setActionError] = useState("");
  const [busy, setBusy] = useState(false);
  const [vFilter, setVFilter] = useState("all");

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
    return <p className="text-rose-300 text-sm">{error}</p>;
  }
  if (!entries) {
    return <p className="text-slate-400 text-sm">Taking inventory of what starts with Windows...</p>;
  }

  const offCount = entries.filter((e) => !e.enabled).length;
  const safeCount = entries.filter(
    (e) => e.verdict === "safe-to-disable" && e.enabled
  ).length;

  return (
    <div className="w-full max-w-4xl flex flex-col gap-6">
      <div className="flex items-end justify-between gap-6">
        <div>
          <p className="text-slate-300 text-sm">
            {entries.length} things are set to start with Windows.{" "}
            {safeCount > 0
              ? `${safeCount} of them probably don't need to.`
              : "Nothing jumps out as unnecessary."}{" "}
            {offCount > 0 && `${offCount} are already turned off.`}
          </p>
          <p className="text-slate-500 text-xs mt-1 max-w-2xl">
            This screen is about what launches itself at startup, not what is running
            right now. Turning something off here does not close it today, it stops it
            from starting by itself next time you log in. Nothing is deleted, every
            switch can be flipped back.
          </p>
        </div>
        <button
          onClick={refresh}
          className="rounded-lg bg-slate-700 hover:bg-slate-600 px-3 py-1.5 text-xs font-medium transition-colors"
        >
          Rescan
        </button>
      </div>

      {actionError && (
        <p className="rounded-md bg-slate-800 px-3 py-2 text-sm text-rose-300">{actionError}</p>
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
                ? "bg-slate-600 text-white"
                : "bg-slate-800 text-slate-400 hover:text-slate-200"
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
          <section key={id} className="rounded-xl bg-slate-800/60 overflow-hidden">
            <h2 className="px-4 py-2.5 text-xs font-medium text-slate-400 uppercase tracking-wide bg-slate-800">
              {label} ({group.length})
            </h2>
            <table className="w-full text-sm">
              <tbody>
                {group.map((e, i) => (
                  <tr
                    key={`${e.source_detail}|${e.name}|${i}`}
                    className="border-t border-slate-700/50"
                  >
                    <td className="px-4 py-2.5 align-top">
                      <div className="flex items-center gap-2 flex-wrap">
                        <span className="font-medium text-slate-100">{e.name}</span>
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
                      <div className="text-xs text-slate-500 mt-0.5">
                        {e.publisher || "Unknown publisher"}
                      </div>
                      <div className="text-xs text-slate-400 mt-1 max-w-xl">{e.reason}</div>
                      {e.last_opened_days != null && (
                        <div className="text-xs text-slate-600 mt-0.5">
                          You last opened this {humanDays(e.last_opened_days)}
                          {e.open_count != null && `, ${e.open_count} times in total`}
                        </div>
                      )}
                    </td>
                    <td className="px-2 py-2.5 align-top text-xs text-slate-400 w-44">
                      <span title={SOURCE_HINTS[e.kind]} className="cursor-help">
                        {e.source}
                      </span>
                      <div className="text-slate-600">
                        {e.scope === "user" ? "just you" : "whole machine"}
                      </div>
                    </td>
                    <td className="px-4 py-2.5 align-top w-56">
                      <div
                        className="font-mono text-xs text-slate-600 break-all line-clamp-2"
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

// A small hover target next to each action button: the visible cue that an
// explanation exists, kept outside the button so it reads as info, not action.
function InfoDot({ text }) {
  return (
    <span
      title={text}
      className="cursor-help select-none text-slate-500 hover:text-slate-300 text-[11px] leading-none"
    >
      ⓘ
    </span>
  );
}

// Process verdicts: what stopping it right now would cost. Hover for the why.
const PROC_VERDICTS = {
  "fine-to-stop": { label: "Fine to stop", cls: "bg-emerald-900/50 text-emerald-300" },
  "your-call": { label: "Your call", cls: "bg-amber-900/40 text-amber-300" },
  keep: { label: "Keep", cls: "bg-slate-700 text-slate-300" },
  protected: { label: "\u{1F512} Protected", cls: "bg-slate-700 text-slate-400" },
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

  if (error) return <p className="text-rose-300 text-sm">{error}</p>;
  if (!snap) return <p className="text-slate-400 text-sm">Taking the first measurement...</p>;

  const memPct = Math.round((snap.mem_used / snap.mem_total) * 100);
  const cpuPct = Math.round(snap.cpu_total);

  // The diagnosis: one honest sentence about how the machine is doing and,
  // when it is strained, who is responsible. The reason Mganga exists.
  const topBy = (key) => [...snap.groups].sort((a, b) => b[key] - a[key]).slice(0, 3);
  let diagnosis;
  if (memPct >= 80 && cpuPct >= 60) {
    diagnosis = `Your machine is straining: ${cpuPct}% of the processor and ${memPct}% of memory are in use. The heaviest right now: ${topBy("cpu")
      .map((g) => g.name)
      .join(", ")}.`;
  } else if (memPct >= 80) {
    diagnosis = `Your machine is using ${memPct}% of its memory, which is why things feel slow. The biggest holders are ${topBy("memory")
      .map((g) => `${g.name} (${formatBytes(g.memory)})`)
      .join(", ")}.`;
  } else if (cpuPct >= 60) {
    diagnosis = `The processor is busy at ${cpuPct}%. The biggest reasons are ${topBy("cpu")
      .map((g) => `${g.name} (${Math.round(g.cpu)}%)`)
      .join(", ")}.`;
  } else if (memPct >= 65) {
    diagnosis = `Memory is filling up at ${memPct}%. The biggest holders are ${topBy("memory")
      .map((g) => `${g.name} (${formatBytes(g.memory)})`)
      .join(", ")}. Not an emergency, but worth knowing.`;
  } else {
    diagnosis = `Your machine looks comfortable right now. Nothing is hogging it.`;
  }

  const sorted = [...snap.groups].sort((a, b) => b[sortKey] - a[sortKey]);
  const filtered =
    filter === "all" ? sorted : sorted.filter((g) => g.verdict === filter);
  const visible = showAll || filter !== "all" ? filtered : filtered.slice(0, 30);

  const countFor = (id) => snap.groups.filter((g) => g.verdict === id).length;
  const chips = [
    ["all", `All (${snap.groups.length})`],
    ...Object.entries(PROC_VERDICTS)
      .map(([id, v]) => [id, `${v.label} (${countFor(id)})`])
      .filter(([id]) => countFor(id) > 0),
  ];

  const sortHeader = (key, label) => (
    <button
      onClick={() => setSortKey(key)}
      className={`text-xs font-medium uppercase tracking-wide ${
        sortKey === key ? "text-slate-200" : "text-slate-500 hover:text-slate-300"
      }`}
    >
      {label}
      {sortKey === key ? " ▾" : ""}
    </button>
  );

  return (
    <div className="w-full max-w-4xl flex flex-col gap-4">
      <div className="rounded-xl bg-slate-800/60 p-4 flex items-center gap-8">
        <div>
          <div className="text-2xl font-semibold">{cpuPct}%</div>
          <div className="text-xs text-slate-400">processor in use</div>
        </div>
        <div>
          <div className="text-2xl font-semibold">{memPct}%</div>
          <div className="text-xs text-slate-400">
            memory in use, {formatBytes(snap.mem_used)} of {formatBytes(snap.mem_total)}
          </div>
        </div>
        <p className="text-sm text-slate-200 ml-2 flex-1">
          {diagnosis}{" "}
          <InfoDot text="What is running right now and what it costs, updated every two seconds. Apps with many processes are shown as one row with the honest total." />
        </p>
      </div>

      {actionError && (
        <p className="rounded-md bg-slate-800 px-3 py-2 text-sm text-rose-300">{actionError}</p>
      )}

      <div className="flex gap-1.5 flex-wrap">
        {chips.map(([id, label]) => (
          <button
            key={id}
            onClick={() => setFilter(id)}
            className={`rounded-full px-3 py-1 text-xs font-medium transition-colors ${
              filter === id
                ? "bg-slate-600 text-white"
                : "bg-slate-800 text-slate-400 hover:text-slate-200"
            }`}
          >
            {label}
          </button>
        ))}
      </div>

      <section
        className="rounded-xl bg-slate-800/60 overflow-hidden"
        onMouseEnter={() => setHover(true)}
        onMouseLeave={() => setHover(false)}
      >
        <div className="grid grid-cols-[minmax(0,1fr)_60px_90px_auto] gap-2 px-4 py-2.5 bg-slate-800 items-center">
          <span className="text-xs font-medium text-slate-500 uppercase tracking-wide">
            Name
            {hovering && (
              <span className="ml-2 normal-case font-normal text-slate-600">
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
              className="grid grid-cols-[minmax(0,1fr)_60px_90px_auto] gap-2 px-4 py-2 border-t border-slate-700/50 items-center"
            >
              <div>
                <span className="text-sm text-slate-100">{g.name}</span>
                {g.count > 1 && (
                  <span className="ml-2 rounded-full bg-slate-700 px-1.5 py-0.5 text-xs text-slate-400">
                    ×{g.count}
                  </span>
                )}
                {g.verdict && (
                  <span className="ml-2">
                    <ProcVerdictTag verdict={g.verdict} reason={g.reason} />
                  </span>
                )}
                {g.throttled && (
                  <span className="ml-2 rounded-full bg-emerald-900/50 px-1.5 py-0.5 text-xs text-emerald-300">
                    🍃 eased off
                  </span>
                )}
                {g.suspended && (
                  <span className="ml-2 rounded-full bg-sky-900/50 px-1.5 py-0.5 text-xs text-sky-300">
                    paused
                  </span>
                )}
                {note && <div className="text-xs text-amber-300/80 mt-0.5">{note}</div>}
              </div>
              <span className="text-right font-mono text-sm text-slate-300">
                {g.cpu.toFixed(1)}%
              </span>
              <span className="text-right font-mono text-sm text-slate-300">
                {formatBytes(g.memory)}
              </span>
              {g.protected ? (
                <span
                  className="text-right text-xs text-slate-500"
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
                      className="rounded-md bg-emerald-700/70 hover:bg-emerald-600 disabled:opacity-40 px-2 py-1 text-xs font-medium transition-colors whitespace-nowrap"
                    >
                      🍃 {g.throttled ? "Full speed" : "Ease off"}
                    </button>
                    <InfoDot text={ACTION_HINTS[g.throttled ? "unthrottle" : "throttle"]} />
                  </span>
                  <span className="flex items-center gap-1">
                    <button
                      onClick={() => act(g, g.suspended ? "resume" : "suspend")}
                      disabled={busy}
                      className="rounded-md bg-slate-700 hover:bg-slate-600 disabled:opacity-40 px-2 py-1 text-xs font-medium transition-colors whitespace-nowrap"
                    >
                      {g.suspended ? "▶ Resume" : "⏸ Pause"}
                    </button>
                    <InfoDot text={ACTION_HINTS[g.suspended ? "resume" : "suspend"]} />
                  </span>
                  <span className="flex items-center gap-1">
                    <button
                      onClick={() => setConfirmKill(g)}
                      disabled={busy}
                      className="rounded-md bg-transparent hover:bg-rose-900/40 disabled:opacity-40 px-2 py-1 text-xs font-medium text-rose-400/80 transition-colors whitespace-nowrap"
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
            className="w-full px-4 py-2.5 text-xs text-slate-500 hover:text-slate-300 border-t border-slate-700/50 text-left"
          >
            Show the {filtered.length - 30} quieter ones too
          </button>
        )}
        {visible.length === 0 && (
          <p className="px-4 py-3 text-sm text-slate-500">
            Nothing running matches that group right now.
          </p>
        )}
      </section>

      {confirmKill && (
        <div className="fixed inset-0 bg-black/60 flex items-center justify-center z-50">
          <div className="rounded-xl bg-slate-800 p-6 max-w-sm flex flex-col gap-4 shadow-xl">
            <h3 className="font-semibold text-slate-100">
              Force-close {confirmKill.name}?
            </h3>
            <p className="text-sm text-slate-300">
              Any unsaved work in it will be lost.
              {confirmKill.count > 1 &&
                ` This closes all ${confirmKill.count} of its processes.`}{" "}
              If it only needs to calm down, Ease off or Pause are kinder.
            </p>
            <div className="flex gap-2 justify-end">
              <button
                onClick={() => setConfirmKill(null)}
                className="rounded-lg bg-slate-700 hover:bg-slate-600 px-4 py-2 text-sm font-medium transition-colors"
              >
                Keep it running
              </button>
              <button
                onClick={() => {
                  const g = confirmKill;
                  setConfirmKill(null);
                  act(g, "kill");
                }}
                className="rounded-lg bg-rose-700 hover:bg-rose-600 px-4 py-2 text-sm font-medium transition-colors"
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
  "throttle-process": "Eased off",
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
    return <p className="text-slate-400 text-sm">Reading the log...</p>;
  }

  return (
    <div className="w-full max-w-2xl flex flex-col gap-4">
      <p className="text-slate-400 text-sm">
        Every change Mganga makes is recorded here and can be undone. Nothing is ever
        deleted, only switched.
      </p>
      {error && (
        <p className="rounded-md bg-slate-800 px-3 py-2 text-sm text-rose-300">{error}</p>
      )}
      {records.length === 0 ? (
        <p className="text-slate-500 text-sm">No changes yet. The log starts when you flip your first switch.</p>
      ) : (
        <section className="rounded-xl bg-slate-800/60 overflow-hidden">
          <table className="w-full text-sm">
            <tbody>
              {records.map((r) => (
                <tr key={r.id} className="border-t border-slate-700/50 first:border-t-0">
                  <td className="px-4 py-2.5">
                    <span className="text-slate-200">
                      {ACTION_LABELS[r.action] || r.action} <b>{r.value_name}</b>
                      {r.detail && <span className="text-slate-400"> ({r.detail})</span>}
                    </span>
                    <div className="text-xs text-slate-500">
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
                        className="rounded-lg bg-slate-700 hover:bg-slate-600 disabled:opacity-50 px-3 py-1.5 text-xs font-medium transition-colors"
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

function PlumbingView() {
  const [pingAnswer, setPingAnswer] = useState("");
  const [brokerState, setBrokerState] = useState("idle"); // idle | starting | connected
  const [brokerInfo, setBrokerInfo] = useState(null);
  const [hklmValue, setHklmValue] = useState("");
  const [error, setError] = useState("");

  async function ping() {
    setPingAnswer(await invoke("ping"));
  }

  async function startBroker() {
    setError("");
    setBrokerState("starting");
    try {
      const result = await invoke("broker_start");
      setBrokerInfo(result);
      setBrokerState("connected");
    } catch (e) {
      setBrokerState("idle");
      setError(friendlyBrokerError(String(e)));
    }
  }

  async function readHklm() {
    setError("");
    try {
      const value = await invoke("broker_read_hklm", {
        path: "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion",
        name: "ProductName",
      });
      setHklmValue(value);
    } catch (e) {
      setError(friendlyBrokerError(String(e)));
    }
  }

  return (
    <div className="w-full max-w-xl flex flex-col gap-6">
      <section className="rounded-xl bg-slate-800/60 p-5 flex flex-col gap-3">
        <h2 className="text-sm font-medium text-slate-400 uppercase tracking-wide">
          Brick 0, the stack
        </h2>
        <div className="flex items-center gap-4">
          <button
            onClick={ping}
            className="rounded-lg bg-emerald-600 hover:bg-emerald-500 px-4 py-2 text-sm font-medium transition-colors"
          >
            Call Rust
          </button>
          {pingAnswer && (
            <span className="font-mono text-sm text-emerald-300">{pingAnswer}</span>
          )}
        </div>
      </section>

      <section className="rounded-xl bg-slate-800/60 p-5 flex flex-col gap-4">
        <h2 className="text-sm font-medium text-slate-400 uppercase tracking-wide">
          Brick 1, the broker
        </h2>
        <p className="text-sm text-slate-400">
          The app itself runs without admin rights. Privileged work is done by a
          small helper that you start here. Windows will ask you once to allow it.
        </p>

        {brokerState !== "connected" && (
          <button
            onClick={startBroker}
            disabled={brokerState === "starting"}
            className="self-start rounded-lg bg-emerald-600 hover:bg-emerald-500 disabled:opacity-50 px-4 py-2 text-sm font-medium transition-colors"
          >
            {brokerState === "starting" ? "Waiting for Windows..." : "Start the helper"}
          </button>
        )}

        {brokerState === "connected" && brokerInfo && (
          <div className="flex flex-col gap-3">
            <p className="font-mono text-sm text-emerald-300">
              {brokerInfo.msg}, elevated: {String(brokerInfo.elevated)}
            </p>
            <div className="flex items-center gap-4">
              <button
                onClick={readHklm}
                className="rounded-lg bg-slate-600 hover:bg-slate-500 px-4 py-2 text-sm font-medium transition-colors"
              >
                Read a system registry value
              </button>
              {hklmValue && (
                <span className="font-mono text-sm text-sky-300">{hklmValue}</span>
              )}
            </div>
          </div>
        )}

        {error && (
          <p className="rounded-md bg-slate-900/80 px-3 py-2 text-sm text-rose-300">
            {error}
          </p>
        )}
      </section>
    </div>
  );
}

function App() {
  const [tab, setTab] = useState("rightnow");

  return (
    <main className="min-h-screen bg-slate-900 text-slate-100 flex flex-col items-center gap-6 p-8">
      <div className="w-full max-w-4xl flex items-center justify-between">
        <h1 className="text-2xl font-semibold tracking-tight">Mganga</h1>
        <nav className="flex gap-1 rounded-lg bg-slate-800 p-1">
          {[
            ["rightnow", "Right now"],
            ["startup", "Startup"],
            ["history", "History"],
            ["plumbing", "Plumbing"],
          ].map(([id, label]) => (
            <button
              key={id}
              onClick={() => setTab(id)}
              className={`rounded-md px-4 py-1.5 text-sm font-medium transition-colors ${
                tab === id ? "bg-slate-600 text-white" : "text-slate-400 hover:text-slate-200"
              }`}
            >
              {label}
            </button>
          ))}
        </nav>
      </div>

      {tab === "rightnow" && <RightNowView />}
      {tab === "startup" && <StartupView />}
      {tab === "history" && <HistoryView />}
      {tab === "plumbing" && <PlumbingView />}
    </main>
  );
}

export default App;
