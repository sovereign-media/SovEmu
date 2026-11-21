//! What goes wrong on a client, in the server's log. The web, Android and iOS clients batch
//! events (a connection that failed, a picture that stalled, a controller nobody knows, a
//! screen that crashed) and send them with `reportClientEvents`. Each becomes a `client event`
//! line, with the session id when it happened in a game, so it sits next to that game's worker
//! lines (`session health`, `client stats`).
//!
//! Nothing is stored or served: the log, and OTLP where it is configured, is where they go. A
//! user may send [`PER_MINUTE`] a minute; past that they are dropped, never queued. With
//! `SOVEMU_CLIENT_EVENTS=false` the mutation still answers, and drops them all.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Events a user may send in a minute.
pub(crate) const PER_MINUTE: u32 = 60;
/// Events in one call.
pub(crate) const MAX_BATCH: usize = 20;
/// Characters kept of a message, and of the client's name.
const MAX_MESSAGE: usize = 500;
const MAX_CLIENT: usize = 120;

/// What kind of thing happened, as the clients name it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// WebRTC could not connect.
    ConnectionFailed,
    /// A connected stream dropped.
    ConnectionLost,
    /// No new picture for a while during play.
    StreamStalled,
    /// A controller the client has no layout for.
    ControllerUnknown,
    /// A screen or the app failed (an exception, a crash caught on the way down).
    ScreenError,
    Other,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Self::ConnectionFailed => "connection_failed",
            Self::ConnectionLost => "connection_lost",
            Self::StreamStalled => "stream_stalled",
            Self::ControllerUnknown => "controller_unknown",
            Self::ScreenError => "screen_error",
            Self::Other => "other",
        }
    }

    /// Faults at warn; what is worth knowing but no fault at info.
    fn is_fault(self) -> bool {
        matches!(
            self,
            Self::ConnectionFailed | Self::ConnectionLost | Self::StreamStalled | Self::ScreenError
        )
    }
}

pub(crate) struct Event {
    pub(crate) kind: Kind,
    pub(crate) message: String,
    pub(crate) session_id: Option<String>,
    /// Seconds since the Unix epoch, on the client's clock.
    pub(crate) at: Option<f64>,
}

/// Each user's count in the current minute.
#[derive(Default)]
pub(crate) struct ClientEvents {
    windows: Mutex<HashMap<i64, (Instant, u32)>>,
}

impl ClientEvents {
    /// Logs what `user_id`'s client (`client`: its name and version) sent, within the user's
    /// allowance. Returns how many were logged.
    pub(crate) fn report(&self, user_id: i64, client: &str, events: &[Event], now: Instant) -> u32 {
        let allowed = self.take(user_id, events.len().min(MAX_BATCH) as u32, now);
        let client = clean(client, MAX_CLIENT);
        for event in &events[..allowed as usize] {
            log(user_id, &client, event);
        }
        if (allowed as usize) < events.len() {
            tracing::debug!(
                user.id = user_id,
                dropped = events.len() - allowed as usize,
                "client events over the allowance: dropped"
            );
        }
        allowed
    }

    /// Takes up to `wanted` from the user's allowance for this minute; returns how many.
    fn take(&self, user_id: i64, wanted: u32, now: Instant) -> u32 {
        let mut windows = self.windows.lock().unwrap_or_else(|e| e.into_inner());
        // Forget finished minutes now and then, so the map stays the size of the active users.
        if windows.len() > 64 {
            windows.retain(|_, (from, _)| now.duration_since(*from) < Duration::from_secs(60));
        }
        let (from, used) = windows.entry(user_id).or_insert((now, 0));
        if now.duration_since(*from) >= Duration::from_secs(60) {
            (*from, *used) = (now, 0);
        }
        let allowed = wanted.min(PER_MINUTE - *used);
        *used += allowed;
        allowed
    }
}

fn log(user_id: i64, client: &str, event: &Event) {
    let message = clean(&event.message, MAX_MESSAGE);
    let session = event.session_id.as_deref().map(|id| clean(id, 64));
    macro_rules! event {
        ($level:ident) => {
            tracing::$level!(
                user.id = user_id,
                client,
                kind = event.kind.as_str(),
                session.id = session.as_deref(),
                at = event.at,
                message = %message,
                "client event"
            )
        };
    }
    if event.kind.is_fault() {
        event!(warn);
    } else {
        event!(info);
    }
}

/// At most `max` characters, control characters (newlines among them) as spaces, so a client
/// can neither flood a line nor forge one in the text log.
fn clean(text: &str, max: usize) -> String {
    text.chars().take(max).map(|c| if c.is_control() { ' ' } else { c }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(kind: Kind) -> Event {
        Event { kind, message: "it broke".into(), session_id: None, at: None }
    }

    #[test]
    fn a_user_gets_so_many_a_minute_then_a_fresh_allowance() {
        let events = ClientEvents::default();
        let t = Instant::now();
        let batch: Vec<Event> = (0..MAX_BATCH).map(|_| event(Kind::Other)).collect();
        assert_eq!(events.report(1, "web", &batch, t), 20);
        assert_eq!(events.report(1, "web", &batch, t), 20);
        assert_eq!(events.report(1, "web", &batch, t), 20);
        assert_eq!(events.report(1, "web", &batch, t), 0, "sixty in the minute");
        assert_eq!(events.report(2, "web", &batch, t), 20, "each user has their own");
        assert_eq!(events.report(1, "web", &batch, t + Duration::from_secs(60)), 20);
    }

    #[test]
    fn a_batch_counts_no_more_than_its_cap() {
        let events = ClientEvents::default();
        let many: Vec<Event> = (0..50).map(|_| event(Kind::Other)).collect();
        assert_eq!(events.report(1, "web", &many, Instant::now()), MAX_BATCH as u32);
    }

    #[test]
    fn text_cannot_forge_a_line_or_run_on() {
        assert_eq!(clean("one\ntwo\r\tthree", 100), "one two  three");
        assert_eq!(clean(&"x".repeat(1000), MAX_MESSAGE).len(), MAX_MESSAGE);
    }
}
