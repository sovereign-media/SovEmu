//! A user's favourite games and named collections of games.

use std::collections::HashSet;

use serde::Serialize;

use crate::AppState;
use crate::api::{GAME_COLUMNS, Game};
use crate::auth::User;
use crate::error::{ApiError, ApiResult};

/// Longest collection name.
const MAX_NAME: usize = 64;
/// Collections a user may keep.
const MAX_COLLECTIONS: i64 = 100;

/// A named list of games.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub(crate) struct Collection {
    pub id: i64,
    pub name: String,
    pub game_count: i64,
}

/// A user's favourites: read for every list of games.
pub(crate) const FAVOURITE_IDS: &str = "SELECT game_id FROM favourites WHERE user_id = ?";
/// The user's collections that hold a game: read when its panel opens.
pub(crate) const OF_GAME: &str = "SELECT c.id FROM collections c
     JOIN collection_games cg ON cg.collection_id = c.id
     WHERE c.user_id = ? AND cg.game_id = ? ORDER BY c.name COLLATE NOCASE";
/// A collection's games, most recently added first (binds: the user's id, the collection's).
pub(crate) fn collection_games_sql() -> String {
    format!(
        "SELECT {GAME_COLUMNS} FROM collection_games cg
         JOIN games g ON g.id = cg.game_id
         LEFT JOIN play_history h ON h.game_id = g.id AND h.user_id = ?
         LEFT JOIN game_metadata m ON m.game_id = g.id
         WHERE cg.collection_id = ? ORDER BY cg.added_at DESC, g.id DESC"
    )
}

/// Marks the user's favourites among `games`.
pub(crate) async fn mark_favourites(
    state: &AppState,
    user_id: i64,
    games: &mut [Game],
) -> ApiResult<()> {
    if games.is_empty() {
        return Ok(());
    }
    let ids: HashSet<i64> = sqlx::query_scalar(FAVOURITE_IDS)
        .bind(user_id)
        .fetch_all(&state.db)
        .await?
        .into_iter()
        .collect();
    for g in games {
        g.favourite = ids.contains(&g.id);
    }
    Ok(())
}

/// Adds the game to the user's favourites, or takes it out.
pub(crate) async fn set_favourite(
    state: &AppState,
    user: &User,
    game_id: i64,
    favourite: bool,
) -> ApiResult<()> {
    crate::api::require_game(state, game_id).await?;
    let sql = if favourite {
        "INSERT OR IGNORE INTO favourites (user_id, game_id) VALUES (?, ?)"
    } else {
        "DELETE FROM favourites WHERE user_id = ? AND game_id = ?"
    };
    sqlx::query(sql).bind(user.id).bind(game_id).execute(&state.db).await?;
    Ok(())
}

/// The user's collections, by name.
pub(crate) async fn list(state: &AppState, user: &User) -> ApiResult<Vec<Collection>> {
    Ok(sqlx::query_as::<_, Collection>(
        "SELECT c.id, c.name, COUNT(cg.game_id) AS game_count FROM collections c
         LEFT JOIN collection_games cg ON cg.collection_id = c.id
         WHERE c.user_id = ? GROUP BY c.id ORDER BY c.name COLLATE NOCASE",
    )
    .bind(user.id)
    .fetch_all(&state.db)
    .await?)
}

/// A collection's games, most recently added first.
pub(crate) async fn games(
    state: &AppState,
    user: &User,
    collection_id: i64,
) -> ApiResult<Vec<Game>> {
    owned(state, user, collection_id).await?;
    let mut games = sqlx::query_as::<_, Game>(&collection_games_sql())
        .bind(user.id)
        .bind(collection_id)
        .fetch_all(&state.db)
        .await?;
    // A hidden system's games are not offered, in a collection as anywhere.
    games.retain(|g| crate::api::enabled(state, &g.system));
    crate::api::annotate(state, user.id, &mut games).await?;
    Ok(games.into_iter().map(Game::with_box_art).collect())
}

/// The ids of the user's collections that hold `game_id`.
pub(crate) async fn of_game(state: &AppState, user_id: i64, game_id: i64) -> ApiResult<Vec<i64>> {
    Ok(sqlx::query_scalar(OF_GAME).bind(user_id).bind(game_id).fetch_all(&state.db).await?)
}

/// Creates a collection; a name the user already has is refused.
pub(crate) async fn create(state: &AppState, user: &User, name: &str) -> ApiResult<Collection> {
    let name = valid_name(name)?;
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM collections WHERE user_id = ?")
        .bind(user.id)
        .fetch_one(&state.db)
        .await?;
    if count >= MAX_COLLECTIONS {
        return Err(ApiError::bad_request(format!("up to {MAX_COLLECTIONS} collections")));
    }
    let id = sqlx::query("INSERT INTO collections (user_id, name) VALUES (?, ?)")
        .bind(user.id)
        .bind(&name)
        .execute(&state.db)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(d) if d.is_unique_violation() => {
                ApiError::conflict(format!("you already have a collection called {name}"))
            }
            e => e.into(),
        })?
        .last_insert_rowid();
    Ok(Collection { id, name, game_count: 0 })
}

pub(crate) async fn rename(
    state: &AppState,
    user: &User,
    collection_id: i64,
    name: &str,
) -> ApiResult<()> {
    owned(state, user, collection_id).await?;
    let name = valid_name(name)?;
    sqlx::query("UPDATE collections SET name = ? WHERE id = ?")
        .bind(&name)
        .bind(collection_id)
        .execute(&state.db)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(d) if d.is_unique_violation() => {
                ApiError::conflict(format!("you already have a collection called {name}"))
            }
            e => e.into(),
        })?;
    Ok(())
}

pub(crate) async fn delete(state: &AppState, user: &User, collection_id: i64) -> ApiResult<()> {
    owned(state, user, collection_id).await?;
    sqlx::query("DELETE FROM collections WHERE id = ?")
        .bind(collection_id)
        .execute(&state.db)
        .await?;
    Ok(())
}

/// Puts the game in the collection, or takes it out.
pub(crate) async fn set_member(
    state: &AppState,
    user: &User,
    collection_id: i64,
    game_id: i64,
    member: bool,
) -> ApiResult<()> {
    owned(state, user, collection_id).await?;
    crate::api::require_game(state, game_id).await?;
    let sql = if member {
        "INSERT OR IGNORE INTO collection_games (collection_id, game_id) VALUES (?, ?)"
    } else {
        "DELETE FROM collection_games WHERE collection_id = ? AND game_id = ?"
    };
    sqlx::query(sql).bind(collection_id).bind(game_id).execute(&state.db).await?;
    Ok(())
}

fn valid_name(name: &str) -> ApiResult<String> {
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() || name.chars().count() > MAX_NAME {
        return Err(ApiError::bad_request(format!(
            "a collection's name has 1 to {MAX_NAME} characters"
        )));
    }
    Ok(name)
}

/// Another user's collection is as good as missing.
async fn owned(state: &AppState, user: &User, collection_id: i64) -> ApiResult<()> {
    let found: Option<i64> =
        sqlx::query_scalar("SELECT id FROM collections WHERE id = ? AND user_id = ?")
            .bind(collection_id)
            .bind(user.id)
            .fetch_optional(&state.db)
            .await?;
    found.map(|_| ()).ok_or_else(|| ApiError::not_found("collection"))
}
