//! Games that failed: starts the server refused before any worker ran (no core, a missing file,
//! no 3D rendering, every port in use), and workers that failed to start or crashed, errored or
//! had to be killed while playing, with the core's last log lines. For the admin's "why won't
//! this game run" (the Sessions page). The most recent are kept in memory,
//! `SOVEMU_SESSION_FAILURES` of them; the log has every one.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use sovemu_sessions::{EndReason, Ended, Observer};

/// A session that failed.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Failure {
    /// When it ended (Unix seconds).
    pub(crate) at: f64,
    pub(crate) user_id: i64,
    pub(crate) game_id: i64,
    pub(crate) title: String,
    pub(crate) system: String,
    /// The core's file name without its extension; none for a start refused before one was found.
    pub(crate) core: Option<String>,
    /// It had loaded: a failure while playing, else while starting.
    pub(crate) loaded: bool,
    /// How its worker ended; none for a start the server refused before any worker ran.
    pub(crate) reason: Option<EndReason>,
    /// What went wrong, in words: why the server refused it, or why its start failed.
    pub(crate) message: Option<String>,
    pub(crate) code: Option<i32>,
    pub(crate) signal: Option<i32>,
    pub(crate) last_lines: Vec<String>,
    /// How long it ran, seconds.
    pub(crate) ran_s: f64,
}

impl Failure {
    fn from_ended(ended: &Ended<'_>) -> Self {
        Self {
            at: now(),
            user_id: ended.user_id,
            game_id: ended.game_id,
            title: ended.title.to_owned(),
            system: ended.system.to_owned(),
            core: Some(ended.core.to_owned()),
            loaded: ended.loaded,
            reason: Some(ended.reason),
            message: ended.start_error.map(str::to_owned),
            code: ended.code,
            signal: ended.signal,
            last_lines: ended.last_lines.to_vec(),
            ran_s: ended.ran.as_secs_f64(),
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

fn now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64())
}

/// The most recent failures, newest last.
#[derive(Debug)]
pub(crate) struct Failures {
    kept: usize,
    list: Mutex<VecDeque<Failure>>,
}

impl Failures {
    /// Keeps up to `kept` failures (0: none).
    pub(crate) fn new(kept: usize) -> Self {
        Self { kept, list: Mutex::new(VecDeque::new()) }
    }

    pub(crate) fn record(&self, failure: Failure) {
        if self.kept == 0 {
            return;
        }
        let mut list = self.list.lock().unwrap_or_else(|e| e.into_inner());
        list.push_back(failure);
        while list.len() > self.kept {
            list.pop_front();
        }
    }

    /// Records a start the server refused before any worker ran, `message` saying why.
    pub(crate) fn refused(&self, start: Refused<'_>, message: &str) {
        self.record(Failure {
            at: now(),
            user_id: start.user_id,
            game_id: start.game_id,
            title: start.title.to_owned(),
            system: start.system.to_owned(),
            core: start.core.and_then(|c| c.file_stem()).map(|s| s.to_string_lossy().into_owned()),
            loaded: false,
            reason: None,
            message: Some(message.to_owned()),
            code: None,
            signal: None,
            last_lines: Vec::new(),
            ran_s: 0.0,
        });
    }

    /// The failures kept, newest first.
    pub(crate) fn recent(&self) -> Vec<Failure> {
        self.list.lock().unwrap_or_else(|e| e.into_inner()).iter().rev().cloned().collect()
    }
}

/// The failures kept, newest first, each with its player's name (none for a deleted account).
pub(crate) async fn recent_with_players(
    state: &crate::AppState,
) -> crate::error::ApiResult<Vec<(Failure, Option<String>)>> {
    let failures = state.failures.recent();
    if failures.is_empty() {
        return Ok(Vec::new());
    }
    let names = crate::api::usernames(state).await?;
    Ok(failures
        .into_iter()
        .map(|f| {
            let name = names.get(&f.user_id).cloned();
            (f, name)
        })
        .collect())
}

/// Keeps the sessions that failed, as the workers report them ending.
impl Observer for Failures {
    fn session_ended(&self, ended: &Ended<'_>) {
        if ended.failed() {
            self.record(Failure::from_ended(ended));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ended(reason: EndReason, lines: &[String]) -> Ended<'_> {
        Ended {
            user_id: 1,
            game_id: 7,
            title: "Tony Hawk's Pro Skater 3",
            system: "ps2",
            core: "pcsx2_libretro",
            reason,
            ran: std::time::Duration::from_secs(3),
            loaded: false,
            code: Some(1),
            signal: None,
            last_lines: lines,
            start_error: None,
        }
    }

    #[test]
    fn only_failures_are_kept_newest_first_up_to_the_limit() {
        let failures = std::sync::Arc::new(Failures::new(2));
        let events = failures.clone();
        let lines = vec!["no BIOS".to_owned()];
        for reason in [EndReason::Stopped, EndReason::Idle, EndReason::Exited] {
            events.session_ended(&ended(reason, &lines));
        }
        assert!(failures.recent().is_empty(), "quitting and idling out are not failures");
        for reason in [EndReason::Error, EndReason::Crashed, EndReason::Killed] {
            events.session_ended(&ended(reason, &lines));
        }
        let reasons: Vec<Option<EndReason>> = failures.recent().iter().map(|f| f.reason).collect();
        assert_eq!(
            reasons,
            [Some(EndReason::Killed), Some(EndReason::Crashed)],
            "the newest two, newest first"
        );
        assert_eq!(failures.recent()[0].last_lines, lines);
    }

    #[test]
    fn a_failed_start_is_kept_with_why() {
        let failures = std::sync::Arc::new(Failures::new(5));
        let events = failures.clone();
        let lines = vec!["no BIOS".to_owned()];
        // The worker exited without an error code, but its game never loaded.
        events.session_ended(&Ended {
            start_error: Some("the worker exited while loading the game"),
            ..ended(EndReason::Exited, &lines)
        });
        let kept = &failures.recent()[0];
        assert_eq!(kept.message.as_deref(), Some("the worker exited while loading the game"));
        assert_eq!((kept.loaded, kept.reason), (false, Some(EndReason::Exited)));
    }

    #[test]
    fn a_refused_start_is_kept_without_a_worker() {
        let failures = Failures::new(5);
        let start =
            |core| Refused { user_id: 1, game_id: 3, title: "Ridge Racer", system: "psx", core };
        failures.refused(start(None), "no core installed for PlayStation");
        failures.refused(
            start(Some(std::path::Path::new("/cores/pcsx_rearmed_libretro.so"))),
            "the game file is missing; rescan the library",
        );
        let kept = failures.recent();
        assert_eq!(kept[0].core.as_deref(), Some("pcsx_rearmed_libretro"));
        assert_eq!(kept[1].core, None);
        assert_eq!(kept[1].message.as_deref(), Some("no core installed for PlayStation"));
        assert!(kept.iter().all(|f| f.reason.is_none() && f.last_lines.is_empty()));
    }

    #[test]
    fn none_are_kept_when_turned_off() {
        let failures = Failures::new(0);
        failures.record(Failure {
            at: 0.0,
            user_id: 1,
            game_id: 1,
            title: String::new(),
            system: String::new(),
            core: None,
            loaded: true,
            reason: Some(EndReason::Crashed),
            message: None,
            code: None,
            signal: Some(11),
            last_lines: Vec::new(),
            ran_s: 0.0,
        });
        assert!(failures.recent().is_empty());
    }
}
