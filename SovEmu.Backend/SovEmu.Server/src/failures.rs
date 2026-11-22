//! The game sessions that went wrong, for the admin Sessions page: starts the server refused
//! (no core, a missing file, no 3D rendering, every port in use), and workers that failed to
//! load their game, crashed, or had to be killed, with the worker's last lines.
//!
//! Kept in memory, newest first, up to `SOVEMU_SESSION_FAILURES`: the log has the rest. Workers
//! report through `sovemu_sessions`' [`Observer`]; the server records its own refusals
//! ([`SessionFailures::refused`]), which no worker ever saw.

use std::collections::VecDeque;
use std::sync::Mutex;

use sovemu_sessions::{EndReason, Ended, Observer};

/// How far a failed session got.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Phase {
    /// The server refused it before any worker ran.
    Starting,
    /// Its worker ended before the game had loaded.
    Loading,
    /// It had been playing.
    Running,
}

/// One session that went wrong.
#[derive(Debug, Clone)]
pub(crate) struct Failure {
    /// When, as a Unix time.
    pub(crate) at: f64,
    pub(crate) user_id: i64,
    pub(crate) game_id: i64,
    pub(crate) title: String,
    pub(crate) system: String,
    /// The core's file name without its extension; none if the server found none.
    pub(crate) core: Option<String>,
    pub(crate) phase: Phase,
    /// Why its worker ended; none for a start the server refused.
    pub(crate) reason: Option<EndReason>,
    /// What went wrong, in words: the refusal, or why the start failed.
    pub(crate) message: Option<String>,
    pub(crate) code: Option<i32>,
    pub(crate) signal: Option<i32>,
    /// The worker's last lines on stderr, oldest first.
    pub(crate) last_lines: Vec<String>,
}

impl Failure {
    fn from_ended(ended: &Ended<'_>, at: f64) -> Self {
        Self {
            at,
            user_id: ended.user_id,
            game_id: ended.game_id,
            title: ended.title.to_owned(),
            system: ended.system.to_owned(),
            core: Some(ended.core.to_owned()),
            phase: if ended.loaded { Phase::Running } else { Phase::Loading },
            reason: Some(ended.reason),
            message: ended.start_error.map(str::to_owned),
            code: ended.code,
            signal: ended.signal,
            last_lines: ended.last_lines.to_vec(),
        }
    }
}

/// The game a refused start was for.
pub(crate) struct Refused<'a> {
    pub(crate) user_id: i64,
    pub(crate) game_id: i64,
    pub(crate) title: &'a str,
    pub(crate) system: &'a str,
    pub(crate) core: Option<&'a std::path::Path>,
}

/// The last failed sessions, newest first.
#[derive(Debug)]
pub(crate) struct SessionFailures {
    kept: usize,
    failures: Mutex<VecDeque<Failure>>,
}

impl SessionFailures {
    /// Keeps the last `kept` failures (0: none).
    pub(crate) fn new(kept: usize) -> Self {
        Self { kept, failures: Mutex::new(VecDeque::with_capacity(kept.min(256))) }
    }

    /// Records a start the server refused, `message` saying why.
    pub(crate) fn refused(&self, start: Refused<'_>, message: &str) {
        self.record(Failure {
            at: now(),
            user_id: start.user_id,
            game_id: start.game_id,
            title: start.title.to_owned(),
            system: start.system.to_owned(),
            core: start.core.and_then(|c| c.file_stem()).map(|s| s.to_string_lossy().into_owned()),
            phase: Phase::Starting,
            reason: None,
            message: Some(message.to_owned()),
            code: None,
            signal: None,
            last_lines: Vec::new(),
        });
    }

    /// The failures kept, newest first.
    pub(crate) fn recent(&self) -> Vec<Failure> {
        self.lock().iter().cloned().collect()
    }

    fn record(&self, failure: Failure) {
        if self.kept == 0 {
            return;
        }
        let mut failures = self.lock();
        failures.truncate(self.kept - 1);
        failures.push_front(failure);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, VecDeque<Failure>> {
        self.failures.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl Observer for SessionFailures {
    fn session_ended(&self, ended: &Ended<'_>) {
        if ended.failed() {
            self.record(Failure::from_ended(ended, now()));
        }
    }
}

fn now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ended(reason: EndReason, loaded: bool, start_error: Option<&str>) -> Ended<'_> {
        Ended {
            session_id: "s",
            user_id: 1,
            game_id: 7,
            title: "Game",
            system: "snes",
            core: "snes9x_libretro",
            reason,
            loaded,
            start_error,
            code: None,
            signal: Some(11),
            last_lines: &[],
            ran: std::time::Duration::ZERO,
        }
    }

    fn refused(failures: &SessionFailures, title: &str) {
        failures.refused(
            Refused { user_id: 1, game_id: 1, title, system: "psx", core: None },
            "no core installed for PlayStation",
        );
    }

    #[test]
    fn keeps_failures_only() {
        let failures = SessionFailures::new(10);
        failures.session_ended(&ended(EndReason::Stopped, true, None));
        failures.session_ended(&ended(EndReason::Idle, true, None));
        assert!(failures.recent().is_empty(), "a session that ended normally is not a failure");
        failures.session_ended(&ended(EndReason::Crashed, true, None));
        failures.session_ended(&ended(EndReason::Exited, false, Some("the worker exited")));
        let recent = failures.recent();
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].phase, Phase::Loading, "newest first");
        assert_eq!(recent[0].message.as_deref(), Some("the worker exited"));
        assert_eq!((recent[1].phase, recent[1].signal), (Phase::Running, Some(11)));
    }

    #[test]
    fn keeps_the_newest_up_to_its_limit() {
        let failures = SessionFailures::new(2);
        for title in ["a", "b", "c"] {
            refused(&failures, title);
        }
        let titles: Vec<String> = failures.recent().into_iter().map(|f| f.title).collect();
        assert_eq!(titles, ["c", "b"]);
        let f = &failures.recent()[0];
        assert_eq!((f.phase, f.reason, f.core.as_deref()), (Phase::Starting, None, None));
    }

    #[test]
    fn keeps_none_when_off() {
        let failures = SessionFailures::new(0);
        refused(&failures, "a");
        assert!(failures.recent().is_empty());
    }
}
