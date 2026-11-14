//! How far each game start has got, for the player's loading bar.
//!
//! `POST /api/sessions` blocks until the game is ready, which on a slow library can take a
//! minute. A client that wants progress sends a `start_id` of its own with that request and
//! polls `GET /api/sessions/starting/{start_id}` while it waits. The phases come from the server
//! (saving the previous session, starting the worker) and from the worker's `progress` lines
//! (opening the core, reading the game). Entries are dropped a minute after they finish.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use sovemu_protocol::ipc::LoadPhase;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// Saving and stopping the player's previous session.
    Stopping,
    /// Starting the worker process.
    Launching,
    /// Opening the emulator core.
    Core,
    /// Reading the game.
    Content,
    Ready,
    Failed,
}

impl From<LoadPhase> for Phase {
    fn from(p: LoadPhase) -> Self {
        match p {
            LoadPhase::Core => Self::Core,
            LoadPhase::Content => Self::Content,
        }
    }
}

/// What a client sees.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StartStatus {
    pub phase: Phase,
    /// Bytes read so far in this phase, where known.
    pub read: Option<u64>,
    /// Bytes this phase will read, when known.
    pub total: Option<u64>,
    /// Since the start began, and since this phase began.
    pub elapsed_ms: u64,
    pub phase_ms: u64,
    pub error: Option<String>,
}

#[derive(Debug)]
struct Start {
    user_id: i64,
    phase: Phase,
    read: Option<u64>,
    total: Option<u64>,
    began: Instant,
    phase_began: Instant,
    error: Option<String>,
    finished: Option<Instant>,
}

/// Starts in progress (and recently finished), by client-chosen id.
#[derive(Debug, Default)]
pub struct Starts(Mutex<HashMap<String, Start>>);

/// Finished starts are kept this long, for a poll that lands after the POST returned.
const KEEP: Duration = Duration::from_secs(60);
/// Unfinished ones this long (a worker that never answered is long gone by then).
const ABANDON: Duration = Duration::from_secs(600);

/// A client's start id: short, and safe to put in a URL.
pub fn valid_id(id: &str) -> bool {
    (8..=64).contains(&id.len())
        && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

impl Starts {
    fn map(&self) -> std::sync::MutexGuard<'_, HashMap<String, Start>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn begin(&self, id: &str, user_id: i64, now: Instant) {
        let mut map = self.map();
        map.retain(|_, s| match s.finished {
            Some(at) => now.duration_since(at) < KEEP,
            None => now.duration_since(s.began) < ABANDON,
        });
        map.insert(
            id.to_owned(),
            Start {
                user_id,
                phase: Phase::Launching,
                read: None,
                total: None,
                began: now,
                phase_began: now,
                error: None,
                finished: None,
            },
        );
    }

    pub fn update(
        &self,
        id: &str,
        phase: Phase,
        read: Option<u64>,
        total: Option<u64>,
        now: Instant,
    ) {
        if let Some(s) = self.map().get_mut(id) {
            if s.phase != phase {
                s.phase = phase;
                s.phase_began = now;
            }
            s.read = read;
            s.total = total;
        }
    }

    pub fn finish(&self, id: &str, error: Option<String>, now: Instant) {
        if let Some(s) = self.map().get_mut(id) {
            s.phase = if error.is_some() { Phase::Failed } else { Phase::Ready };
            s.phase_began = now;
            s.read = None;
            s.total = None;
            s.error = error;
            s.finished = Some(now);
        }
    }

    /// The start `id`, if it is `user_id`'s.
    pub fn get(&self, id: &str, user_id: i64, now: Instant) -> Option<StartStatus> {
        let map = self.map();
        let s = map.get(id).filter(|s| s.user_id == user_id)?;
        let ms = |since: Instant| now.duration_since(since).as_millis() as u64;
        Some(StartStatus {
            phase: s.phase,
            read: s.read,
            total: s.total,
            elapsed_ms: ms(s.began),
            phase_ms: ms(s.phase_began),
            error: s.error.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follows_a_start_through_its_phases() {
        let starts = Starts::default();
        let t0 = Instant::now();
        starts.begin("start-0001", 7, t0);
        assert_eq!(starts.get("start-0001", 7, t0).unwrap().phase, Phase::Launching);
        assert!(starts.get("start-0001", 8, t0).is_none(), "another user's start is private");

        let t1 = t0 + Duration::from_secs(2);
        starts.update("start-0001", Phase::Content, Some(10), Some(100), t1);
        starts.update(
            "start-0001",
            Phase::Content,
            Some(50),
            Some(100),
            t1 + Duration::from_secs(1),
        );
        let s = starts.get("start-0001", 7, t1 + Duration::from_secs(1)).unwrap();
        assert_eq!((s.phase, s.read, s.total), (Phase::Content, Some(50), Some(100)));
        assert_eq!(
            (s.elapsed_ms, s.phase_ms),
            (3000, 1000),
            "the phase clock starts at the change"
        );

        starts.finish("start-0001", Some("no core".into()), t1);
        let s = starts.get("start-0001", 7, t1).unwrap();
        assert_eq!((s.phase, s.error.as_deref()), (Phase::Failed, Some("no core")));
    }

    #[test]
    fn forgets_finished_and_abandoned_starts() {
        let starts = Starts::default();
        let t0 = Instant::now();
        starts.begin("finished-1", 1, t0);
        starts.finish("finished-1", None, t0);
        starts.begin("abandoned-1", 1, t0);
        let later = t0 + ABANDON + Duration::from_secs(1);
        starts.begin("new-start-1", 1, later);
        assert!(starts.get("finished-1", 1, later).is_none());
        assert!(starts.get("abandoned-1", 1, later).is_none());
        assert!(starts.get("new-start-1", 1, later).is_some());
    }

    #[test]
    fn ids_are_url_safe() {
        assert!(valid_id("0f8e2a4c-1b2d-4e5f-9a8b-7c6d5e4f3a2b"));
        assert!(!valid_id("short"));
        assert!(!valid_id("has/slash-in-it"));
        assert!(!valid_id(&"x".repeat(65)));
    }
}
