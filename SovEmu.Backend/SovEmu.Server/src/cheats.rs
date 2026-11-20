//! Cheat codes per user and game. Enabled codes are sent to the game's session whenever it
//! starts and whenever they change.

use axum::http::StatusCode;
use serde::Serialize;
use sovemu_protocol::ipc::Request;

use crate::AppState;
use crate::auth::User;
use crate::error::{ApiError, ApiResult};

/// A cheat code for a game, this user's.
#[derive(Debug, Serialize, sqlx::FromRow, async_graphql::SimpleObject)]
pub(crate) struct Cheat {
    pub id: i64,
    pub description: String,
    pub code: String,
    pub enabled: bool,
}

/// The enabled codes for a user's game, in the order they were added.
pub(crate) async fn enabled_codes(
    state: &AppState,
    user_id: i64,
    game_id: i64,
) -> Result<Vec<String>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT code FROM cheats WHERE user_id = ? AND game_id = ? AND enabled ORDER BY id",
    )
    .bind(user_id)
    .bind(game_id)
    .fetch_all(&state.db)
    .await
}

/// Sends the current codes to the user's session, if it is playing this game.
async fn push(state: &AppState, user_id: i64, game_id: i64) -> ApiResult<()> {
    let Some(session) = state.workers.for_user(user_id).filter(|s| s.info().game_id == game_id)
    else {
        return Ok(());
    };
    let codes = enabled_codes(state, user_id, game_id).await?;
    session
        .request(|id| Request::SetCheats { id, codes })
        .await
        .map_err(|e| ApiError::new(StatusCode::BAD_GATEWAY, format!("{e:#}")))?;
    Ok(())
}

async fn list_for(state: &AppState, user_id: i64, game_id: i64) -> ApiResult<Vec<Cheat>> {
    Ok(sqlx::query_as::<_, Cheat>(
        "SELECT id, description, code, enabled FROM cheats WHERE user_id = ? AND game_id = ? ORDER BY id",
    )
    .bind(user_id)
    .bind(game_id)
    .fetch_all(&state.db)
    .await?)
}

pub(crate) async fn list(state: &AppState, user: &User, game_id: i64) -> ApiResult<Vec<Cheat>> {
    list_for(state, user.id, game_id).await
}

/// Adds a cheat (enabled) and returns the game's cheats.
pub(crate) async fn add(
    state: &AppState,
    user: &User,
    game_id: i64,
    code: &str,
    description: &str,
) -> ApiResult<Vec<Cheat>> {
    let code = code.trim();
    if code.is_empty() || code.len() > 256 {
        return Err(ApiError::bad_request("a cheat needs a code (up to 256 characters)"));
    }
    let exists: Option<i64> = sqlx::query_scalar("SELECT id FROM games WHERE id = ?")
        .bind(game_id)
        .fetch_optional(&state.db)
        .await?;
    exists.ok_or_else(|| ApiError::not_found("game"))?;
    sqlx::query("INSERT INTO cheats (user_id, game_id, description, code) VALUES (?, ?, ?, ?)")
        .bind(user.id)
        .bind(game_id)
        .bind(description.trim())
        .bind(code)
        .execute(&state.db)
        .await?;
    push(state, user.id, game_id).await?;
    list_for(state, user.id, game_id).await
}

/// Turns a cheat on or off and returns the game's cheats.
pub(crate) async fn set_enabled(
    state: &AppState,
    user: &User,
    game_id: i64,
    cheat_id: i64,
    enabled: bool,
) -> ApiResult<Vec<Cheat>> {
    let changed =
        sqlx::query("UPDATE cheats SET enabled = ? WHERE id = ? AND user_id = ? AND game_id = ?")
            .bind(enabled)
            .bind(cheat_id)
            .bind(user.id)
            .bind(game_id)
            .execute(&state.db)
            .await?
            .rows_affected();
    if changed == 0 {
        return Err(ApiError::not_found("cheat"));
    }
    push(state, user.id, game_id).await?;
    list_for(state, user.id, game_id).await
}

pub(crate) async fn remove(
    state: &AppState,
    user: &User,
    game_id: i64,
    cheat_id: i64,
) -> ApiResult<()> {
    let removed = sqlx::query("DELETE FROM cheats WHERE id = ? AND user_id = ? AND game_id = ?")
        .bind(cheat_id)
        .bind(user.id)
        .bind(game_id)
        .execute(&state.db)
        .await?
        .rows_affected();
    if removed == 0 {
        return Err(ApiError::not_found("cheat"));
    }
    push(state, user.id, game_id).await?;
    Ok(())
}
