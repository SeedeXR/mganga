// Brick 5: the live process view, read-only.
//
// The frontend polls snapshot() every couple of seconds. The System lives in
// Tauri state between calls because per-process CPU is a delta between two
// observations; a fresh System every call would always read 0.
//
// Honesty rules:
// - Multi-process apps are grouped by executable name and their costs SUMMED,
//   so seven steamwebhelper.exe become one row with the real total.
// - sysinfo reports CPU as a share of one core (can exceed 100 on multicore);
//   we divide by core count so numbers line up with Task Manager.

use serde::Serialize;
use std::collections::HashMap;
use sysinfo::{ProcessesToUpdate, System};

#[derive(Serialize)]
pub struct ProcessGroup {
    /// Display name, e.g. "steamwebhelper" (without .exe).
    pub name: String,
    /// Percent of the whole machine's CPU, summed over the group.
    pub cpu: f32,
    /// Bytes of memory, summed over the group.
    pub memory: u64,
    /// How many processes share this name right now.
    pub count: u32,
    /// All PIDs in the group, for Brick 6's actions.
    pub pids: Vec<u32>,
    /// Brick 6 flags, filled in by the command layer: protected names get a
    /// lock instead of buttons; throttled/suspended reflect Mganga's own
    /// memory of what it did (EcoQoS cannot be read back from Windows).
    pub protected: bool,
    pub throttled: bool,
    pub suspended: bool,
    /// What stopping this would cost, when Mganga knows. None means no claim.
    pub verdict: Option<String>,
    pub reason: Option<String>,
}

#[derive(Serialize)]
pub struct ProcessSnapshot {
    /// Whole-machine CPU percent.
    pub cpu_total: f32,
    pub mem_total: u64,
    pub mem_used: u64,
    pub groups: Vec<ProcessGroup>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Probe, not assertion: two snapshots a second apart, print the totals
    /// and the heaviest groups so they can be eyeballed against Task Manager.
    #[test]
    fn probe_snapshot() {
        let mut sys = System::new();
        let _ = snapshot(&mut sys); // primes the CPU delta counters
        std::thread::sleep(std::time::Duration::from_millis(1200));
        let snap = snapshot(&mut sys);

        let mem_pct = snap.mem_used as f64 / snap.mem_total as f64 * 100.0;
        println!(
            "cpu_total={:.1}%  mem={:.0}% ({} / {} bytes)  groups={}",
            snap.cpu_total, mem_pct, snap.mem_used, snap.mem_total,
            snap.groups.len()
        );
        for g in snap.groups.iter().take(8) {
            println!("  {:<28} x{:<3} cpu={:>5.1}% mem={:>8} KB", g.name, g.count, g.cpu, g.memory / 1024);
        }
        assert!(snap.mem_total > 0);
        assert!(!snap.groups.is_empty());
    }
}

pub fn snapshot(sys: &mut System) -> ProcessSnapshot {
    sys.refresh_memory();
    sys.refresh_cpu_usage();
    sys.refresh_processes(ProcessesToUpdate::All, true);

    let cores = sys.cpus().len().max(1) as f32;

    let mut groups: HashMap<String, ProcessGroup> = HashMap::new();
    for (pid, process) in sys.processes() {
        let pid = pid.as_u32();
        if pid == 0 {
            continue; // the Idle pseudo-process would dwarf everything
        }
        let raw_name = process.name().to_string_lossy().to_string();
        let display = raw_name
            .strip_suffix(".exe")
            .or_else(|| raw_name.strip_suffix(".EXE"))
            .unwrap_or(&raw_name)
            .to_string();
        let key = display.to_lowercase();

        let entry = groups.entry(key).or_insert_with(|| ProcessGroup {
            name: display,
            cpu: 0.0,
            memory: 0,
            count: 0,
            pids: Vec::new(),
            protected: false,
            throttled: false,
            suspended: false,
            verdict: None,
            reason: None,
        });
        entry.cpu += process.cpu_usage() / cores;
        entry.memory += process.memory();
        entry.count += 1;
        entry.pids.push(pid);
    }

    let mut groups: Vec<ProcessGroup> = groups.into_values().collect();
    // Default order: most expensive first. The frontend can re-sort.
    groups.sort_by(|a, b| b.cpu.partial_cmp(&a.cpu).unwrap_or(std::cmp::Ordering::Equal));

    ProcessSnapshot {
        cpu_total: sys.global_cpu_usage(),
        mem_total: sys.total_memory(),
        mem_used: sys.used_memory(),
        groups,
    }
}
