# Memory recon

Recon for a future "help the user control their memory" feature. **This is data, not a
design.** It exists so a brainstorming session starts from measured ground instead of
spending its energy rediscovering what a real machine looks like.

Same rule as `windows-internals.md`: every number here was measured, not recalled. Where a
claim is about a Win32 API rather than an observation, it is marked **[verify]** — fetch
the source before implementing it.

**Measured:** 29 Aug 2026, host `rog-strix`. See `disk-triage.md` for the storage half.

---

## 1. The reference machine

```
CPU        AMD Ryzen 5 5600G (6C/12T, Zen 3)
Board      ASUS ROG STRIX B550-F GAMING WIFI II, BIOS 2604 (Feb 2022)
RAM        2 × 16 GB Corsair CMK32GX4M2D3600C18   rated 3600 MHz, running 2133
Slots      4 total, 2 populated
GPU        NVIDIA RTX 3060 (discrete; the 5600G iGPU is inactive, taking no system RAM)
Pagefile   D:\pagefile.sys 20 GB — on the SATA HDD. C: (NVMe) has none.
```

Live snapshot, 491 processes running:

```
Physical installed              31.79 GB
Physical in use                 18.04 GB
Available bytes                 13.86 GB
Committed bytes                 42.15 GB   ← 10.4 GB MORE than physical exists
Commit limit                    51.81 GB   ← RAM + pagefile
Pages Input/sec                  0.00      ← no hard faults; nothing is thrashing
Memory Compression working set   4.16 GB
```

---

## 2. Four numbers all called "memory used". They disagree, and all are correct.

This is the single most important section. Picking the wrong one produces a UI that lies
confidently — the exact failure Mganga exists to avoid.

| Number | Measured | What it actually means |
|---|---|---|
| **Working set** | 18.83 GB summed | Pages resident in RAM *right now*. Includes shared DLLs. |
| **Private bytes / commit** | 35.87 GB summed | Memory *promised* to processes. Not necessarily resident — which is why it exceeds the 31.79 GB that physically exists. |
| **Available bytes** | 13.86 GB | What a new allocation can take without forcing paging. |
| **Commit charge vs limit** | 42.15 / 51.81 GB | The ceiling that produces "out of memory" errors — a *separate* ceiling from physical exhaustion. |

### 2.1 Summing working sets is accurate. The folklore is wrong.

Received wisdom says summing per-process working sets double-counts shared pages badly.
Measured here:

```
Sum of WorkingSet64 across 491 processes   18.83 GB
Physical actually in use                   18.04 GB
over-count                                  1.04×
```

**4% off.** Brick 5's approach (group by exe, sum working set) is sound and matches what
Task Manager shows. Do not redesign it on the strength of the folklore.

### 2.2 Never show private bytes as "RAM used"

Summed private bytes came to **35.87 GB on a machine with 31.79 GB of RAM.** Presented as
"memory used" that reads as 113% and is nonsense. Private bytes is commit, not residency.

### 2.3 The trap that caught this audit

Reading `Win32_OperatingSystem.TotalVirtualMemorySize` / `FreeVirtualMemory` and treating
the difference as commit gave **86%** memory pressure. The performance counter gave
**75%**. The WMI fields are a poor proxy and produced a confidently wrong number that was
reported to the user before being caught.

**Use these instead:**

- `\Memory\Committed Bytes` and `\Memory\Commit Limit`
- `\Memory\Available Bytes`
- `\Memory\Pages Input/sec`

In Rust: `GetPerformanceInfo` returns `CommitTotal`, `CommitLimit`, `PhysicalTotal`,
`PhysicalAvailable` in one call — the same values without the counter overhead. **[verify]**

---

## 3. Memory Compression: the biggest apparent hog is not a hog

`Memory Compression` held **4.16 GB** — larger than any real application on the machine.
It will sit at the top of any "biggest memory users" list, and users will want to kill it.

It is not an app. When Windows runs short, it compresses cold pages instead of paging them
to disk; that process is where the compressed pages live. Its size is a **symptom of
pressure, not a cause of it.** Killing or trimming it makes things worse.

**This belongs on the protected list, with an explainer.** It is close to a perfect
demonstration of Mganga's whole thesis: the scary number is real, the naive verdict is
wrong, and the value is in explaining why.

---

## 4. "Free RAM" is the wrong health metric

Unused RAM is wasted RAM — Windows fills it with cache deliberately. A tool that reports
"only 13.86 GB free!" as a problem is selling fear.

**The honest signal for memory pressure is hard faults:** `\Memory\Pages Input/sec`,
measured at **0.00** here. That is a machine with no memory problem, despite 42 GB
committed against 31.79 GB of RAM.

Suggested diagnosis logic, in the plain-language style of the existing home screen:

- `Pages Input/sec` sustained above zero → the machine is genuinely paging. Say so.
- Commit charge near the commit limit → allocations will start failing. Different problem,
  different sentence.
- Neither → the machine is fine, whatever the free-MB number looks like. **Say that too.**

---

## 5. Two findings Mganga could report with zero risk

Both are pure diagnosis. No action, no privilege, nothing to break — and no other consumer
tool says either of them in plain language.

### 5.1 RAM running below its rated speed

```
PartNumber        CMK32GX4M2D3600C18   → rated 3600 MHz (encoded in the part number)
ConfiguredMHz     2133                 → 59% of rated
```

XMP (called **D.O.C.P.** on ASUS AMD boards) is disabled in BIOS. On Ryzen this also drops
the Infinity Fabric from 1800 to ~1066 MHz, so it costs latency, not just bandwidth.

Detect with `Win32_PhysicalMemory`: compare `ConfiguredClockSpeed` against the rated speed
parsed from `PartNumber`. Corsair encodes it as `...D3600C18` → 3600 MHz, CL18. Other
vendors encode differently, so this needs a small per-vendor parser and should stay silent
when it cannot parse rather than guess.

Mganga cannot fix this — it is a BIOS setting. **It can name it**, which is worth a lot to
someone who has no idea their memory is running at 59% of what they paid for.

### 5.2 Free DIMM slots

```
Win32_PhysicalMemoryArray.MemoryDevices   4
Populated                                 2
```

"You have two empty slots; this machine can take more RAM" is useful, honest, and
impossible to get wrong.

### 5.3 Pagefile on the slow disk

Measured: the pagefile sits on the **SATA HDD**; the NVMe SSD has none. Worth reporting as
a "your call" advisory with the cost stated — though note the counters here showed the
pagefile barely used (peak 8.73%), so Mganga should report the *placement*, not claim it is
currently hurting. Evidence first.

---

## 6. The gentle ladder does not transfer to memory as-is

**This is the design trap.** The existing ladder — Efficiency mode → Pause → Stop — is a
**CPU** ladder. Applied to memory it will mislead users:

| Action | Frees memory? |
|---|---|
| Efficiency mode (EcoQoS) | **No.** It throttles CPU scheduling and frequency. Working set is untouched. |
| Pause (`NtSuspendProcess`) | **Not directly.** A suspended process keeps its working set; Windows only trims it later under pressure. |
| Stop | **Yes** — and it is the violent option. |

So on the current ladder, the only rung that returns memory is the one Mganga is designed
to offer last. A memory feature built on it would either not work or push users toward Stop.

**The missing gentle rung is `EmptyWorkingSet`** (psapi). It trims a process's resident
pages, handing the RAM back immediately; the pages fault back in when the app is next used.
Non-destructive, reversible by nature, no data loss. **[verify]** the exact API and whether
`SetProcessWorkingSetSizeEx` with `(SIZE_T)-1` is the better call.

Honest framing is mandatory here, because this is precisely the API that "RAM booster"
scamware abuses:

- It helps when an app is **idle** and you need RAM **now**.
- It costs that app a stall when you next touch it, as pages fault back.
- It is **not** a permanent win, and running it on a schedule is what the fake optimisers do.

Which makes it a good fit for Mganga specifically: a real mechanism, a real cost, stated
plainly, offered once rather than sold as a daily ritual.

---

## 7. What the process list actually looks like

Top holders on a normal working day, grouped by executable (working set):

```
4.48 GB   Memory Compression   1 proc    ← protected, explain (§3)
4.30 GB   brave               31 procs
3.64 GB   WizTree64            1 proc    ← a disk scanner holding the MFT in RAM
1.33 GB   claude               4 procs
1.11 GB   node                16 procs
0.91 GB   svchost             95 procs   ← see below
0.60 GB   Unity                2 procs
0.44 GB   msedgewebview2      19 procs
0.42 GB   MsMpEng              1 proc    ← Defender, protected
```

**Grouping by exe is right for browsers, wrong for `svchost`.** 95 svchost processes host
completely unrelated services; summing them into one row invents a 0.91 GB entity the user
cannot act on. Either group svchost by its hosted service name, or exclude it from the
"biggest users" list and handle it under services.

`msedgewebview2` at 19 processes is worth noting too — it is embedded WebView across
several apps, not one program.

---

## 8. Suggested shape (for the brainstorm to argue with, not to implement)

Nothing here is decided. Listed so the discussion starts from options rather than a blank page.

1. **Diagnosis only, first.** Hard-fault rate, commit headroom, and the two zero-risk
   hardware findings from §5. No actions at all. This alone answers "why is my machine slow"
   better than Task Manager does.
2. **Then one gentle action:** `EmptyWorkingSet` on a chosen idle process, with the cost
   stated and no scheduling.
3. **Protected list additions:** Memory Compression, Defender, the existing core-Windows set.
4. **Open question:** does memory deserve its own tab, or is it a column and a sentence on
   the Running-now tab that already exists? The existing tab may already be the right home.

---

## References

- `GetPerformanceInfo` — CommitTotal, CommitLimit, PhysicalTotal, PhysicalAvailable **[verify]**
- `GlobalMemoryStatusEx` — `ullTotalPageFile` / `ullAvailPageFile` are commit limit / available commit, **not** pagefile size **[verify]**
- `K32GetProcessMemoryInfo` → `PROCESS_MEMORY_COUNTERS_EX` — `WorkingSetSize`, `PrivateUsage` **[verify]**
- `EmptyWorkingSet` (psapi) — the gentle memory rung **[verify]**
- Counters: `\Memory\Committed Bytes`, `\Memory\Commit Limit`, `\Memory\Available Bytes`, `\Memory\Pages Input/sec`
- `Win32_PhysicalMemory` — `ConfiguredClockSpeed`, `Speed`, `PartNumber`, `Capacity`, `DeviceLocator`
- `Win32_PhysicalMemoryArray` — `MemoryDevices` (total slots)
- `Win32_PageFileUsage` / `Win32_PageFileSetting` — placement and size
