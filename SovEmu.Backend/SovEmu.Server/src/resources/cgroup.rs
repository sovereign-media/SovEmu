//! The container's own CPU and memory, as its cgroup counts them (as Sovereign's
//! `ContainerResourceReader`).
//!
//! Read from the cgroup rather than this process: the game sessions are `sovemu-worker` child
//! processes, and they do nearly all the work, so this process alone would badly under-report.
//! cgroup v2 first, then v1; with neither (a bare-metal or macOS run) there is no reading, and
//! nothing is guessed. Every file is read through one function, so each layout is tested from
//! fixtures.

/// One reading of the counters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Counters {
    /// CPU time used by every process in the cgroup, in microseconds. A counter: a percentage
    /// is the change between two readings.
    pub(crate) cpu_usage_micros: u64,
    /// The cores the container may use: its quota over its period, or the host's count when no
    /// quota is set.
    pub(crate) allowed_cores: f64,
    /// Whether a CPU quota is set.
    pub(crate) cpu_limited: bool,
    /// Memory in use, less reclaimable page cache (`inactive_file`), as `docker stats` counts it.
    pub(crate) memory_used: u64,
    /// The container's memory limit, or the host's total when none is set (or it is above it).
    pub(crate) memory_limit: u64,
    /// Whether a memory limit below the host's total is set.
    pub(crate) memory_limited: bool,
}

const ROOT: &str = "/sys/fs/cgroup";

/// A v1 `memory.limit_in_bytes` with no limit reads as a page-aligned `i64::MAX`.
const V1_NO_LIMIT: u64 = (i64::MAX / 2) as u64;

/// Reads the counters through `read` (a file's text, or none if it is missing or unreadable).
/// `cpus`: the host's (or the affinity mask's) processor count, used when no quota is set.
pub(crate) fn read(read: &dyn Fn(&str) -> Option<String>, cpus: usize) -> Option<Counters> {
    let host_memory = host_memory(read);
    if read(&format!("{ROOT}/cgroup.controllers")).is_some() {
        read_v2(read, &v2_dir(read), cpus, host_memory)
    } else {
        read_v1(read, cpus, host_memory)
    }
}

/// The counters from the real files.
pub(crate) fn read_system() -> Option<Counters> {
    let cpus = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
    read(&|path| std::fs::read_to_string(path).ok(), cpus)
}

/// This process's v2 directory: the root with a private cgroup namespace (Docker's default),
/// else the `0::` path in `/proc/self/cgroup` when it is visible.
fn v2_dir(read: &dyn Fn(&str) -> Option<String>) -> String {
    let Some(own) = read("/proc/self/cgroup") else { return ROOT.into() };
    for line in own.lines() {
        let Some(path) = line.strip_prefix("0::") else { continue };
        let path = path.trim();
        if path.is_empty() || path == "/" {
            return ROOT.into();
        }
        let candidate = format!("{ROOT}{path}");
        if read(&format!("{candidate}/cpu.stat")).is_some() {
            return candidate;
        }
    }
    ROOT.into()
}

fn read_v2(
    read: &dyn Fn(&str) -> Option<String>,
    dir: &str,
    cpus: usize,
    host_memory: Option<u64>,
) -> Option<Counters> {
    let usage = keyed(read(&format!("{dir}/cpu.stat")).as_deref(), "usage_usec")?;
    let current = number(read(&format!("{dir}/memory.current")).as_deref())?;
    let (allowed_cores, cpu_limited) = cpu_max(read(&format!("{dir}/cpu.max")).as_deref(), cpus);
    let inactive = keyed(read(&format!("{dir}/memory.stat")).as_deref(), "inactive_file");
    let limit = read(&format!("{dir}/memory.max")).and_then(|t| match t.trim() {
        "max" => None,
        t => number(Some(t)),
    });
    let (memory_limit, memory_limited) = memory_limit(limit, host_memory);
    Some(Counters {
        cpu_usage_micros: usage,
        allowed_cores,
        cpu_limited,
        memory_used: current.saturating_sub(inactive.unwrap_or(0)),
        memory_limit,
        memory_limited,
    })
}

fn read_v1(
    read: &dyn Fn(&str) -> Option<String>,
    cpus: usize,
    host_memory: Option<u64>,
) -> Option<Counters> {
    // Mounted combined or apart, by distribution: the combined directory first.
    let first = |dirs: &[&str], file: &str| {
        dirs.iter().map(|d| format!("{ROOT}/{d}")).find(|d| read(&format!("{d}/{file}")).is_some())
    };
    let cpuacct = first(&["cpu,cpuacct", "cpuacct"], "cpuacct.usage")?;
    let usage_nanos = number(read(&format!("{cpuacct}/cpuacct.usage")).as_deref())?;
    let current = number(read(&format!("{ROOT}/memory/memory.usage_in_bytes")).as_deref())?;
    let (mut allowed_cores, mut cpu_limited) = (cpus as f64, false);
    if let Some(cpu) = first(&["cpu,cpuacct", "cpu"], "cpu.cfs_quota_us") {
        // An unlimited quota is -1, which does not parse as a u64.
        let quota = number(read(&format!("{cpu}/cpu.cfs_quota_us")).as_deref());
        let period = number(read(&format!("{cpu}/cpu.cfs_period_us")).as_deref());
        if let (Some(q), Some(p)) = (quota, period)
            && q > 0
            && p > 0
        {
            (allowed_cores, cpu_limited) = (q as f64 / p as f64, true);
        }
    }
    let stat = read(&format!("{ROOT}/memory/memory.stat"));
    let inactive = keyed(stat.as_deref(), "total_inactive_file")
        .or_else(|| keyed(stat.as_deref(), "inactive_file"));
    let limit = number(read(&format!("{ROOT}/memory/memory.limit_in_bytes")).as_deref())
        .filter(|&l| l < V1_NO_LIMIT);
    let (memory_limit, memory_limited) = memory_limit(limit, host_memory);
    Some(Counters {
        cpu_usage_micros: usage_nanos / 1000,
        allowed_cores,
        cpu_limited,
        memory_used: current.saturating_sub(inactive.unwrap_or(0)),
        memory_limit,
        memory_limited,
    })
}

/// v2 `cpu.max`: `<quota> <period>`, or `max <period>` for none.
fn cpu_max(text: Option<&str>, cpus: usize) -> (f64, bool) {
    let mut parts = text.unwrap_or_default().split_whitespace();
    match (
        parts.next().and_then(|q| q.parse::<u64>().ok()),
        parts.next().and_then(|p| p.parse::<u64>().ok()),
    ) {
        (Some(q), Some(p)) if q > 0 && p > 0 => (q as f64 / p as f64, true),
        _ => (cpus as f64, false),
    }
}

/// The limit to measure against: the cgroup's when set and below the host's total, else the
/// host's (a limit above what the host has limits nothing).
fn memory_limit(cgroup: Option<u64>, host: Option<u64>) -> (u64, bool) {
    match (cgroup, host) {
        (Some(l), Some(h)) if l > 0 && l < h => (l, true),
        (Some(l), None) if l > 0 => (l, true),
        _ => (host.unwrap_or(0), false),
    }
}

/// `MemTotal` from `/proc/meminfo`, in bytes.
pub(crate) fn host_memory(read: &dyn Fn(&str) -> Option<String>) -> Option<u64> {
    let meminfo = read("/proc/meminfo")?;
    let line = meminfo.lines().find_map(|l| l.strip_prefix("MemTotal:"))?;
    let kb: u64 = line.split_whitespace().next()?.parse().ok()?;
    Some(kb * 1024)
}

/// `key value` on its own line of a flat-keyed cgroup file.
fn keyed(text: Option<&str>, key: &str) -> Option<u64> {
    text?.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        (parts.next() == Some(key)).then(|| parts.next()?.parse().ok()).flatten()
    })
}

fn number(text: Option<&str>) -> Option<u64> {
    text?.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    const GIB: u64 = 1024 * 1024 * 1024;

    fn files(entries: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> =
            entries.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect();
        move |path| map.get(path).cloned()
    }

    const MEMINFO: (&str, &str) = ("/proc/meminfo", "MemTotal:       16384000 kB\nMemFree: 1 kB\n");

    #[test]
    fn v2_with_limits() {
        let read = files(&[
            MEMINFO,
            ("/sys/fs/cgroup/cgroup.controllers", "cpu memory"),
            ("/proc/self/cgroup", "0::/\n"),
            ("/sys/fs/cgroup/cpu.stat", "usage_usec 5000000\nuser_usec 4000000\n"),
            ("/sys/fs/cgroup/cpu.max", "200000 100000\n"),
            ("/sys/fs/cgroup/memory.current", "3221225472\n"),
            ("/sys/fs/cgroup/memory.stat", "anon 1\ninactive_file 1073741824\n"),
            ("/sys/fs/cgroup/memory.max", "4294967296\n"),
        ]);
        let c = read_counters(&read, 8);
        assert_eq!(c.cpu_usage_micros, 5_000_000);
        assert_eq!((c.allowed_cores, c.cpu_limited), (2.0, true));
        assert_eq!(c.memory_used, 2 * GIB, "page cache it can drop is not counted");
        assert_eq!((c.memory_limit, c.memory_limited), (4 * GIB, true));
    }

    fn read_counters(read: &dyn Fn(&str) -> Option<String>, cpus: usize) -> Counters {
        super::read(read, cpus).expect("a reading")
    }

    #[test]
    fn v2_without_limits_measures_against_the_host() {
        let read = files(&[
            MEMINFO,
            ("/sys/fs/cgroup/cgroup.controllers", "cpu memory"),
            ("/sys/fs/cgroup/cpu.stat", "usage_usec 10\n"),
            ("/sys/fs/cgroup/cpu.max", "max 100000\n"),
            ("/sys/fs/cgroup/memory.current", "1000\n"),
            ("/sys/fs/cgroup/memory.max", "max\n"),
        ]);
        let c = read_counters(&read, 8);
        assert_eq!((c.allowed_cores, c.cpu_limited), (8.0, false));
        assert_eq!((c.memory_limit, c.memory_limited), (16_384_000 * 1024, false));
    }

    #[test]
    fn v2_on_the_host_namespace_finds_its_own_directory() {
        let read = files(&[
            ("/sys/fs/cgroup/cgroup.controllers", "cpu memory"),
            ("/proc/self/cgroup", "0::/system.slice/sovemu.service\n"),
            ("/sys/fs/cgroup/system.slice/sovemu.service/cpu.stat", "usage_usec 42\n"),
            ("/sys/fs/cgroup/system.slice/sovemu.service/memory.current", "7\n"),
            ("/sys/fs/cgroup/cpu.stat", "usage_usec 999999\n"),
            ("/sys/fs/cgroup/memory.current", "999999\n"),
        ]);
        let c = read_counters(&read, 4);
        assert_eq!((c.cpu_usage_micros, c.memory_used), (42, 7));
    }

    #[test]
    fn v1_combined_and_unlimited() {
        let read = files(&[
            MEMINFO,
            ("/sys/fs/cgroup/cpu,cpuacct/cpuacct.usage", "3000000000\n"),
            ("/sys/fs/cgroup/cpu,cpuacct/cpu.cfs_quota_us", "-1\n"),
            ("/sys/fs/cgroup/cpu,cpuacct/cpu.cfs_period_us", "100000\n"),
            ("/sys/fs/cgroup/memory/memory.usage_in_bytes", "2000\n"),
            ("/sys/fs/cgroup/memory/memory.stat", "total_inactive_file 500\n"),
            ("/sys/fs/cgroup/memory/memory.limit_in_bytes", "9223372036854771712\n"),
        ]);
        let c = read_counters(&read, 6);
        assert_eq!(c.cpu_usage_micros, 3_000_000, "nanoseconds to microseconds");
        assert_eq!((c.allowed_cores, c.cpu_limited), (6.0, false));
        assert_eq!(c.memory_used, 1500);
        assert!(!c.memory_limited);
    }

    #[test]
    fn v1_separate_with_a_quota() {
        let read = files(&[
            MEMINFO,
            ("/sys/fs/cgroup/cpuacct/cpuacct.usage", "1000\n"),
            ("/sys/fs/cgroup/cpu/cpu.cfs_quota_us", "150000\n"),
            ("/sys/fs/cgroup/cpu/cpu.cfs_period_us", "100000\n"),
            ("/sys/fs/cgroup/memory/memory.usage_in_bytes", "10\n"),
            ("/sys/fs/cgroup/memory/memory.limit_in_bytes", "1073741824\n"),
        ]);
        let c = read_counters(&read, 6);
        assert_eq!((c.allowed_cores, c.cpu_limited), (1.5, true));
        assert_eq!((c.memory_limit, c.memory_limited), (GIB, true));
    }

    #[test]
    fn a_limit_above_the_host_limits_nothing() {
        assert_eq!(memory_limit(Some(64 * GIB), Some(16 * GIB)), (16 * GIB, false));
    }

    #[test]
    fn no_cgroup_is_no_reading() {
        assert_eq!(read(&files(&[MEMINFO]), 4), None);
    }
}
