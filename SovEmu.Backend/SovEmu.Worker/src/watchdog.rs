//! A watchdog for the stream loop.
//!
//! The loop runs everything on one thread, cores included, and a core can stop coming back from
//! `retro_run` (a deadlock between its own threads, a driver waiting forever). The server then
//! kills the worker after its shutdown timeout, and nothing says why. So the loop beats once a
//! turn, saying what it is doing, and a thread of this module's watches: when the beats stop
//! for [`STALL`], it logs what the loop was doing and, on Linux, what every thread of the
//! process is doing (its state and kernel wait channel from `/proc`, and how much CPU it used
//! over a second: a deadlock shows every thread blocked, a spin shows one using a whole core).
//! It logs again if the loop is still stuck at [`STILL_STUCK`], and once when it comes back.
//!
//! One stream loop per process, so the state is process-wide.

use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// What the loop was doing at its last beat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum Phase {
    /// Waiting for and handling network traffic.
    Network = 0,
    /// Inside the core (a frame, run-ahead's frames, a save state).
    Emulating = 1,
    /// Encoding and sending the frame.
    Encoding = 2,
}

impl Phase {
    fn name(v: u8) -> &'static str {
        match v {
            1 => "emulating (inside the core)",
            2 => "encoding",
            _ => "network",
        }
    }
}

/// No beat for this long: the loop is stuck.
const STALL: Duration = Duration::from_secs(5);
/// Report a second time if it is still stuck this long.
const STILL_STUCK: Duration = Duration::from_secs(20);

static STARTED: AtomicBool = AtomicBool::new(false);
/// Milliseconds since [`epoch`] at the last beat.
static BEAT_MS: AtomicU64 = AtomicU64::new(0);
static PHASE: AtomicU8 = AtomicU8::new(0);

fn epoch() -> Instant {
    static EPOCH: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    *EPOCH.get_or_init(Instant::now)
}

fn now_ms() -> u64 {
    epoch().elapsed().as_millis() as u64
}

/// The loop came round, and is now doing `phase`.
pub(crate) fn beat(phase: Phase) {
    BEAT_MS.store(now_ms(), Ordering::Relaxed);
    PHASE.store(phase as u8, Ordering::Relaxed);
}

/// Starts watching (once per process). The watcher keeps the caller's span, so its lines carry
/// the session.
pub(crate) fn start() {
    if STARTED.swap(true, Ordering::Relaxed) {
        return;
    }
    beat(Phase::Network);
    let span = tracing::Span::current();
    let spawned = std::thread::Builder::new().name("watchdog".into()).spawn(move || {
        let _span = span.enter();
        watch();
    });
    if let Err(e) = spawned {
        tracing::debug!(error = %e, "no watchdog thread");
    }
}

fn watch() {
    // The beat a report was made for, and how many reports it has had.
    let mut reported: Option<(u64, u32)> = None;
    loop {
        std::thread::sleep(Duration::from_secs(1));
        let beat = BEAT_MS.load(Ordering::Relaxed);
        let stuck = Duration::from_millis(now_ms().saturating_sub(beat));
        match reported {
            Some((at, _)) if at != beat => {
                tracing::warn!(
                    stuck_s = Duration::from_millis(beat.saturating_sub(at)).as_secs_f64(),
                    "stream loop came back"
                );
                reported = None;
            }
            Some((at, 1)) if stuck >= STILL_STUCK => {
                report(stuck);
                reported = Some((at, 2));
            }
            None if stuck >= STALL => {
                report(stuck);
                reported = Some((beat, 1));
            }
            _ => {}
        }
    }
}

fn report(stuck: Duration) {
    let phase = Phase::name(PHASE.load(Ordering::Relaxed));
    let threads = threads().unwrap_or_else(|| "(no /proc on this system)".into());
    tracing::warn!(
        stuck_s = stuck.as_secs(),
        phase,
        threads = %threads,
        "stream loop stuck: no turn for a while; the threads, busiest first"
    );
}

/// One thread as `/proc` shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ThreadState {
    tid: u32,
    name: String,
    /// `R` running, `S` sleeping, `D` waiting on I/O, …
    state: char,
    /// CPU time used, in clock ticks (user + system).
    cpu_ticks: u64,
}

/// Parses `/proc/<pid>/task/<tid>/stat`: `tid (name) state … utime stime …`. The name may hold
/// spaces and parentheses, so the fields are counted from its last `)`.
fn parse_stat(stat: &str) -> Option<ThreadState> {
    let open = stat.find('(')?;
    let close = stat.rfind(')')?;
    let tid = stat[..open].trim().parse().ok()?;
    let name = stat[open + 1..close].to_owned();
    let rest: Vec<&str> = stat[close + 1..].split_whitespace().collect();
    // After the name: state is field 3 (rest[0]); utime and stime are fields 14 and 15.
    let state = rest.first()?.chars().next()?;
    let utime: u64 = rest.get(11)?.parse().ok()?;
    let stime: u64 = rest.get(12)?.parse().ok()?;
    Some(ThreadState { tid, name, state, cpu_ticks: utime + stime })
}

fn sample() -> Option<Vec<ThreadState>> {
    let tasks = std::fs::read_dir("/proc/self/task").ok()?;
    let mut threads: Vec<ThreadState> = tasks
        .flatten()
        .filter_map(|t| std::fs::read_to_string(t.path().join("stat")).ok())
        .filter_map(|s| parse_stat(&s))
        .collect();
    threads.sort_by_key(|t| t.tid);
    Some(threads)
}

/// Every thread: name, state, wait channel, and CPU used over the next second (Linux clock
/// ticks are 10 ms), busiest first. `None` without `/proc`.
fn threads() -> Option<String> {
    let before = sample()?;
    std::thread::sleep(Duration::from_secs(1));
    let after = sample()?;
    let mut rows: Vec<(u64, String)> = after
        .iter()
        .map(|t| {
            let was = before.iter().find(|b| b.tid == t.tid).map_or(t.cpu_ticks, |b| b.cpu_ticks);
            let cpu_ms = t.cpu_ticks.saturating_sub(was) * 10;
            let wchan = std::fs::read_to_string(format!("/proc/self/task/{}/wchan", t.tid))
                .ok()
                .filter(|w| !w.is_empty() && w != "0")
                .unwrap_or_else(|| "-".into());
            (cpu_ms, format!("{}({}) {} {} {}ms", t.name, t.tid, t.state, wchan.trim(), cpu_ms))
        })
        .collect();
    rows.sort_by_key(|row| std::cmp::Reverse(row.0));
    Some(rows.into_iter().map(|(_, row)| row).collect::<Vec<_>>().join("; "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_thread_stat_line_even_with_odd_names() {
        let stat = "4242 (EE (Core) 1)) R 1 4242 4242 0 -1 4194560 100 0 0 0 1234 56 0 0 20 0 30 0";
        let t = parse_stat(stat).unwrap();
        assert_eq!((t.tid, t.name.as_str(), t.state), (4242, "EE (Core) 1)", 'R'));
        assert_eq!(t.cpu_ticks, 1234 + 56);
        assert_eq!(parse_stat("garbage"), None);
    }

    #[test]
    fn names_the_phase() {
        assert_eq!(Phase::name(Phase::Emulating as u8), "emulating (inside the core)");
        assert_eq!(Phase::name(Phase::Network as u8), "network");
    }

    #[test]
    fn a_beat_records_the_phase() {
        beat(Phase::Encoding);
        assert_eq!(PHASE.load(Ordering::Relaxed), Phase::Encoding as u8);
        assert!(now_ms() >= BEAT_MS.load(Ordering::Relaxed));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn samples_this_process_threads() {
        let threads = sample().unwrap();
        assert!(threads.iter().any(|t| t.tid == std::process::id()), "{threads:?}");
    }
}
