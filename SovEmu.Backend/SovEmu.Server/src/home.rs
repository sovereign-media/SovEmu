//! The home screen's feed: a hero of featured games and rows of the player's recent games and
//! the library's newest, in one query (as Sovereign's home feed). Where the player left off
//! (the auto save) is each game's `autoSave` in the API.

use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::AppState;
use crate::api::Game;
use crate::auth::User;
use crate::error::ApiResult;

#[derive(Debug, Serialize)]
pub(crate) struct Home {
    /// The hero: games with artwork and a description first, a new pick each day.
    pub(crate) featured: Vec<Game>,
    /// This player's games, most recently played first.
    pub(crate) recent: Vec<Game>,
    /// The library's newest games.
    pub(crate) added: Vec<Game>,
    /// This player's favourites, by title.
    pub(crate) favourites: Vec<Game>,
    /// The library's best rated games (ScreenScraper's players' ratings).
    pub(crate) top_rated: Vec<Game>,
}

/// `AND g.system NOT IN (…)` for the systems the configuration hides, and the ids to bind.
pub(crate) fn hidden_systems(state: &AppState) -> (String, Vec<String>) {
    let hidden: Vec<String> = state
        .config
        .systems
        .iter()
        .filter(|(_, s)| s.enabled == Some(false))
        .map(|(id, _)| id.clone())
        .collect();
    if hidden.is_empty() {
        return (String::new(), hidden);
    }
    (format!(" AND g.system NOT IN ({})", vec!["?"; hidden.len()].join(", ")), hidden)
}

/// Today, as a number that changes once a day: the featured games stay put for a day.
fn day() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| (d.as_secs() / 86_400) as i64)
}

/// One of Home's rows: which games, in what order (its SQL is [`row_sql`]).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Row<'a> {
    pub filter: &'a str,
    pub order: &'a str,
}

/// This player's games, most recently played first.
pub(crate) const RECENT: Row =
    Row { filter: "h.last_played_at IS NOT NULL", order: "h.last_played_at DESC" };
/// Games with something to show first (artwork and a description), then a shuffle that changes
/// daily: a multiplicative hash of the id, offset by the day (bound). It reads every game with
/// artwork, by design: it is the only way to shuffle them.
pub(crate) const FEATURED: Row = Row {
    filter: "m.box_art IS NOT NULL OR m.screenshot IS NOT NULL",
    order: "(m.description IS NOT NULL AND m.screenshot IS NOT NULL) DESC,
         m.description IS NOT NULL DESC,
         ((g.id + ?) * 2654435761) % 4294967291",
};
/// The best rated games, then by title (read through the `game_metadata_rating` index).
pub(crate) const TOP_RATED: Row = Row {
    filter: "m.rating IS NOT NULL",
    order: "m.rating DESC, COALESCE(m.title, g.title) COLLATE NOCASE",
};
/// The library's newest games (read through the `games_added` index).
pub(crate) const ADDED: Row = Row { filter: "1", order: "g.added_at DESC, g.id DESC" };
/// This player's favourites, by title (binds the user's id).
pub(crate) const FAVOURITES: Row = Row {
    filter: "g.id IN (SELECT game_id FROM favourites WHERE user_id = ?)",
    order: "COALESCE(m.title, g.title) COLLATE NOCASE, g.id",
};

/// A row's query; `hidden` is [`hidden_systems`]'s condition.
pub(crate) fn row_sql(row: Row, hidden: &str) -> String {
    let Row { filter, order } = row;
    crate::api::games_sql(&format!("WHERE ({filter}){hidden} ORDER BY {order} LIMIT ?"))
}

pub(crate) async fn query(
    state: &AppState,
    user_id: i64,
    filter: &str,
    filter_binds: &[i64],
    order: &str,
    limit: u32,
    seed: Option<i64>,
) -> ApiResult<Vec<Game>> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let (hidden, hidden_ids) = hidden_systems(state);
    let sql = row_sql(Row { filter, order }, &hidden);
    let mut q = sqlx::query_as::<_, Game>(&sql).bind(user_id);
    // Placeholders in the order they appear: the join, the filter, its hidden systems, the order.
    for value in filter_binds {
        q = q.bind(*value);
    }
    for id in &hidden_ids {
        q = q.bind(id);
    }
    if let Some(seed) = seed {
        q = q.bind(seed);
    }
    let mut games = q.bind(limit).fetch_all(&state.db).await?;
    crate::api::annotate(state, user_id, &mut games).await?;
    Ok(games)
}

pub(crate) async fn home(state: &AppState, user: &User) -> ApiResult<Home> {
    let row = state.config.home_row;
    let recent = query(state, user.id, RECENT.filter, &[], RECENT.order, row, None).await?;
    let featured = query(
        state,
        user.id,
        FEATURED.filter,
        &[],
        FEATURED.order,
        state.config.home_featured,
        Some(day()),
    )
    .await?;
    let added = query(state, user.id, ADDED.filter, &[], ADDED.order, row, None).await?;
    let favourites =
        query(state, user.id, FAVOURITES.filter, &[user.id], FAVOURITES.order, row, None).await?;
    let top_rated =
        query(state, user.id, TOP_RATED.filter, &[], TOP_RATED.order, row, None).await?;
    Ok(Home { featured, recent, added, favourites, top_rated })
}
