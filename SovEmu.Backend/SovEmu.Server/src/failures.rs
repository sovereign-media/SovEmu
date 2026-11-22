//! Games that failed: those that ended while starting with an error, or while playing in a crash,
//! an error or a kill, with the core's last log lines. For the admin's "why won't this game run"
//! (the Sessions page). The most recent are kept in memory, `SOVEMU_SESSION_FAILURES` of them; the
//! log has every one.

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
    pub(crate) core: String,
    /// It had loaded: a failure while playing, else while starting.
    pub(crate) loaded: bool,
    pub(crate) reason: EndReason,
    pub(crate) code: Option<i32>,
    pub(crate) signal: Option<i32>,
    pub(crate) last_lines: Vec<String>,
    /// How long it ran, seconds.
    pub(crate) ran_s: f64,
}

/// Whether an end is a failure worth showing: an error, a crash, or a worker the server had to
/// kill (it would not stop, or its start timed out). Quitting and idling out are not.
pub(crate) fn is_failure(reason: EndReason) -> bool {
    matches!(reason, EndReason::Error | EndReason::Crashed | EndReason::Killed)
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

    /// The failures kept, newest first.
    pub(crate) fn recent(&self) -> Vec<Failure> {
        self.list.lock().unwrap_or_else(|e| e.into_inner()).iter().rev().cloned().collect()
    }
}

/// The failures kept, newest first, each with its player's name (none for a deleted account).
pub(crate) async fn recent_with_players(
    state: &crate::AppState,
) -> crate::error::ApiResult<Vec<(Failure, Option<String>)>> {
    let names: std::collections::HashMap<i64, String> =
        sqlx::query_as::<_, (i64, String)>("SELECT id, username FROM users")
            .fetch_all(&state.db)
            .await?
            .into_iter()
            .collect();
    Ok(state
        .failures
        .recent()
        .into_iter()
        .map(|f| {
            let name = names.get(&f.user_id).cloned();
            (f, name)
        })
        .collect())
}

/// What the server does as sessions start and end: the metrics, and the failures kept.
#[derive(Debug)]
pub(crate) struct SessionEvents {
    pub(crate) failures: std::sync::Arc<Failures>,
}

impl Observer for SessionEvents {
    fn session_started(&self, system: &str) {
        crate::telemetry::SessionMetrics.session_started(system);
    }

    fn session_ended(&self, ended: &Ended<'_>) {
        crate::telemetry::SessionMetrics.session_ended(ended);
        if is_failure(ended.reason) {
            self.failures.record(Failure {
                at: SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64()),
                user_id: ended.user_id,
                game_id: ended.game_id,
                title: ended.title.to_owned(),
                system: ended.system.to_owned(),
                core: ended.core.to_owned(),
                loaded: ended.loaded,
                reason: ended.reason,
                code: ended.code,
                signal: ended.signal,
                last_lines: ended.last_lines.to_vec(),
                ran_s: ended.ran.as_secs_f64(),
            });
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
        }
    }

    #[test]
    fn only_failures_are_kept_newest_first_up_to_the_limit() {
        let failures = std::sync::Arc::new(Failures::new(2));
        let events = SessionEvents { failures: failures.clone() };
        let lines = vec!["no BIOS".to_owned()];
        for reason in [EndReason::Stopped, EndReason::Idle, EndReason::Exited] {
            events.session_ended(&ended(reason, &lines));
        }
        assert!(failures.recent().is_empty(), "quitting and idling out are not failures");
        for reason in [EndReason::Error, EndReason::Crashed, EndReason::Killed] {
            events.session_ended(&ended(reason, &lines));
        }
        let reasons: Vec<EndReason> = failures.recent().iter().map(|f| f.reason).collect();
        assert_eq!(
            reasons,
            [EndReason::Killed, EndReason::Crashed],
            "the newest two, newest first"
        );
        assert_eq!(failures.recent()[0].last_lines, lines);
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
            core: String::new(),
            loaded: true,
            reason: EndReason::Crashed,
            code: None,
            signal: Some(11),
            last_lines: Vec::new(),
            ran_s: 0.0,
        });
        assert!(failures.recent().is_empty());
    }
}
