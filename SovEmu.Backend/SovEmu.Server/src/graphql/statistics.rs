//! Play statistics: what was played over a window, by game, account, system and day.

use super::*;

/// The window the figures cover, ending now.
#[derive(Enum, Clone, Copy, PartialEq, Eq)]
pub(super) enum StatsRange {
    Week,
    Month,
    Year,
}

impl StatsRange {
    fn days(self) -> i64 {
        match self {
            Self::Week => 7,
            Self::Month => 30,
            Self::Year => 365,
        }
    }
}

/// Plays and time played, totalled.
#[derive(SimpleObject)]
pub(super) struct PlayTotals {
    pub(super) sessions: i32,
    /// Seconds played (a Float: a year of a busy server can pass 2^31).
    pub(super) seconds: f64,
    pub(super) players: i32,
    pub(super) games: i32,
}

/// One UTC day's plays.
#[derive(SimpleObject)]
pub(super) struct PlayDay {
    /// The day's start (UTC midnight), Unix seconds.
    pub(super) day: f64,
    pub(super) sessions: i32,
    pub(super) seconds: f64,
}

/// A game's plays in the window.
#[derive(SimpleObject)]
pub(super) struct GamePlays {
    /// Null for a game since gone from the library.
    pub(super) game_id: Option<i64>,
    pub(super) title: String,
    pub(super) system: String,
    pub(super) sessions: i32,
    pub(super) seconds: f64,
    pub(super) players: i32,
}

/// An account's plays in the window.
#[derive(SimpleObject)]
pub(super) struct PlayerPlays {
    pub(super) user_id: i64,
    /// Null for an account since deleted.
    pub(super) username: Option<String>,
    pub(super) sessions: i32,
    pub(super) seconds: f64,
    pub(super) games: i32,
}

/// A system's plays in the window.
#[derive(SimpleObject)]
pub(super) struct SystemPlays {
    pub(super) system: String,
    pub(super) sessions: i32,
    pub(super) seconds: f64,
    pub(super) players: i32,
}

/// A game, and how many accounts keep it as a favourite.
#[derive(SimpleObject)]
pub(super) struct FavouriteGame {
    pub(super) game_id: i64,
    pub(super) title: String,
    pub(super) system: String,
    pub(super) users: i32,
}

/// What was played over a window.
#[derive(SimpleObject)]
pub(super) struct PlayStatistics {
    /// The window, Unix seconds: from its start up to now.
    pub(super) from: f64,
    pub(super) to: f64,
    pub(super) totals: PlayTotals,
    /// Days with plays, oldest first (days without are left out).
    pub(super) days: Vec<PlayDay>,
    /// The most played games first, by time played (the top 10).
    pub(super) games: Vec<GamePlays>,
    /// Every account that played, the most time first.
    pub(super) players: Vec<PlayerPlays>,
    /// Every system played, the most time first.
    pub(super) systems: Vec<SystemPlays>,
    /// The most favourited games, of all time (the top 10).
    pub(super) favourites: Vec<FavouriteGame>,
}

const TOP: u32 = 10;

#[derive(Default)]
pub(super) struct StatisticsQuery;

#[Object]
impl StatisticsQuery {
    /// Admins: what was played over the last week, month or year: totals, plays per day, the
    /// most played games, each account's and each system's time, and the most favourited games.
    async fn play_statistics(
        &self,
        ctx: &Context<'_>,
        range: StatsRange,
    ) -> Result<PlayStatistics> {
        admin(ctx).await?;
        let state = state(ctx);
        let to = crate::client_events::now_unix() + 1;
        let from = to - range.days() * 86_400;
        let s = crate::play_stats::statistics(&state.db, from, to, TOP)
            .await
            .map_err(ApiError::from)
            .api()?;
        let names = crate::api::usernames(state).await.api()?;
        Ok(PlayStatistics {
            from: s.from as f64,
            to: s.to as f64,
            totals: PlayTotals {
                sessions: s.totals.sessions as i32,
                seconds: s.totals.seconds as f64,
                players: s.totals.players as i32,
                games: s.totals.games as i32,
            },
            days: s
                .days
                .into_iter()
                .map(|d| PlayDay {
                    day: d.day as f64,
                    sessions: d.sessions as i32,
                    seconds: d.seconds as f64,
                })
                .collect(),
            games: s
                .games
                .into_iter()
                .map(|g| GamePlays {
                    game_id: g.game_id,
                    title: g.title,
                    system: g.system,
                    sessions: g.sessions as i32,
                    seconds: g.seconds as f64,
                    players: g.players as i32,
                })
                .collect(),
            players: s
                .players
                .into_iter()
                .map(|p| PlayerPlays {
                    username: names.get(&p.user_id).cloned(),
                    user_id: p.user_id,
                    sessions: p.sessions as i32,
                    seconds: p.seconds as f64,
                    games: p.games as i32,
                })
                .collect(),
            systems: s
                .systems
                .into_iter()
                .map(|y| SystemPlays {
                    system: y.system,
                    sessions: y.sessions as i32,
                    seconds: y.seconds as f64,
                    players: y.players as i32,
                })
                .collect(),
            favourites: s
                .favourites
                .into_iter()
                .map(|f| FavouriteGame {
                    game_id: f.game_id,
                    title: f.title,
                    system: f.system,
                    users: f.users as i32,
                })
                .collect(),
        })
    }
}
