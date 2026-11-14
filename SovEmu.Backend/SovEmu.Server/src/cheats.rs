//! Cheat codes per user and game. Enabled codes are sent to the game's session whenever it
//! starts and whenever they change.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use sovemu_protocol::ipc::Request;

use crate::AppState;
use crate::auth::CurrentUser;
use crate::error::{ApiError, ApiResult};

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Cheat {
    id: i64,
    description: String,
    code: String,
    enabled: bool,
}

#[derive(Debug, Deserialize)]
pub struct NewCheat {
    #[serde(default)]
    description: String,
    code: String,
}

#[derive(Debug, Deserialize)]
pub struct CheatUpdate {
    enabled: bool,
}

/// The enabled codes for a user's game, in the order they were added.
pub async fn enabled_codes(
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

pub async fn list(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(game_id): Path<i64>,
) -> ApiResult<Json<Vec<Cheat>>> {
    Ok(Json(list_for(&state, user.id, game_id).await?))
}

pub async fn add(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(game_id): Path<i64>,
    Json(new): Json<NewCheat>,
) -> ApiResult<(StatusCode, Json<Vec<Cheat>>)> {
    let code = new.code.trim();
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
        .bind(new.description.trim())
        .bind(code)
        .execute(&state.db)
        .await?;
    push(&state, user.id, game_id).await?;
    Ok((StatusCode::CREATED, Json(list_for(&state, user.id, game_id).await?)))
}

pub async fn update(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path((game_id, cheat_id)): Path<(i64, i64)>,
    Json(update): Json<CheatUpdate>,
) -> ApiResult<Json<Vec<Cheat>>> {
    let changed =
        sqlx::query("UPDATE cheats SET enabled = ? WHERE id = ? AND user_id = ? AND game_id = ?")
            .bind(update.enabled)
            .bind(cheat_id)
            .bind(user.id)
            .bind(game_id)
            .execute(&state.db)
            .await?
            .rows_affected();
    if changed == 0 {
        return Err(ApiError::not_found("cheat"));
    }
    push(&state, user.id, game_id).await?;
    Ok(Json(list_for(&state, user.id, game_id).await?))
}

pub async fn remove(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path((game_id, cheat_id)): Path<(i64, i64)>,
) -> ApiResult<StatusCode> {
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
    push(&state, user.id, game_id).await?;
    Ok(StatusCode::NO_CONTENT)
}
