//! The home screen's feed: a hero of featured games and rows of the player's recent games and
//! the library's newest, in one query (as Sovereign's home feed). Where the player left off
//! (the auto save) is each game's `autoSave` in the API.

use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::AppState;
use crate::api::{GAME_COLUMNS, Game};
use crate::auth::User;
use crate::error::ApiResult;

#[derive(Debug, Serialize)]
pub struct Home {
    /// The hero: games with artwork and a description first, a new pick each day.
    pub(crate) featured: Vec<Game>,
    /// This player's games, most recently played first.
    pub(crate) recent: Vec<Game>,
    /// The library's newest games.
    pub(crate) added: Vec<Game>,
    /// This player's favourites, by title.
    pub(crate) favourites: Vec<Game>,
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
    let sql = format!(
        "SELECT {GAME_COLUMNS} FROM games g
         LEFT JOIN play_history h ON h.game_id = g.id AND h.user_id = ?
         LEFT JOIN game_metadata m ON m.game_id = g.id
         WHERE ({filter}){hidden} ORDER BY {order} LIMIT ?"
    );
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
    Ok(games.into_iter().map(Game::with_box_art).collect())
}

pub async fn home(state: &AppState, user: &User) -> ApiResult<Home> {
    let row = state.config.home_row;
    let recent = query(
        state,
        user.id,
        "h.last_played_at IS NOT NULL",
        &[],
        "h.last_played_at DESC",
        row,
        None,
    )
    .await?;
    // Games with something to show first (artwork and a description), then a shuffle that
    // changes daily: a multiplicative hash of the id, offset by the day.
    let featured = query(
        state,
        user.id,
        "g.media != '' OR m.box_art IS NOT NULL",
        &[],
        "(m.description IS NOT NULL AND g.media LIKE '%image%') DESC,
         m.description IS NOT NULL DESC,
         ((g.id + ?) * 2654435761) % 4294967291",
        state.config.home_featured,
        Some(day()),
    )
    .await?;
    let added = query(state, user.id, "1", &[], "g.added_at DESC, g.id DESC", row, None).await?;
    let favourites = query(
        state,
        user.id,
        "g.id IN (SELECT game_id FROM favourites WHERE user_id = ?)",
        &[user.id],
        "COALESCE(m.title, g.title) COLLATE NOCASE, g.id",
        row,
        None,
    )
    .await?;
    Ok(Home { featured, recent, added, favourites })
}
