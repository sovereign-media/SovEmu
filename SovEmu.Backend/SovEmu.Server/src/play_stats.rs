//! Play statistics: every game played, kept as it ends (`play_sessions`, written by
//! [`PlayLog`] as `sovemu_sessions`' observer), and the admin Statistics page's figures over a
//! window: totals, plays per day, the most played games, each account's and each system's time,
//! and the most favourited games.
//!
//! Each figure is one query over the window by start time (`play_sessions_by_start`), so a page
//! costs what its window holds, never the whole history.

use sqlx::SqlitePool;

/// Keeps every game played, as its session ends.
#[derive(Debug, Clone)]
pub(crate) struct PlayLog {
    db: SqlitePool,
}

impl PlayLog {
    pub(crate) fn new(db: SqlitePool) -> Self {
        Self { db }
    }
}

impl sovemu_sessions::Observer for PlayLog {
    fn session_ended(&self, ended: &sovemu_sessions::Ended<'_>) {
        // A start that never loaded was never played (`failures.rs` keeps it).
        if !ended.loaded {
            return;
        }
        let now = crate::client_events::now_unix();
        let seconds = ended.ran.as_secs() as i64;
        let row = Played {
            user_id: ended.user_id,
            game_id: ended.game_id,
            title: ended.title.to_owned(),
            system: ended.system.to_owned(),
            core: ended.core.to_owned(),
            started_at: now - seconds,
            ended_at: now,
            end_reason: ended.reason.as_str(),
        };
        let db = self.db.clone();
        crate::db::write_in_background("play session", async move { row.insert(&db).await });
    }
}

/// One game played.
struct Played {
    user_id: i64,
    game_id: i64,
    title: String,
    system: String,
    core: String,
    started_at: i64,
    ended_at: i64,
    end_reason: &'static str,
}

impl Played {
    async fn insert(&self, db: &SqlitePool) -> sqlx::Result<()> {
        // The game may have left the library while it was played: its row keeps title and system.
        sqlx::query(
            "INSERT INTO play_sessions
               (user_id, game_id, title, system, core, started_at, ended_at, seconds, end_reason)
             VALUES (?, (SELECT id FROM games WHERE id = ?), ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(self.user_id)
        .bind(self.game_id)
        .bind(&self.title)
        .bind(&self.system)
        .bind(&self.core)
        .bind(self.started_at)
        .bind(self.ended_at)
        .bind(self.ended_at - self.started_at)
        .bind(self.end_reason)
        .execute(db)
        .await?;
        Ok(())
    }
}

/// The window's whole.
#[derive(Debug, Clone, Default, PartialEq, sqlx::FromRow)]
pub(crate) struct Totals {
    pub(crate) sessions: i64,
    pub(crate) seconds: i64,
    pub(crate) players: i64,
    pub(crate) games: i64,
}

/// One UTC day's plays.
#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub(crate) struct Day {
    /// The day's start, Unix seconds (UTC midnight).
    pub(crate) day: i64,
    pub(crate) sessions: i64,
    pub(crate) seconds: i64,
}

/// A game's plays in the window.
#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub(crate) struct Game {
    /// None for a game since gone from the library.
    pub(crate) game_id: Option<i64>,
    pub(crate) title: String,
    pub(crate) system: String,
    pub(crate) sessions: i64,
    pub(crate) seconds: i64,
    pub(crate) players: i64,
}

/// An account's plays in the window.
#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub(crate) struct Player {
    pub(crate) user_id: i64,
    pub(crate) sessions: i64,
    pub(crate) seconds: i64,
    pub(crate) games: i64,
}

/// A system's plays in the window.
#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub(crate) struct System {
    pub(crate) system: String,
    pub(crate) sessions: i64,
    pub(crate) seconds: i64,
    pub(crate) players: i64,
}

/// A game and how many accounts have it as a favourite.
#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub(crate) struct Favourite {
    pub(crate) game_id: i64,
    pub(crate) title: String,
    pub(crate) system: String,
    pub(crate) users: i64,
}

/// The figures of one window, `[from, to)` in Unix seconds.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Statistics {
    pub(crate) from: i64,
    pub(crate) to: i64,
    pub(crate) totals: Totals,
    /// Days with plays, oldest first.
    pub(crate) days: Vec<Day>,
    /// The most played first, by time.
    pub(crate) games: Vec<Game>,
    pub(crate) players: Vec<Player>,
    pub(crate) systems: Vec<System>,
    /// Of all time: favourites are not dated.
    pub(crate) favourites: Vec<Favourite>,
}

pub(crate) const TOTALS: &str = "SELECT COUNT(*) AS sessions, COALESCE(SUM(seconds), 0) AS seconds,
       COUNT(DISTINCT user_id) AS players, COUNT(DISTINCT title || char(31) || system) AS games
     FROM play_sessions WHERE started_at >= ?1 AND started_at < ?2";
pub(crate) const DAYS: &str = "SELECT started_at / 86400 * 86400 AS day, COUNT(*) AS sessions,
       SUM(seconds) AS seconds
     FROM play_sessions WHERE started_at >= ?1 AND started_at < ?2
     GROUP BY day ORDER BY day";
pub(crate) const GAMES: &str =
    "SELECT MAX(game_id) AS game_id, title, system, COUNT(*) AS sessions,
       SUM(seconds) AS seconds, COUNT(DISTINCT user_id) AS players
     FROM play_sessions WHERE started_at >= ?1 AND started_at < ?2
     GROUP BY title, system ORDER BY seconds DESC, sessions DESC LIMIT ?3";
pub(crate) const PLAYERS: &str = "SELECT user_id, COUNT(*) AS sessions, SUM(seconds) AS seconds,
       COUNT(DISTINCT title || char(31) || system) AS games
     FROM play_sessions WHERE started_at >= ?1 AND started_at < ?2
     GROUP BY user_id ORDER BY seconds DESC";
pub(crate) const SYSTEMS: &str = "SELECT system, COUNT(*) AS sessions, SUM(seconds) AS seconds,
       COUNT(DISTINCT user_id) AS players
     FROM play_sessions WHERE started_at >= ?1 AND started_at < ?2
     GROUP BY system ORDER BY seconds DESC";
pub(crate) const FAVOURITES: &str = "SELECT g.id AS game_id, g.title, g.system, COUNT(*) AS users
     FROM favourites f JOIN games g ON g.id = f.game_id
     GROUP BY g.id ORDER BY users DESC, g.title LIMIT ?1";

/// The figures for `[from, to)`, with the `top` games and favourites.
pub(crate) async fn statistics(
    db: &SqlitePool,
    from: i64,
    to: i64,
    top: u32,
) -> sqlx::Result<Statistics> {
    Ok(Statistics {
        from,
        to,
        totals: sqlx::query_as(TOTALS).bind(from).bind(to).fetch_one(db).await?,
        days: sqlx::query_as(DAYS).bind(from).bind(to).fetch_all(db).await?,
        games: sqlx::query_as(GAMES).bind(from).bind(to).bind(top).fetch_all(db).await?,
        players: sqlx::query_as(PLAYERS).bind(from).bind(to).fetch_all(db).await?,
        systems: sqlx::query_as(SYSTEMS).bind(from).bind(to).fetch_all(db).await?,
        favourites: sqlx::query_as(FAVOURITES).bind(top).fetch_all(db).await?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn db() -> SqlitePool {
        let dir = std::env::temp_dir().join(format!("sovemu-play-stats-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        crate::db::open(&dir.join("stats.db")).await.unwrap()
    }

    fn played(user_id: i64, title: &str, system: &str, day: i64, seconds: i64) -> Played {
        let started_at = day * 86_400 + 3600;
        Played {
            user_id,
            game_id: 0,
            title: title.into(),
            system: system.into(),
            core: "core".into(),
            started_at,
            ended_at: started_at + seconds,
            end_reason: "stopped",
        }
    }

    #[tokio::test]
    async fn figures_cover_the_window_only() {
        let db = db().await;
        for p in [
            played(1, "Tetris", "gb", 10, 600),
            played(1, "Tetris", "gb", 11, 1200),
            played(2, "Tetris", "gb", 11, 300),
            played(2, "Ridge Racer", "psx", 11, 900),
            played(1, "Old Game", "nes", 2, 5000),
        ] {
            p.insert(&db).await.unwrap();
        }
        let s = statistics(&db, 10 * 86_400, 12 * 86_400, 10).await.unwrap();
        assert_eq!(s.totals, Totals { sessions: 4, seconds: 3000, players: 2, games: 2 });
        assert_eq!(
            s.days,
            [
                Day { day: 10 * 86_400, sessions: 1, seconds: 600 },
                Day { day: 11 * 86_400, sessions: 3, seconds: 2400 }
            ]
        );
        assert_eq!(
            (s.games[0].title.as_str(), s.games[0].seconds, s.games[0].players),
            ("Tetris", 2100, 2)
        );
        assert_eq!(s.games[0].game_id, None, "no such game in the library");
        assert_eq!((s.players[0].user_id, s.players[0].seconds, s.players[0].games), (1, 1800, 1));
        assert_eq!(s.systems.iter().map(|s| s.system.as_str()).collect::<Vec<_>>(), ["gb", "psx"]);
        assert!(s.favourites.is_empty());

        let top = statistics(&db, 0, 20 * 86_400, 1).await.unwrap();
        assert_eq!(top.games.len(), 1, "only the top");
        assert_eq!(top.games[0].title, "Old Game");
    }
}
