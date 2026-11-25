//! What goes wrong on a client. The web, Android and iOS clients batch events (a connection
//! that failed, a picture that stalled, a controller nobody knows, a screen that crashed) and
//! send them with `reportClientEvents`. Each becomes a `client event` line, with the session id
//! when it happened in a game, so it sits next to that game's worker lines (`session health`,
//! `client stats`), and a row in `client_events` for the admin Client errors page, which
//! maintenance prunes after `SovEmu__ClientErrors__RetentionDays`.
//!
//! A user may send [`PER_MINUTE`] a minute; past that they are dropped, never queued. With
//! `SovEmu__ClientErrors__Enabled=false` the mutation still answers, and drops them all.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use sqlx::SqlitePool;

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
    const ALL: [Self; 6] = [
        Self::ConnectionFailed,
        Self::ConnectionLost,
        Self::StreamStalled,
        Self::ControllerUnknown,
        Self::ScreenError,
        Self::Other,
    ];

    /// As it is logged and stored.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::ConnectionFailed => "connection_failed",
            Self::ConnectionLost => "connection_lost",
            Self::StreamStalled => "stream_stalled",
            Self::ControllerUnknown => "controller_unknown",
            Self::ScreenError => "screen_error",
            Self::Other => "other",
        }
    }

    /// From what [`Kind::as_str`] wrote; anything else is `Other`.
    pub(crate) fn parse(text: &str) -> Self {
        Self::ALL.into_iter().find(|k| k.as_str() == text).unwrap_or(Self::Other)
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

/// An event the server took: within the user's allowance, its text cleaned as it is logged.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Accepted {
    pub(crate) kind: Kind,
    pub(crate) message: String,
    pub(crate) session_id: Option<String>,
    pub(crate) client_at: Option<f64>,
}

/// Each user's count in the current minute.
#[derive(Default)]
pub(crate) struct ClientEvents {
    windows: Mutex<HashMap<i64, (Instant, u32)>>,
}

impl ClientEvents {
    /// Logs what `user_id`'s client (`client`: its name and version) sent, within the user's
    /// allowance, and returns what it took, to be stored ([`store`]).
    pub(crate) fn report(
        &self,
        user_id: i64,
        client: &str,
        events: &[Event],
        now: Instant,
    ) -> Vec<Accepted> {
        let allowed = self.take(user_id, events.len().min(MAX_BATCH) as u32, now);
        let accepted: Vec<Accepted> = events[..allowed as usize]
            .iter()
            .map(|e| Accepted {
                kind: e.kind,
                message: clean(&e.message, MAX_MESSAGE),
                session_id: e.session_id.as_deref().map(|id| clean(id, 64)),
                client_at: e.at,
            })
            .collect();
        for event in &accepted {
            log(user_id, client, event);
        }
        if (allowed as usize) < events.len() {
            tracing::debug!(
                user.id = user_id,
                dropped = events.len() - allowed as usize,
                "client events over the allowance: dropped"
            );
        }
        accepted
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

fn log(user_id: i64, client: &str, event: &Accepted) {
    let client = clean(client, MAX_CLIENT);
    macro_rules! event {
        ($level:ident) => {
            tracing::$level!(
                user.id = user_id,
                client,
                kind = event.kind.as_str(),
                session.id = event.session_id.as_deref(),
                at = event.client_at,
                message = %event.message,
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

/// What a signed-in user's client sent (`reportClientEvents`): logged and kept within the
/// user's allowance, or dropped with `SovEmu__ClientErrors__Enabled` off. How many were taken.
pub(crate) async fn receive(
    state: &crate::AppState,
    user_id: i64,
    client: &str,
    events: &[Event],
) -> sqlx::Result<usize> {
    if !state.config.client_events {
        return Ok(0);
    }
    let taken = state.client_events.report(user_id, client, events, Instant::now());
    // The session's game, while it still runs: a session id alone says nothing once it ends.
    let game_of =
        |id: &str| state.workers.get(id).filter(|s| s.user_id == user_id).map(|s| s.info().game_id);
    store(&state.db, user_id, client, &taken, now_unix(), game_of).await?;
    Ok(taken.len())
}

/// The time, in Unix seconds.
pub(crate) fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// Keeps what `user_id`'s client reported, at Unix time `at`. `game_of` names the game of a
/// session that is still running, so the page can say what was being played.
pub(crate) async fn store(
    db: &SqlitePool,
    user_id: i64,
    client: &str,
    events: &[Accepted],
    at: i64,
    game_of: impl Fn(&str) -> Option<i64>,
) -> sqlx::Result<()> {
    if events.is_empty() {
        return Ok(());
    }
    let client = clean(client, MAX_CLIENT);
    let mut tx = db.begin().await?;
    for e in events {
        sqlx::query(
            "INSERT INTO client_events
               (at, client_at, user_id, client, kind, message, session_id, game_id)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(at)
        .bind(e.client_at)
        .bind(user_id)
        .bind(&client)
        .bind(e.kind.as_str())
        .bind(&e.message)
        .bind(&e.session_id)
        .bind(e.session_id.as_deref().and_then(&game_of))
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await
}

/// Deletes events older than `days` (0: kept). How many.
pub(crate) async fn prune(db: &SqlitePool, days: u32, now: i64) -> sqlx::Result<u64> {
    if days == 0 {
        return Ok(0);
    }
    let cutoff = now - i64::from(days) * 86_400;
    Ok(sqlx::query("DELETE FROM client_events WHERE at < ?")
        .bind(cutoff)
        .execute(db)
        .await?
        .rows_affected())
}

/// The events since a time, by kind and client: how many, from how many users, and the latest.
#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub(crate) struct Group {
    pub(crate) kind: String,
    pub(crate) client: String,
    pub(crate) count: i64,
    pub(crate) users: i64,
    /// The latest event's time and message.
    pub(crate) last_at: i64,
    pub(crate) last_message: String,
}

/// Grouped by kind and client since `since`, the most frequent first. SQLite takes a bare
/// column (`message`) from the row that `MAX(at)` chose: the latest event's.
pub(crate) const GROUPS: &str = "SELECT kind, client, COUNT(*) AS count,
       COUNT(DISTINCT user_id) AS users, MAX(at) AS last_at, message AS last_message
     FROM client_events WHERE at >= ?
     GROUP BY kind, client ORDER BY count DESC, last_at DESC";

pub(crate) async fn groups(db: &SqlitePool, since: i64) -> sqlx::Result<Vec<Group>> {
    sqlx::query_as(GROUPS).bind(since).fetch_all(db).await
}

/// One stored event, with the title and system of its session's game where there was one.
#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub(crate) struct Stored {
    pub(crate) at: i64,
    pub(crate) client_at: Option<f64>,
    pub(crate) user_id: i64,
    pub(crate) client: String,
    pub(crate) kind: String,
    pub(crate) message: String,
    pub(crate) session_id: Option<String>,
    pub(crate) game_id: Option<i64>,
    pub(crate) title: Option<String>,
    pub(crate) system: Option<String>,
}

/// One group's events since `since`, newest first, at most `limit`.
pub(crate) const EVENTS: &str = "SELECT e.at, e.client_at, e.user_id, e.client, e.kind,
       e.message, e.session_id, e.game_id, g.title, g.system
     FROM client_events e LEFT JOIN games g ON g.id = e.game_id
     WHERE e.at >= ? AND e.kind = ? AND e.client = ?
     ORDER BY e.at DESC LIMIT ?";

pub(crate) async fn events(
    db: &SqlitePool,
    kind: Kind,
    client: &str,
    since: i64,
    limit: u32,
) -> sqlx::Result<Vec<Stored>> {
    sqlx::query_as(EVENTS)
        .bind(since)
        .bind(kind.as_str())
        .bind(client)
        .bind(limit)
        .fetch_all(db)
        .await
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
        let n = |user, at| events.report(user, "web", &batch, at).len();
        assert_eq!(n(1, t), 20);
        assert_eq!(n(1, t), 20);
        assert_eq!(n(1, t), 20);
        assert_eq!(n(1, t), 0, "sixty in the minute");
        assert_eq!(n(2, t), 20, "each user has their own");
        assert_eq!(n(1, t + Duration::from_secs(60)), 20);
    }

    #[test]
    fn a_batch_counts_no_more_than_its_cap() {
        let events = ClientEvents::default();
        let many: Vec<Event> = (0..50).map(|_| event(Kind::Other)).collect();
        assert_eq!(events.report(1, "web", &many, Instant::now()).len(), MAX_BATCH);
    }

    #[test]
    fn kinds_read_back_as_written() {
        for kind in Kind::ALL {
            assert_eq!(Kind::parse(kind.as_str()), kind);
        }
        assert_eq!(Kind::parse("a kind from a newer client"), Kind::Other);
    }

    #[tokio::test]
    async fn events_are_kept_grouped_read_back_and_pruned() {
        let dir = std::env::temp_dir().join(format!("sovemu-client-events-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = crate::db::open(&dir.join("events.db")).await.unwrap();
        let taken = ClientEvents::default().report(
            1,
            "android 1.4.0\n(Mi TV)",
            &[
                Event {
                    message: "ICE failed".into(),
                    session_id: Some("s1".into()),
                    ..event(Kind::ConnectionFailed)
                },
                Event { message: "ICE failed again".into(), ..event(Kind::ConnectionFailed) },
            ],
            Instant::now(),
        );
        let game_of = |s: &str| (s == "s1").then_some(7);
        store(&db, 1, "android 1.4.0\n(Mi TV)", &taken, 1_000, game_of).await.unwrap();
        store(&db, 2, "web", &taken[..1], 2_000, |_| None).await.unwrap();

        let found = groups(&db, 0).await.unwrap();
        assert_eq!(found.len(), 2);
        let android = found.iter().find(|g| g.client.starts_with("android")).unwrap();
        assert_eq!(android.client, "android 1.4.0 (Mi TV)", "cleaned as it is logged");
        assert_eq!((android.count, android.users, android.last_at), (2, 1, 1_000));
        assert!(groups(&db, 1_500).await.unwrap().iter().all(|g| g.client == "web"), "since");

        let rows = events(&db, Kind::ConnectionFailed, &android.client, 0, 10).await.unwrap();
        assert_eq!(rows.len(), 2);
        let in_game = rows.iter().find(|r| r.session_id.is_some()).unwrap();
        assert_eq!(
            (in_game.game_id, in_game.title.as_deref()),
            (Some(7), None),
            "no such game row"
        );

        assert_eq!(prune(&db, 1, 1_500 + 86_400).await.unwrap(), 2, "older than a day");
        assert_eq!(prune(&db, 0, i64::MAX).await.unwrap(), 0, "0: kept");
        assert_eq!(groups(&db, 0).await.unwrap().len(), 1);
    }

    #[test]
    fn text_cannot_forge_a_line_or_run_on() {
        assert_eq!(clean("one\ntwo\r\tthree", 100), "one two  three");
        assert_eq!(clean(&"x".repeat(1000), MAX_MESSAGE).len(), MAX_MESSAGE);
    }
}
