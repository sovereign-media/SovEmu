//! What each session's worker may use (sovereign-media/SovEmu#282), so a core that leaks or
//! spins ends its own session and nobody else's.
//!
//! Every worker gets its memory and open-file limits as arguments and applies them to itself
//! before it loads the core (`SovEmu.Worker/src/limits.rs`): over its memory it saves and ends
//! the session, and the kernel kills a worker before the server when the machine runs out.
//!
//! Where the server's cgroup (v2) is delegated to it, writable by its own user (a systemd unit
//! with `Delegate=yes`, rootless Podman), each worker also gets a cgroup of its own, under
//! `sovemu-workers/`, with `memory.max` (at the worker's hard limit, with no swap), `pids.max`
//! (threads) and `cpu.max`: the only way to cap threads and CPU per worker, since
//! `RLIMIT_NPROC` counts every process of the user and `RLIMIT_CPU` would end a long evening's
//! session. A stock Docker or
//! Kubernetes container mounts the cgroup tree read-only, or writable only by root, and then
//! there are no cgroups: the worker's own limits still hold.
//!
//! Taking the delegation means moving the server (and every other process in its cgroup) into
//! `sovemu-server/` first: cgroup v2 lets a cgroup hand controllers to its children only once
//! it has no processes of its own.

use std::path::{Path, PathBuf};

/// What one worker may use; 0 means no limit (for open files: what the server has).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Limits {
    /// Memory held, resident and swapped out, MiB. The worker saves and ends its session over it; a cgroup's
    /// `memory.max` sits at [`HARD_PERCENT`] of it.
    pub memory_mb: u64,
    /// CPU time, in CPUs (1.5 is a core and a half), through a cgroup only.
    pub cpus: f64,
    /// Threads and processes, through a cgroup only.
    pub threads: u32,
    /// Open files (`RLIMIT_NOFILE`).
    pub open_files: u64,
}

/// The worker's hard memory limit, in percent of [`Limits::memory_mb`]: as the worker's own
/// (`SovEmu.Worker/src/limits.rs`), so the kernel kills only a worker that could not leave.
const HARD_PERCENT: u64 = 125;

/// `cpu.max`'s period, microseconds (the kernel's default).
const CPU_PERIOD_US: u64 = 100_000;

/// Linux's "no such process", which moving a process that has exited gives.
const ESRCH: i32 = 3;
/// Linux's "busy", which handing controllers down from a cgroup with a process in it gives.
const EBUSY: i32 = 16;

/// The controllers SovEmu uses, as `cgroup.controllers` names them.
const CONTROLLERS: [&str; 3] = ["memory", "pids", "cpu"];

/// The cgroups workers get: `<server's cgroup>/sovemu-workers/<session id>`.
#[derive(Debug, Clone)]
pub struct Cgroups {
    workers: PathBuf,
    /// The controllers enabled for workers, of [`CONTROLLERS`].
    controllers: Vec<&'static str>,
}

/// Why workers get no cgroups, for the startup log.
#[derive(Debug)]
pub enum NoCgroups {
    /// Not Linux, or no cgroup v2 (`/proc/self/cgroup` has no `0::` line).
    NotV2,
    /// The cgroup has none of the controllers SovEmu uses.
    NoControllers,
    /// The cgroup cannot be written by this user: it is not delegated.
    NotDelegated(std::io::Error),
}

impl std::fmt::Display for NoCgroups {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotV2 => f.write_str("no cgroup v2"),
            Self::NoControllers => f.write_str("the cgroup has no memory, pids or cpu controller"),
            Self::NotDelegated(e) => write!(f, "the cgroup is not delegated to the server ({e})"),
        }
    }
}

impl Cgroups {
    /// Takes the server's own cgroup for its workers, if it may.
    pub fn take() -> Result<Self, NoCgroups> {
        let own = std::fs::read_to_string("/proc/self/cgroup").map_err(|_| NoCgroups::NotV2)?;
        Self::take_at(Path::new("/sys/fs/cgroup"), &own)
    }

    /// [`Self::take`] with the cgroup tree mounted at `root` and `/proc/self/cgroup` as `own`.
    fn take_at(root: &Path, own: &str) -> Result<Self, NoCgroups> {
        let path = own.lines().find_map(|l| l.strip_prefix("0::")).ok_or(NoCgroups::NotV2)?;
        let base = root.join(path.trim().trim_start_matches('/'));
        let available = std::fs::read_to_string(base.join("cgroup.controllers"))
            .map_err(|_| NoCgroups::NotV2)?;
        let controllers: Vec<&'static str> = CONTROLLERS
            .into_iter()
            .filter(|c| available.split_whitespace().any(|a| a == *c))
            .collect();
        if controllers.is_empty() {
            return Err(NoCgroups::NoControllers);
        }
        let delegate = || -> std::io::Result<PathBuf> {
            // Every process here (the server, tini) moves to a leaf, so the controllers can
            // be handed down.
            let server = base.join("sovemu-server");
            make_dir(&server)?;
            let enable: String = controllers.iter().map(|c| format!("+{c} ")).collect();
            // A process started meanwhile (a probe the server runs) lands here too, and the
            // kernel refuses the controllers while one is: move it, and try again.
            let mut attempts = 0;
            loop {
                let procs = std::fs::read_to_string(base.join("cgroup.procs"))?;
                for pid in procs.split_whitespace() {
                    match std::fs::write(server.join("cgroup.procs"), pid) {
                        // It exited since the list was read: nothing to move.
                        Err(e) if e.raw_os_error() == Some(ESRCH) => {}
                        moved => moved?,
                    }
                }
                match std::fs::write(base.join("cgroup.subtree_control"), enable.trim_end()) {
                    Err(e) if e.raw_os_error() == Some(EBUSY) && attempts < 10 => attempts += 1,
                    enabled => break enabled?,
                }
            }
            let workers = base.join("sovemu-workers");
            make_dir(&workers)?;
            std::fs::write(workers.join("cgroup.subtree_control"), enable.trim_end())?;
            Ok(workers)
        };
        let workers = delegate().map_err(NoCgroups::NotDelegated)?;
        Ok(Self { workers, controllers })
    }

    /// The controllers workers' cgroups have.
    pub fn controllers(&self) -> &[&'static str] {
        &self.controllers
    }

    /// A cgroup for session `id` with `limits`, ready for its worker.
    pub(crate) fn create(&self, id: &str, limits: &Limits) -> std::io::Result<Cgroup> {
        let dir = self.workers.join(id);
        make_dir(&dir)?;
        let cgroup = Cgroup { dir };
        let has = |c: &str| self.controllers.contains(&c);
        let set = |file: &str, value: String| std::fs::write(cgroup.dir.join(file), value);
        let written = (|| {
            if has("memory") && limits.memory_mb > 0 {
                let hard = limits.memory_mb * HARD_PERCENT / 100 * 1024 * 1024;
                set("memory.max", hard.to_string())?;
                // Over it, killed, not swapped out to slow the whole machine down (a host
                // without swap, or swap accounting, has no such file).
                if cgroup.dir.join("memory.swap.max").exists() {
                    set("memory.swap.max", "0".into())?;
                }
                // A worker over it is killed whole, never one thread of it.
                set("memory.oom.group", "1".into())?;
            }
            if has("pids") && limits.threads > 0 {
                set("pids.max", limits.threads.to_string())?;
            }
            if has("cpu") && limits.cpus > 0.0 {
                let quota = (limits.cpus * CPU_PERIOD_US as f64).round().max(1000.0) as u64;
                set("cpu.max", format!("{quota} {CPU_PERIOD_US}"))?;
            }
            Ok(())
        })();
        match written {
            Ok(()) => Ok(cgroup),
            Err(e) => {
                cgroup.remove();
                Err(e)
            }
        }
    }
}

/// `mkdir`, where one already there is fine (a server restarted in the same container).
fn make_dir(dir: &Path) -> std::io::Result<()> {
    match std::fs::create_dir(dir) {
        Err(e) if e.kind() != std::io::ErrorKind::AlreadyExists => Err(e),
        _ => Ok(()),
    }
}

/// One worker's cgroup.
#[derive(Debug)]
pub(crate) struct Cgroup {
    dir: PathBuf,
}

/// What a worker's cgroup saw by the time it exited.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Events {
    /// The kernel killed it over `memory.max`.
    pub(crate) oom_killed: bool,
    /// It tried to start a thread over `pids.max`.
    pub(crate) threads_refused: bool,
}

impl Cgroup {
    /// Moves process `pid` (the worker, just started) into it.
    pub(crate) fn add(&self, pid: u32) -> std::io::Result<()> {
        std::fs::write(self.dir.join("cgroup.procs"), pid.to_string())
    }

    /// What it saw: read once its worker has exited.
    pub(crate) fn events(&self) -> Events {
        let count = |file: &str, key: &str| {
            std::fs::read_to_string(self.dir.join(file)).ok().and_then(|text| {
                text.lines().find_map(|l| {
                    l.strip_prefix(key)?.strip_prefix(' ')?.trim().parse::<u64>().ok()
                })
            })
        };
        Events {
            oom_killed: count("memory.events", "oom_kill").is_some_and(|n| n > 0),
            threads_refused: count("pids.events", "max").is_some_and(|n| n > 0),
        }
    }

    /// Removes it, once its worker has exited (the kernel refuses while a process is in it).
    pub(crate) fn remove(self) {
        if let Err(e) = std::fs::remove_dir(&self.dir) {
            tracing::debug!(dir = %self.dir.display(), error = %e, "cannot remove a worker's cgroup");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A cgroup tree in a temporary folder: plain files where the kernel has its own.
    fn tree(name: &str, controllers: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("sovemu-cgroups-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let base = root.join("docker/abc");
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(base.join("cgroup.controllers"), controllers).unwrap();
        std::fs::write(base.join("cgroup.procs"), "1\n42\n").unwrap();
        root
    }

    #[test]
    fn takes_a_delegated_cgroup_moving_its_processes_to_a_leaf() {
        let root = tree("take", "cpuset cpu io memory pids\n");
        let cgroups = Cgroups::take_at(&root, "0::/docker/abc\n").unwrap();
        let base = root.join("docker/abc");
        assert_eq!(cgroups.controllers(), ["memory", "pids", "cpu"]);
        // A plain file keeps only the last write; the kernel's moves each pid.
        assert_eq!(std::fs::read_to_string(base.join("sovemu-server/cgroup.procs")).unwrap(), "42");
        let enabled = "+memory +pids +cpu";
        assert_eq!(std::fs::read_to_string(base.join("cgroup.subtree_control")).unwrap(), enabled);
        let workers = base.join("sovemu-workers/cgroup.subtree_control");
        assert_eq!(std::fs::read_to_string(workers).unwrap(), enabled);
        // A second server in the same container takes it again.
        assert!(Cgroups::take_at(&root, "0::/docker/abc\n").is_ok());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn no_cgroup_v2_or_no_controllers_means_none() {
        let root = tree("none", "cpuset io\n");
        let v1 = "12:memory:/docker/abc\n";
        assert!(matches!(Cgroups::take_at(&root, v1), Err(NoCgroups::NotV2)));
        let missing = "0::/elsewhere\n";
        assert!(matches!(Cgroups::take_at(&root, missing), Err(NoCgroups::NotV2)));
        let none = Cgroups::take_at(&root, "0::/docker/abc\n");
        assert!(matches!(none, Err(NoCgroups::NoControllers)));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_worker_s_cgroup_has_its_limits_and_reports_its_kills() {
        let root = tree("create", "memory pids cpu\n");
        let cgroups = Cgroups::take_at(&root, "0::/docker/abc\n").unwrap();
        let limits = Limits { memory_mb: 512, cpus: 1.5, threads: 64, open_files: 256 };
        let cgroup = cgroups.create("s1", &limits).unwrap();
        let read = |file: &str| std::fs::read_to_string(cgroup.dir.join(file)).unwrap();
        assert_eq!(read("memory.max"), (640u64 * 1024 * 1024).to_string(), "125% of 512 MiB");
        assert_eq!(read("memory.oom.group"), "1");
        assert_eq!(read("pids.max"), "64");
        assert_eq!(read("cpu.max"), "150000 100000");
        cgroup.add(7).unwrap();
        assert_eq!(read("cgroup.procs"), "7");

        assert_eq!(cgroup.events(), Events::default(), "no events file: nothing seen");
        std::fs::write(
            cgroup.dir.join("memory.events"),
            "low 0\nhigh 0\nmax 3\noom 1\noom_kill 1\n",
        )
        .unwrap();
        std::fs::write(cgroup.dir.join("pids.events"), "max 2\n").unwrap();
        assert_eq!(cgroup.events(), Events { oom_killed: true, threads_refused: true });
        std::fs::remove_dir_all(root).unwrap();
    }

    /// Against the kernel's own cgroups: a process over its cgroup's memory is killed, and its
    /// cgroup says so. It takes this process's cgroup, so it runs only by hand, in a container
    /// with a delegated one: `docker run --security-opt writable-cgroups=true` as root, then
    /// `cargo nextest run -p sovemu-sessions --run-ignored only`.
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "needs a delegated cgroup v2, and moves this process into it"]
    fn the_kernel_kills_a_worker_over_its_memory_and_says_so() {
        let cgroups = Cgroups::take().expect("a delegated cgroup");
        let limits = Limits { memory_mb: 32, threads: 16, cpus: 0.5, open_files: 0 };
        let cgroup = cgroups.create("kernel-test", &limits).unwrap();
        let swap = std::fs::read_to_string(cgroup.dir.join("memory.swap.max"));
        assert!(swap.is_err() || swap.unwrap().trim() == "0", "killed, not swapped out");
        // 200 MiB held in memory by `tail`, which keeps the last lines whole: there is no
        // newline, so one line of all of it.
        let mut child = std::process::Command::new("sh")
            .args(["-c", "sleep 0.2; head -c 200M /dev/zero | tail > /dev/null"])
            .spawn()
            .unwrap();
        cgroup.add(child.id()).unwrap();
        let status = child.wait().unwrap();
        assert!(!status.success(), "{status}");
        assert_eq!(cgroup.events(), Events { oom_killed: true, threads_refused: false });
        cgroup.remove();
        assert!(!cgroups.workers.join("kernel-test").exists(), "removed once empty");
    }

    #[test]
    fn no_limit_writes_nothing() {
        let root = tree("unlimited", "memory pids cpu\n");
        let cgroups = Cgroups::take_at(&root, "0::/docker/abc\n").unwrap();
        let cgroup = cgroups.create("s2", &Limits::default()).unwrap();
        assert!(!cgroup.dir.join("memory.max").exists());
        assert!(!cgroup.dir.join("pids.max").exists());
        assert!(!cgroup.dir.join("cpu.max").exists());
        std::fs::remove_dir_all(root).unwrap();
    }
}
