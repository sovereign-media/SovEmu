//! The library and session routes. Auth routes are in [`crate::auth`].

use std::path::PathBuf;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use sovemu_protocol::{SdpKind, SessionDescription};

use crate::AppState;
use crate::auth::{AdminUser, CurrentUser};
use crate::error::{ApiError, ApiResult};
use crate::library::{self, ScanReport};
use crate::systems::{self, System};
use crate::workers::{Launch, SessionInfo};

#[derive(Debug, Serialize)]
pub struct SystemSummary {
    #[serde(flatten)]
    system: System,
    game_count: i64,
    /// Whether a core that plays it is installed.
    playable: bool,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Game {
    id: i64,
    system: String,
    title: String,
    tags: String,
    path: String,
    size: i64,
    last_played_at: Option<String>,
    play_count: Option<i64>,
}

/// The installed core for a system, if any.
pub fn find_core(cores_dir: &std::path::Path, system: &System) -> Option<PathBuf> {
    system.cores.iter().find_map(|name| {
        ["so", "dylib", "dll"]
            .iter()
            .map(|ext| cores_dir.join(format!("{name}_libretro.{ext}")))
            .find(|p| p.is_file())
    })
}

pub async fn list_systems(
    State(state): State<AppState>,
    _user: CurrentUser,
) -> ApiResult<Json<Vec<SystemSummary>>> {
    let counts: Vec<(String, i64)> =
        sqlx::query_as("SELECT system, COUNT(*) FROM games GROUP BY system")
            .fetch_all(&state.db)
            .await?;
    let summaries = systems::SYSTEMS
        .iter()
        .map(|s| SystemSummary {
            system: *s,
            game_count: counts.iter().find(|(id, _)| id == s.id).map_or(0, |(_, n)| *n),
            playable: find_core(&state.config.cores_dir, s).is_some(),
        })
        .collect();
    Ok(Json(summaries))
}

const GAME_COLUMNS: &str = "g.id, g.system, g.title, g.tags, g.path, g.size,
     h.last_played_at, h.play_count";

pub async fn list_games(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(system): Path<String>,
) -> ApiResult<Json<Vec<Game>>> {
    systems::by_id(&system).ok_or_else(|| ApiError::not_found("system"))?;
    let games = sqlx::query_as::<_, Game>(&format!(
        "SELECT {GAME_COLUMNS} FROM games g
         LEFT JOIN play_history h ON h.game_id = g.id AND h.user_id = ?
         WHERE g.system = ? ORDER BY g.title COLLATE NOCASE, g.path"
    ))
    .bind(user.id)
    .bind(&system)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(games))
}

async fn fetch_game(state: &AppState, user_id: i64, id: i64) -> ApiResult<Game> {
    sqlx::query_as::<_, Game>(&format!(
        "SELECT {GAME_COLUMNS} FROM games g
         LEFT JOIN play_history h ON h.game_id = g.id AND h.user_id = ?
         WHERE g.id = ?"
    ))
    .bind(user_id)
    .bind(id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| ApiError::not_found("game"))
}

pub async fn get_game(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(id): Path<i64>,
) -> ApiResult<Json<Game>> {
    Ok(Json(fetch_game(&state, user.id, id).await?))
}

pub async fn scan(State(state): State<AppState>, _admin: AdminUser) -> ApiResult<Json<ScanReport>> {
    Ok(Json(library::scan(&state.db, &state.config.roms_dir).await?))
}

#[derive(Debug, Deserialize)]
pub struct StartSession {
    game_id: i64,
}

/// Starts a game. Ends the user's current session first.
pub async fn start_session(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Json(start): Json<StartSession>,
) -> ApiResult<(StatusCode, Json<SessionInfo>)> {
    let game = fetch_game(&state, user.id, start.game_id).await?;
    let system = systems::by_id(&game.system).ok_or_else(|| ApiError::not_found("system"))?;
    let core = find_core(&state.config.cores_dir, system).ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            format!("no core installed for {}", system.name),
        )
    })?;
    let rom = state.config.roms_dir.join(&game.path);
    if !rom.is_file() {
        return Err(ApiError::new(
            StatusCode::GONE,
            "the game file is missing; rescan the library",
        ));
    }
    let session = state
        .workers
        .start(Launch {
            user_id: user.id,
            game_id: game.id,
            system: game.system.clone(),
            title: game.title.clone(),
            core,
            rom,
            save_dir: state
                .config
                .data_dir
                .join("saves")
                .join(user.id.to_string())
                .join(&game.system),
        })
        .await
        .map_err(|e| ApiError::new(StatusCode::SERVICE_UNAVAILABLE, format!("{e:#}")))?;
    sqlx::query(
        "INSERT INTO play_history (user_id, game_id, last_played_at, play_count)
         VALUES (?, ?, datetime('now'), 1)
         ON CONFLICT(user_id, game_id) DO UPDATE SET
           last_played_at = excluded.last_played_at, play_count = play_count + 1",
    )
    .bind(user.id)
    .bind(game.id)
    .execute(&state.db)
    .await?;
    Ok((StatusCode::CREATED, Json(session.info())))
}

pub async fn current_session(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
) -> Json<Option<SessionInfo>> {
    Json(state.workers.for_user(user.id).map(|s| s.info()))
}

fn owned_session(
    state: &AppState,
    user_id: i64,
    id: &str,
) -> ApiResult<std::sync::Arc<crate::workers::Session>> {
    state
        .workers
        .get(id)
        .filter(|s| s.user_id == user_id)
        .ok_or_else(|| ApiError::not_found("session"))
}

pub async fn offer(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(id): Path<String>,
    Json(offer): Json<SessionDescription>,
) -> ApiResult<Json<SessionDescription>> {
    if offer.kind != SdpKind::Offer {
        return Err(ApiError::bad_request("expected an SDP offer"));
    }
    let session = owned_session(&state, user.id, &id)?;
    let answer = session.offer(offer).await.map_err(|e| ApiError::bad_request(format!("{e:#}")))?;
    Ok(Json(answer))
}

pub async fn reset(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    let session = owned_session(&state, user.id, &id)?;
    session
        .request(|id| sovemu_protocol::ipc::Request::Reset { id })
        .await
        .map_err(|e| ApiError::new(StatusCode::BAD_GATEWAY, format!("{e:#}")))?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn stop_session(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    owned_session(&state, user.id, &id)?;
    state.workers.stop(&id).await;
    Ok(StatusCode::NO_CONTENT)
}
