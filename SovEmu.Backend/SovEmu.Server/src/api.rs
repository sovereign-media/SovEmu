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
    /// Artwork available through `/api/games/{id}/media/{kind}`.
    #[serde(serialize_with = "split_media")]
    media: String,
}

fn split_media<S: serde::Serializer>(media: &str, s: S) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeSeq;
    let kinds: Vec<&str> = media.split(',').filter(|k| !k.is_empty()).collect();
    let mut seq = s.serialize_seq(Some(kinds.len()))?;
    for k in kinds {
        seq.serialize_element(k)?;
    }
    seq.end()
}

/// Games larger than this (disc images) are cached only after the session that streamed them.
const LARGE_GAME_BYTES: i64 = 64 * 1024 * 1024;

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
     h.last_played_at, h.play_count, g.media";

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
    prefetch_art(&state, &system, &games);
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
    let original = state.config.roms_dir.join(&game.path);
    if !original.is_file() {
        return Err(ApiError::new(
            StatusCode::GONE,
            "the game file is missing; rescan the library",
        ));
    }
    let (save_dir, state_dir) = save_dirs(&state, user.id, &game);
    // A local copy when the library is on a slow share and the game has been played before.
    // Otherwise the original: small games are copied for next time straight away, large ones
    // (disc images) only once this session ends, so the copy never competes with the game.
    let cached = state.cache.cached("roms", &game.path, &original);
    let large = game.size > LARGE_GAME_BYTES;
    let rom = match &cached {
        Some(copy) => copy.clone(),
        None if large => original.clone(),
        None => state.cache.local_or_fetch("roms", &game.path, &original),
    };
    let after_exit: Option<Box<dyn FnOnce() + Send + Sync>> =
        (cached.is_none() && large).then(|| {
            let (cache, path, original) =
                (state.cache.clone(), game.path.clone(), original.clone());
            Box::new(move || {
                cache.local_or_fetch("roms", &path, &original);
            }) as Box<dyn FnOnce() + Send + Sync>
        });
    let session = state
        .workers
        .start(Launch {
            user_id: user.id,
            game_id: game.id,
            system: game.system.clone(),
            title: game.title.clone(),
            core,
            rom,
            save_dir,
            state_dir,
            variables: system
                .options
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
            after_exit,
        })
        .await
        .map_err(|e| ApiError::new(StatusCode::SERVICE_UNAVAILABLE, format!("{e:#}")))?;
    let codes = crate::cheats::enabled_codes(&state, user.id, game.id).await?;
    if !codes.is_empty()
        && let Err(e) =
            session.request(|id| sovemu_protocol::ipc::Request::SetCheats { id, codes }).await
    {
        tracing::warn!("could not apply cheats: {e:#}");
    }
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

/// Where a user's saves for a game live: battery saves in
/// `saves/<user>/<system>/<game>.srm`, states in `saves/<user>/<system>/states/<game>/`.
fn save_dirs(state: &AppState, user_id: i64, game: &Game) -> (PathBuf, PathBuf) {
    let save_dir = state.config.data_dir.join("saves").join(user_id.to_string()).join(&game.system);
    let stem = std::path::Path::new(&game.path)
        .file_stem()
        .map_or_else(|| game.id.to_string(), |s| s.to_string_lossy().into_owned());
    let state_dir = save_dir.join("states").join(stem);
    (save_dir, state_dir)
}

/// The highest save-state slot (slot 0 is the auto state).
const MAX_SLOT: u32 = 9;

#[derive(Debug, Serialize)]
pub struct SaveSlot {
    slot: u32,
    /// Unix seconds.
    saved_at: u64,
    screenshot: bool,
}

pub async fn list_states(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(id): Path<i64>,
) -> ApiResult<Json<Vec<SaveSlot>>> {
    let game = fetch_game(&state, user.id, id).await?;
    let (_, dir) = save_dirs(&state, user.id, &game);
    let slots = (0..=MAX_SLOT)
        .filter_map(|slot| {
            let meta = std::fs::metadata(dir.join(format!("slot{slot}.state"))).ok()?;
            let saved_at =
                meta.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
            Some(SaveSlot {
                slot,
                saved_at,
                screenshot: dir.join(format!("slot{slot}.png")).is_file(),
            })
        })
        .collect();
    Ok(Json(slots))
}

pub async fn state_screenshot(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path((id, slot)): Path<(i64, u32)>,
) -> ApiResult<axum::response::Response> {
    use axum::response::IntoResponse;
    let game = fetch_game(&state, user.id, id).await?;
    let (_, dir) = save_dirs(&state, user.id, &game);
    let bytes = tokio::fs::read(dir.join(format!("slot{slot}.png")))
        .await
        .map_err(|_| ApiError::not_found("screenshot"))?;
    Ok(([(axum::http::header::CONTENT_TYPE, "image/png")], bytes).into_response())
}

pub async fn delete_state(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path((id, slot)): Path<(i64, u32)>,
) -> ApiResult<StatusCode> {
    let game = fetch_game(&state, user.id, id).await?;
    let (_, dir) = save_dirs(&state, user.id, &game);
    let state_file = dir.join(format!("slot{slot}.state"));
    if !state_file.is_file() {
        return Err(ApiError::not_found("save state"));
    }
    let _ = tokio::fs::remove_file(&state_file).await;
    let _ = tokio::fs::remove_file(dir.join(format!("slot{slot}.png"))).await;
    Ok(StatusCode::NO_CONTENT)
}

async fn session_request(
    state: &AppState,
    user_id: i64,
    id: &str,
    make: impl FnOnce(u64) -> sovemu_protocol::ipc::Request,
) -> ApiResult<StatusCode> {
    let session = owned_session(state, user_id, id)?;
    session.request(make).await.map_err(|e| ApiError::bad_request(format!("{e:#}")))?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn save_state(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path((id, slot)): Path<(String, u32)>,
) -> ApiResult<StatusCode> {
    session_request(&state, user.id, &id, |rid| sovemu_protocol::ipc::Request::SaveState {
        id: rid,
        slot,
    })
    .await
}

pub async fn load_state(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path((id, slot)): Path<(String, u32)>,
) -> ApiResult<StatusCode> {
    session_request(&state, user.id, &id, |rid| sovemu_protocol::ipc::Request::LoadState {
        id: rid,
        slot,
    })
    .await
}

/// A game's artwork or video (`image`, `thumb`, `marquee`, `video`), served with range support
/// so browsers can stream the video.
pub async fn game_media(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path((id, kind)): Path<(i64, String)>,
    request: axum::extract::Request,
) -> ApiResult<axum::response::Response> {
    use axum::response::IntoResponse;
    use tower::ServiceExt;
    if !library::MEDIA_KINDS.contains(&kind.as_str()) {
        return Err(ApiError::not_found("media"));
    }
    let game = fetch_game(&state, user.id, id).await?;
    let relative =
        media_relative(&state, &game, &kind).ok_or_else(|| ApiError::not_found("media"))?;
    let original = state.config.roms_dir.join(&relative);
    let file = state.cache.local_or_fetch("media", &relative, &original);
    let mut response = tower_http::services::ServeFile::new(file)
        .oneshot(request)
        .await
        .map_err(ApiError::internal)?
        .into_response();
    // Artwork changes only on a rescrape; let browsers keep it a day.
    response.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("private, max-age=86400"),
    );
    Ok(response)
}

/// Where a game's artwork of `kind` is, relative to the library root, if it exists.
fn media_relative(state: &AppState, game: &Game, kind: &str) -> Option<String> {
    let rom = std::path::Path::new(&game.path);
    let stem = rom.file_stem()?.to_string_lossy().into_owned();
    let folder = rom.parent().unwrap_or(std::path::Path::new(""));
    library::media_candidates(&stem, kind)
        .into_iter()
        .map(|c| folder.join(c).to_string_lossy().replace('\\', "/"))
        .find(|rel| state.config.roms_dir.join(rel).is_file())
}

/// Opening a system warms the cache with its games' stills (not videos), in the background.
fn prefetch_art(state: &AppState, system: &str, games: &[Game]) {
    if !state.cache.enabled() {
        return;
    }
    let state2 = state.clone();
    let games: Vec<(String, String)> =
        games.iter().map(|g| (g.path.clone(), g.media.clone())).collect();
    let system = system.to_owned();
    tokio::task::spawn_blocking(move || {
        let mut items = Vec::new();
        for (path, media) in games {
            let rom = std::path::Path::new(&path);
            let Some(stem) = rom.file_stem().map(|s| s.to_string_lossy().into_owned()) else {
                continue;
            };
            let folder = rom.parent().unwrap_or(std::path::Path::new("")).to_path_buf();
            for kind in ["thumb", "image"] {
                if !media.split(',').any(|m| m == kind) {
                    continue;
                }
                // Media file names come from the scan; the first candidate that exists wins.
                for candidate in library::media_candidates(&stem, kind) {
                    let rel = folder.join(&candidate).to_string_lossy().replace('\\', "/");
                    let original = state2.config.roms_dir.join(&rel);
                    if original.is_file() {
                        items.push(("media", rel, original));
                        break;
                    }
                }
            }
        }
        state2.cache.prefetch_many(format!("{system} artwork"), items);
    });
}
