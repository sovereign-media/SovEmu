//! Progress while a session loads, for the player's loading bar.
//!
//! Most of a slow start is the game being read: SovEmu reading a cartridge into memory, or a
//! core reading a disc on a network share. Bytes read are counted from the kernel's view of
//! this process (`rchar` in `/proc/self/io`), so a core that reads its content itself shows
//! progress too; elsewhere (macOS) only the phase is reported.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use sovemu_protocol::ipc::LoadPhase;

/// Receives `(phase, bytes read in it so far, bytes it will read)`; called from a sampling
/// thread as well as the loading one.
pub type Report<'a> = &'a (dyn Fn(LoadPhase, Option<u64>, Option<u64>) + Sync);

/// A report that goes nowhere (`serve`, `headless`).
pub fn ignore(_: LoadPhase, _: Option<u64>, _: Option<u64>) {}

const INTERVAL: Duration = Duration::from_millis(250);

/// Bytes this process has read through system calls so far, where the platform says.
pub fn bytes_read() -> Option<u64> {
    parse_rchar(&std::fs::read_to_string("/proc/self/io").ok()?)
}

fn parse_rchar(io: &str) -> Option<u64> {
    io.lines().find_map(|l| l.strip_prefix("rchar:")).and_then(|v| v.trim().parse().ok())
}

/// Runs `work` on this thread (a GL context may be current here), reporting `phase` and the
/// bytes read since it began, at the start and every [`INTERVAL`] until it returns.
pub fn reporting<T>(
    phase: LoadPhase,
    total: Option<u64>,
    report: Report<'_>,
    work: impl FnOnce() -> T,
) -> T {
    let base = bytes_read();
    let read = || base.and_then(|b| bytes_read().map(|n| n.saturating_sub(b)));
    report(phase, read(), total);
    let done = AtomicBool::new(false);
    std::thread::scope(|scope| {
        let sampler = scope.spawn(|| {
            loop {
                std::thread::park_timeout(INTERVAL);
                if done.load(Ordering::Acquire) {
                    break;
                }
                report(phase, read(), total);
            }
        });
        let out = work();
        done.store(true, Ordering::Release);
        sampler.thread().unpark();
        out
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    #[test]
    fn parses_rchar() {
        let io = "rchar: 123456\nwchar: 7\nsyscr: 3\n";
        assert_eq!(parse_rchar(io), Some(123_456));
        assert_eq!(parse_rchar("nothing"), None);
    }

    #[test]
    fn reports_at_the_start_and_while_working() {
        let seen = Mutex::new(Vec::new());
        let report =
            |phase, read: Option<u64>, total| seen.lock().unwrap().push((phase, read, total));
        let out = reporting(LoadPhase::Content, Some(10), &report, || {
            std::thread::sleep(Duration::from_millis(600));
            42
        });
        assert_eq!(out, 42);
        let seen = seen.into_inner().unwrap();
        assert!(seen.len() >= 2, "a report at once and at least one while working: {seen:?}");
        assert!(seen.iter().all(|(p, _, t)| *p == LoadPhase::Content && *t == Some(10)));
        if cfg!(target_os = "linux") {
            assert!(seen[0].1.is_some(), "Linux counts bytes read");
        }
    }
}
