//! What a session's worker may use (sovereign-media/SovEmu#282), applied to itself before it
//! loads the core, so nothing the core runs is ever without them.
//!
//! - **Open files**: `RLIMIT_NOFILE`, soft and hard, so a core cannot raise it again.
//! - **No core dumps** (`RLIMIT_CORE` 0): a crashing 3D core would write gigabytes.
//! - **Memory**: the worker watches the memory it holds, resident and swapped out (every
//!   [`CHECK_EVERY`]). Over the limit, it says so and the stream loop ends the session as a
//!   player quitting would, so the battery save and the auto state are written
//!   ([`over_memory`]). Over [`HARD_PERCENT`] of it,
//!   or still running [`GRACE`] after it went over (the core is stuck allocating inside a
//!   frame, where the loop cannot act), the worker tells the server and aborts: the saves since
//!   the last flush are lost, but nothing else is. A cgroup's `memory.max`, where the server
//!   has one, sits at the hard limit too, so the kernel does the same if the worker cannot.
//! - **First to go**: `oom_score_adj` 1000, so when the machine runs out of memory the kernel
//!   kills a worker before the server.
//!
//! Memory held, not address space: every session reserves far more than it uses (a 2D game
//! about 1 GiB of address space and 450 MiB of private writable memory for 110 MiB resident; the
//! N64 core 4.5 GiB and 1.3 GiB for 300 MiB, as measured for #282), so an `RLIMIT_AS` or
//! `RLIMIT_DATA` low enough to matter stops games that are fine.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// How often the worker reads the memory it holds.
const CHECK_EVERY: Duration = Duration::from_millis(250);

/// How far over the limit, in percent of it, the worker aborts at once instead of waiting for
/// the stream loop to save and leave. The headroom is the saves' own: an auto state is a copy of
/// the core's memory.
pub(crate) const HARD_PERCENT: u64 = 125;

/// How long the stream loop has to save and exit once over the limit before the worker aborts.
const GRACE: Duration = Duration::from_secs(10);

/// Set once the worker is over its memory limit: the stream loop ends the session.
static OVER: AtomicBool = AtomicBool::new(false);

/// The worker is over its memory limit: end the session, saving as it goes.
pub(crate) fn over_memory() -> bool {
    OVER.load(Ordering::Relaxed)
}

/// Applies the limits (0: none) and starts watching memory. `abort` is told just before the
/// worker aborts over its memory limit (to say why to the server: nothing else will).
pub(crate) fn apply(memory_mb: u64, open_files: u64, abort: impl Fn() + Send + 'static) {
    use rustix::process::{Resource, Rlimit, setrlimit};
    if open_files > 0 {
        let limit = Rlimit { current: Some(open_files), maximum: Some(open_files) };
        if let Err(e) = setrlimit(Resource::Nofile, limit) {
            tracing::warn!(open_files, error = %e, "cannot limit the open files");
        }
    }
    let none = Rlimit { current: Some(0), maximum: Some(0) };
    if let Err(e) = setrlimit(Resource::Core, none) {
        tracing::debug!(error = %e, "cannot turn core dumps off");
    }
    first_to_go();
    if memory_mb > 0 {
        watch(memory_mb * 1024 * 1024, Box::new(abort));
    }
    tracing::debug!(memory_mb, open_files, "limits applied");
}

#[cfg(target_os = "linux")]
fn first_to_go() {
    // Raising it needs no privilege; only lowering it does.
    if let Err(e) = std::fs::write("/proc/self/oom_score_adj", "1000") {
        tracing::debug!(error = %e, "cannot make the worker the first to go out of memory");
    }
}

#[cfg(not(target_os = "linux"))]
fn first_to_go() {}

/// What [`watch`] does with one reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verdict {
    Fine,
    /// Over the limit for the first time: tell the stream loop.
    Over,
    /// Over the hard limit, or over for longer than [`GRACE`]: abort.
    Abort,
}

/// Judges a reading of `resident` bytes against `limit`, `over_since` being when it first went
/// over (None while it has not).
fn judge(resident: u64, limit: u64, over_since: Option<Instant>, now: Instant) -> Verdict {
    if resident.saturating_mul(100) >= limit.saturating_mul(HARD_PERCENT) {
        return Verdict::Abort;
    }
    match over_since {
        Some(since) if now.duration_since(since) >= GRACE => Verdict::Abort,
        Some(_) => Verdict::Fine,
        None if resident >= limit => Verdict::Over,
        None => Verdict::Fine,
    }
}

fn watch(limit: u64, abort: Box<dyn Fn() + Send>) {
    let span = tracing::Span::current();
    let spawned = std::thread::Builder::new().name("limits".into()).spawn(move || {
        let _span = span.enter();
        let mut over_since = None;
        loop {
            std::thread::sleep(CHECK_EVERY);
            let Some(resident) = resident_bytes() else { return };
            let now = Instant::now();
            match judge(resident, limit, over_since, now) {
                Verdict::Fine => {}
                Verdict::Over => {
                    tracing::warn!(
                        memory_mb = resident / (1024 * 1024),
                        limit_mb = limit / (1024 * 1024),
                        "over the memory limit: saving and ending the session"
                    );
                    over_since = Some(now);
                    OVER.store(true, Ordering::Relaxed);
                }
                Verdict::Abort => {
                    tracing::error!(
                        memory_mb = resident / (1024 * 1024),
                        limit_mb = limit / (1024 * 1024),
                        "over the memory limit and the session could not end: aborting"
                    );
                    abort();
                    std::process::abort();
                }
            }
        }
    });
    if let Err(e) = spawned {
        tracing::warn!(error = %e, "cannot watch the memory limit");
    }
}

/// The memory this process holds: resident, plus what the kernel swapped out (on a host with
/// swap a leak would otherwise move to swap and never reach the limit), from `/proc/self/status`
/// (None where there is no `/proc`).
fn resident_bytes() -> Option<u64> {
    in_use(&std::fs::read_to_string("/proc/self/status").ok()?)
}

/// `VmRSS` plus `VmSwap` from a `/proc/<pid>/status`, in bytes.
fn in_use(status: &str) -> Option<u64> {
    let kib = |key: &str| {
        status.lines().find_map(|l| {
            l.strip_prefix(key)?.trim().strip_suffix("kB")?.trim().parse::<u64>().ok()
        })
    };
    Some((kib("VmRSS:")? + kib("VmSwap:").unwrap_or(0)) * 1024)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MB: u64 = 1024 * 1024;

    #[test]
    fn under_the_limit_is_fine_and_over_it_ends_the_session() {
        let now = Instant::now();
        assert_eq!(judge(99 * MB, 100 * MB, None, now), Verdict::Fine);
        assert_eq!(judge(100 * MB, 100 * MB, None, now), Verdict::Over);
        // Told once: the loop is ending the session.
        assert_eq!(judge(110 * MB, 100 * MB, Some(now), now), Verdict::Fine);
    }

    #[test]
    fn far_over_or_over_for_too_long_aborts() {
        let now = Instant::now();
        assert_eq!(judge(125 * MB, 100 * MB, None, now), Verdict::Abort);
        assert_eq!(judge(101 * MB, 100 * MB, Some(now), now + GRACE), Verdict::Abort);
    }

    #[test]
    fn counts_resident_and_swapped_memory() {
        let status = "Name:\tsovemu-worker\nVmRSS:\t  110000 kB\nRssAnon:\t 90000 kB\nVmSwap:\t    2400 kB\n";
        assert_eq!(in_use(status), Some(112_400 * 1024));
        assert_eq!(in_use("VmRSS:\t 100 kB\n"), Some(100 * 1024), "no swap line: none");
        assert_eq!(in_use("Name:\tx\n"), None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn reads_its_own_resident_memory() {
        let resident = resident_bytes().expect("/proc/self/status");
        assert!(resident > MB && resident < 64 * 1024 * MB, "{resident}");
    }
}
