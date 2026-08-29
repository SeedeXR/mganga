# Disk triage reference

Source of truth for any future disk-space work in Mganga. Same rule as
`windows-internals.md`: every claim here was **measured on a real machine**, not recalled.
Where a number appears, it came from a command in §7. Do not invent disk behavior.

This is a reference, not a brick. It does not authorise building anything — see
`build-plan.md` for the build order and the review gates.

**Field audit:** 28-29 Aug 2026, host `rog-strix`, C: 931 GB NVMe, 5.7 GB free at start.
Full findings with charts: the "Mganga Disk Triage" artifact.

---

## 1. The first law, applied to disk

*A healer does not poison the patient.* On disk this cashes out differently than on
processes, because deleting a file is not reversible the way flipping a StartupApproved
byte is. There is no undo log that can bring bytes back.

So the disk equivalent of "reversible by default" is **reclaim only what regenerates
itself**:

> Mganga may reclaim data that the owning tool will rebuild on demand. It may not
> reclaim data whose only copy is the one on disk.

That single sentence decides every case below. It also means Mganga's disk feature is
**not** a "delete old files" cleaner. Age is not a verdict. Regenerability is.

---

## 2. The three verdict classes

Reuse the existing UI vocabulary — the colours already mean these things:

| Class | Token | Meaning | Action |
|---|---|---|---|
| **Regenerable** | `glitch-green` | Derived or re-downloadable. Costs time and bandwidth to rebuild, never information. | Offer directly |
| **Your call** | `caution` | Reclaimable, but the user loses a capability or something not guaranteed to come back. | Show the cost, require confirm |
| **Protected** | `glitch-red` | Deleting breaks a tool, corrupts projects, or destroys the only copy. | Show, locked, one-line why |

Same shape as the autostart verdicts, so no new UI grammar is needed.

---

## 3. Classifying a cache: the decision procedure

Run these in order. **Stop at the first one that fires.** Do not skip to the size sort —
the whole failure mode of every other cleaner is sorting by size and asking questions later.

### 3.1 Is it on the protected list? → Protected

Hard-coded, enforced in the broker, not only in the GUI (same rule as processes). See §4.

### 3.2 Is it hardlinked into live data? → Protected

**This is the test nobody else does, and it is the one that prevents real damage.**

Some caches are content-addressed stores whose files are *hardlinked* into projects.
The cache is not a copy of the installed data — it **is** the installed data, one inode
with two names. Deleting it corrupts every project at once.

```
Confirmed on this machine:
  pnpm store v10   → hardlinks into every node_modules on C:   PROTECTED
  uv cache         → hardlinks into venvs                       PROTECTED from deletion
  npm-cache        → extracts copies, nothing links back        regenerable
  Gradle caches    → copies                                     regenerable
  Unity cache      → copies into each project's Library/        regenerable
```

**How to test, in Rust:** open the file with `CreateFileW` (`FILE_READ_ATTRIBUTES` is
enough, no read access needed), call `GetFileInformationByHandle`, read
`nNumberOfLinks`. Greater than 1 means another name points at these bytes.

Sample several files, not one — a store can hold both linked and unlinked entries.

> A second consequence: **never offer to relocate a hardlink cache to another volume.**
> Hardlinks cannot cross volumes, so the tool silently falls back to full copies and the
> source drive gets *fuller*. The exact opposite of what the user asked for.
>
> Safe operation for these is the tool's own pruner, which knows what is still reachable:
> `pnpm store prune`, `uv cache prune`. Mganga should invoke the pruner, never `rm`.

### 3.3 Is every version still referenced? → cross-reference before condemning

Package caches accumulate versions. **Version age does not make an entry dead.** On this
machine, `com.unity.collab-proxy` had 21 cached versions and `com.unity.burst` had 15 —
and older projects still pinned versions eight releases behind current.

Measured by parsing 69 Unity projects' `Packages/packages-lock.json` (498 distinct
`pkg@version` pins) and intersecting with the cache listing:

```
Unity\cache\packages     412 versions   24.07 GB
  referenced by a project  212 versions   14.40 GB   ← keep
  referenced by nothing    200 versions    9.67 GB   ← reclaimable
```

Nearly **half** the "old" versions were live. A version sort would have broken projects.

The generalisation: **a cache entry is dead only when no manifest on disk declares it.**
Find the manifests, parse them, intersect. If Mganga cannot find the manifests for an
ecosystem, it must not offer to clean that ecosystem's cache by version.

Manifest locations worth knowing:

| Ecosystem | Declares versions in |
|---|---|
| Unity | `<project>\Packages\packages-lock.json` (resolved) and `manifest.json` (requested) |
| Node | `package-lock.json`, `pnpm-lock.yaml` |
| Rust | `Cargo.lock` |
| Python (uv) | `uv.lock`, plus the venvs themselves |

### 3.4 Is the owning app still installed? → check install evidence, not the folder name

Per-version app caches outlive their apps. But the version number in the folder name is
**not** the signal.

```
Installed:  Rider 2025.3.3 · RustRover 2026.1.2 · WebStorm 2025.3.3
            PhpStorm 2025.3.3 · PyCharm 2023.2          ← oldest, and still in use
Orphaned:   Rider2024.2 · RustRover2024.1 · 2024.2 · 2025.1     6.70 GB
```

`PyCharm2023.2` is older than two of the orphans and is installed and working. Sorting by
version would have deleted a live IDE's configuration.

**The signal is install evidence:** an install directory *plus* a Start Menu entry. Check
both — this machine had a `WebStorm 2023.1.4` directory with no shortcut, which is
ambiguous, so it was excluded pending a human check. **Ambiguous means excluded, not
guessed.**

### 3.5 Otherwise → Regenerable, and say what rebuilding costs

Name the cost in the UI: "Unity re-downloads these on next project open." The user is
trading bandwidth for space and deserves to know which.

---

## 4. The protected list, disk edition

Hard-coded, enforced in the broker. Every one of these is large, cache-shaped, and sitting
in a folder a naive cleaner would sweep — which is exactly why the list exists.

| Path | Why | Measured |
|---|---|---|
| `AppData\Local\pnpm\store` | Hardlinked into every `node_modules`. Deleting corrupts all projects. | 9.29 GB |
| `C:\Windows\Installer` | MSI rollback/repair data. Deleting breaks uninstall, repair and update — surfacing months later with no obvious cause. | 9.99 GB |
| `ProgramData\Package Cache` | Visual Studio / VC++ redist repair payloads. | 2.18 GB |
| `%LOCALAPPDATA%\Packages\<app>\LocalCache\...\vm_bundles` | App-managed VM images. Recoverable only by reinstalling. | 9.02 GB |
| `%USERPROFILE%\.claude` | Session transcripts, memory, skills. **Irreplaceable — no re-download exists.** | 1.51 GB |
| `%LOCALAPPDATA%\Temp\claude` | Live agent scratchpads. Sits *inside* the temp folder a cleaner sweeps. | 2.23 GB |
| `hiberfil.sys`, `pagefile.sys`, `swapfile.sys` | Never delete as files. These are toggled through `powercfg` / System settings, or not at all. | — |

Two general rules behind the list:

- **Anything whose only copy is local is protected**, regardless of which folder it sits in.
  User profile dot-directories (`.claude`, `.ssh`, `.config`, `.aws`) are the common case.
- **Live scratch is protected even inside a temp sweep.** Exclude by name *before* walking,
  not by filtering results afterward — a filter that runs late can still race a deletion.

---

## 5. Age, and what it is actually good for

Age is a usable signal for **one** case only: unowned scratch in `%TEMP%`, where the
owning process is gone and no manifest exists to consult.

Measured: 1,816 directories and 12,386 loose files with a last-write older than 30 days,
totalling 36.9 GB. Deleting them freed exactly what was expected and broke nothing.

Two warnings that came out of that pass:

**Directory mtime does not reflect its contents.** A folder's `LastWriteTime` only changes
when direct children are added or removed. Deep writes leave it stale. For anything larger
than scratch, check the newest file inside, not the folder.

**Storage Sense being enabled proves nothing.** It was on (`StoragePolicy\01 = 1`) and had
still left 36.9 GB of month-old data in place, because it sweeps a fixed allowlist of known
temp locations, not arbitrary app-created folders. **Never infer disk state from a
configuration flag. Measure it.**

---

## 6. Caches clean themselves — check before offering

The largest single item found in the audit was `.gradle\caches\8.13\transforms` at
**33.16 GB** (2,945 artifact-transform entries). Twenty-four hours later, without
intervention, it was **2.28 GB** — Gradle's own GC evicted the unused entries when a build
ran.

The lesson for a healer: **some patients recover on their own.** Before presenting a cache
as a problem, check whether the tool has a documented eviction policy and whether it is
running. A tool that shouts about 33 GB the user would have got back anyway is the
fear-selling Mganga exists to be the opposite of.

Where a pruner exists, prefer invoking it over deleting: it knows reachability, Mganga does
not.

---

## 7. Measuring, correctly

### Walk the tree the way Windows wants to

Recursive `stat` through a POSIX layer timed out past **ten minutes** on this tree
(7.6 M files). This returned the same totals in seconds:

```
robocopy "<dir>" NULL /L /S /NJH /BYTES /NC /NDL /NFL /XJ /R:0 /W:0
```

`/L` list-only (never copies), `/XJ` excludes junctions so nothing is counted twice.
Parse the `Bytes :` line of the summary — note `/NJS` would suppress that summary, so do
not add it.

In Rust, the equivalent is walking with `FindFirstFileExW` +
`FindExInfoBasic` + `FIND_FIRST_EX_LARGE_FETCH` and skipping reparse points
(`FILE_ATTRIBUTE_REPARSE_POINT`). For a whole-volume picture, reading the **NTFS MFT**
directly is what WizTree does and is an order faster again — but it needs elevation, so it
belongs in the broker if it is ever wanted.

### Free space comes from the volume, never from a sum

Summing the sizes of everything you intended to delete over-reports every time: locked
files get skipped, hardlinked bytes survive their last visible name, sparse files misreport.

Read it before and after: `GetDiskFreeSpaceExW`. **The filesystem is the only honest
reporter.** The audit's own script did this and the difference was material.

### Check media type before ever suggesting a move

```
Get-PhysicalDisk | Select-Object FriendlyName, MediaType, BusType
```

On this machine the volume with the most free space (D:, 193.8 GB) was a **7200rpm SATA
HDD**, while the full volume was NVMe. Relocating Gradle's cache there — tens of thousands
of small random reads — would have bought 34 GB and paid for it on every build, forever.

**A space win measured only in gigabytes is not a win.** If Mganga ever offers relocation,
it must compare `MediaType`/`BusType` and refuse a downgrade, or state it loudly.

---

## 8. Skipping is correct behavior

One directory out of 1,816 survived the sweep because Adobe held it open. That is not a
failure to work around. A locked file is the filesystem saying a process still needs it.

**Report the skip, name the holder, move on.** Never force, never schedule a delete-on-
reboot to get around a lock. Same instinct as offering Efficiency mode before Stop.

---

## 9. Open questions (owner decisions, not yet made)

- **Does disk belong in Mganga at all?** The app answers "why is my machine slow" and
  "what starts with Windows". Disk is a third question. It may deserve its own tab, or it
  may be scope creep. Not decided.
- **Is deletion allowed at all**, given "nothing is deleted, ever" in the README? A
  defensible reading: Mganga *invokes the owning tool's pruner* and never deletes directly,
  keeping the promise intact. Cheaper and safer than a delete engine, and it inherits each
  tool's own reachability knowledge.
- **Staging vs deleting.** If direct deletion is ever wanted, moving to a staging folder
  first and emptying after N days would give a real undo — at the cost of not freeing the
  space until later, which is the whole point of the feature. Probably not worth it.

---

## References

- Hardlink count: `GetFileInformationByHandle` → `BY_HANDLE_FILE_INFORMATION.nNumberOfLinks`
- Free space: `GetDiskFreeSpaceExW`
- Fast enumeration: `FindFirstFileExW` with `FindExInfoBasic`, `FIND_FIRST_EX_LARGE_FETCH`
- Reparse points: `FILE_ATTRIBUTE_REPARSE_POINT` — skip, or junctions double-count
- Physical media: `MSFT_PhysicalDisk` (Storage WMI namespace), fields `MediaType`, `BusType`
- Hibernation: `powercfg /a`, `/h off`, `/h /type reduced` — never touch `hiberfil.sys` as a file
- Storage Sense state: `HKCU\Software\Microsoft\Windows\CurrentVersion\StorageSense\Parameters\StoragePolicy`
