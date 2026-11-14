//! The library and session routes. Auth routes are in [`crate::auth`].

use std::path::PathBuf;
use std::time::Instant;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::Serialize;
use sovemu_protocol::{SdpKind, SessionDescription};

use crate::AppState;
use crate::auth::{CurrentUser, User};
use crate::error::{ApiError, ApiResult};
use crate::library::{self, ScanReport};
use crate::systems::{self, System};
use crate::workers::{Launch, SessionInfo};

#[derive(Debug, Serialize)]
pub struct SystemSummary {
    #[serde(flatten)]
    pub(crate) system: System,
    pub(crate) game_count: i64,
    /// Whether a core that plays it is installed (and, for a 3D system, it has a renderer).
    pub(crate) playable: bool,
    /// For a 3D system: `gpu`, `software` (slow) or `off` (unplayable here).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) rendering: Option<crate::gpu::Mode>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Game {
    pub(crate) id: i64,
    pub(crate) system: String,
    pub(crate) title: String,
    pub(crate) tags: String,
    pub(crate) path: String,
    pub(crate) size: i64,
    pub(crate) last_played_at: Option<String>,
    pub(crate) play_count: Option<i64>,
    /// Artwork available through `/api/games/{id}/media/{kind}`.
    #[serde(serialize_with = "split_media")]
    pub(crate) media: String,
    /// The title from the file name, when metadata supplied a better `title`.
    pub(crate) file_title: Option<String>,
    pub(crate) description: Option<String>,
    pub(crate) release_date: Option<String>,
    pub(crate) developer: Option<String>,
    pub(crate) publisher: Option<String>,
    pub(crate) genre: Option<String>,
    pub(crate) players: Option<String>,
    /// 0 to 1.
    pub(crate) rating: Option<f64>,
    /// Downloaded box art (a file under `<data>/metadata/media`), served as `thumb`.
    #[serde(skip)]
    pub(crate) box_art: Option<String>,
}

impl Game {
    /// Downloaded box art stands in for a missing local `thumb`.
    pub(crate) fn with_box_art(mut self) -> Self {
        if self.box_art.is_some() && !self.media.split(',').any(|k| k == "thumb") {
            self.media = if self.media.is_empty() {
                "thumb".into()
            } else {
                format!("{},thumb", self.media)
            };
        }
        self
    }
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

/// The installed core for a system, if any: the one `SOVEMU_SYSTEMS__<id>__CORE` names, or
/// else the first of SovEmu's choices that is installed.
pub fn find_core(state: &AppState, system: &System) -> Option<PathBuf> {
    let chosen = state.config.systems.get(system.id).and_then(|s| s.core.as_deref());
    let names: Vec<&str> = chosen.map_or_else(|| system.cores.to_vec(), |c| vec![c]);
    names.iter().find_map(|name| {
        ["so", "dylib", "dll"]
            .iter()
            .map(|ext| state.config.cores_dir.join(format!("{name}_libretro.{ext}")))
            .find(|p| p.is_file())
    })
}

/// Whether a system is offered (`SOVEMU_SYSTEMS__<id>__ENABLED=false` hides it).
fn enabled(state: &AppState, id: &str) -> bool {
    state.config.systems.get(id).and_then(|s| s.enabled).unwrap_or(true)
}

/// The system, if it exists and is enabled.
fn system_by_id(state: &AppState, id: &str) -> ApiResult<&'static System> {
    systems::by_id(id).filter(|s| enabled(state, s.id)).ok_or_else(|| ApiError::not_found("system"))
}

pub async fn list_systems(state: &AppState) -> ApiResult<Vec<SystemSummary>> {
    let counts: Vec<(String, i64)> =
        sqlx::query_as("SELECT system, COUNT(*) FROM games GROUP BY system")
            .fetch_all(&state.db)
            .await?;
    let summaries = systems::SYSTEMS
        .iter()
        .filter(|s| enabled(state, s.id))
        .map(|s| SystemSummary {
            system: *s,
            game_count: counts.iter().find(|(id, _)| id == s.id).map_or(0, |(_, n)| *n),
            playable: find_core(state, s).is_some()
                && (!s.needs_gpu || state.gpu.mode != crate::gpu::Mode::Off),
            rendering: s.needs_gpu.then_some(state.gpu.mode),
        })
        .collect();
    Ok(summaries)
}

pub(crate) const GAME_COLUMNS: &str =
    "g.id, g.system, COALESCE(m.title, g.title) AS title, g.tags, g.path,
     g.size, h.last_played_at, h.play_count, g.media,
     CASE WHEN m.title IS NOT NULL AND m.title != g.title THEN g.title END AS file_title,
     m.description, m.release_date, m.developer, m.publisher, m.genre, m.players, m.rating,
     m.box_art";

pub async fn list_games(state: &AppState, user: &User, system: &str) -> ApiResult<Vec<Game>> {
    system_by_id(state, system)?;
    let games = sqlx::query_as::<_, Game>(&format!(
        "SELECT {GAME_COLUMNS} FROM games g
         LEFT JOIN play_history h ON h.game_id = g.id AND h.user_id = ?
         LEFT JOIN game_metadata m ON m.game_id = g.id
         WHERE g.system = ? ORDER BY COALESCE(m.title, g.title) COLLATE NOCASE, g.path"
    ))
    .bind(user.id)
    .bind(system)
    .fetch_all(&state.db)
    .await?;
    prefetch_art(state, system, &games);
    Ok(games.into_iter().map(Game::with_box_art).collect())
}

async fn fetch_game(state: &AppState, user_id: i64, id: i64) -> ApiResult<Game> {
    sqlx::query_as::<_, Game>(&format!(
        "SELECT {GAME_COLUMNS} FROM games g
         LEFT JOIN play_history h ON h.game_id = g.id AND h.user_id = ?
         LEFT JOIN game_metadata m ON m.game_id = g.id
         WHERE g.id = ?"
    ))
    .bind(user_id)
    .bind(id)
    .fetch_optional(&state.db)
    .await?
    .map(Game::with_box_art)
    .ok_or_else(|| ApiError::not_found("game"))
}

pub async fn get_game(state: &AppState, user: &User, id: i64) -> ApiResult<Game> {
    fetch_game(state, user.id, id).await
}

pub async fn scan(state: &AppState) -> ApiResult<ScanReport> {
    Ok(library::scan(&state.db, &state.config.roms_dir).await?)
}

/// Starts a game. Ends the user's current session first. `start_id`, chosen by the client, lets
/// it follow the start's progress ([`start_progress`]) while this waits.
pub async fn start_session(
    state: &AppState,
    user: &User,
    game_id: i64,
    start_id: Option<&str>,
) -> ApiResult<SessionInfo> {
    let start_id = match start_id {
        Some(id) if !crate::starts::valid_id(id) => {
            return Err(ApiError::bad_request("start_id: 8 to 64 letters, digits, - or _"));
        }
        id => id.map(str::to_owned),
    };
    let starts = &state.workers.starts;
    if let Some(id) = &start_id {
        starts.begin(id, user.id, Instant::now());
    }
    let result = launch(state, user, game_id, start_id.clone()).await;
    if let Some(id) = &start_id {
        starts.finish(id, result.as_ref().err().map(|e| e.message.clone()), Instant::now());
    }
    result
}

/// How far the start `start_id` has got (see [`crate::starts`]).
pub fn start_progress(
    state: &AppState,
    user: &User,
    start_id: &str,
) -> ApiResult<crate::starts::StartStatus> {
    state
        .workers
        .starts
        .get(start_id, user.id, Instant::now())
        .ok_or_else(|| ApiError::not_found("start"))
}

async fn launch(
    state: &AppState,
    user: &User,
    game_id: i64,
    start_id: Option<String>,
) -> ApiResult<SessionInfo> {
    let state = state.clone();
    let game = fetch_game(&state, user.id, game_id).await?;
    let system = system_by_id(&state, &game.system)?;
    if system.needs_gpu && state.gpu.mode == crate::gpu::Mode::Off {
        return Err(ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            format!(
                "{} needs 3D rendering, which is unavailable on this server ({})",
                system.name,
                state.gpu.note.as_deref().unwrap_or("SOVEMU_GPU")
            ),
        ));
    }
    let core = find_core(&state, system).ok_or_else(|| {
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
    let large = game.size.max(0).unsigned_abs() > state.config.cache_large_game_mb << 20;
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
            core: core.clone(),
            rom,
            save_dir,
            state_dir,
            variables: crate::settings::for_launch(&state, system).await?,
            after_exit,
            start_id,
        })
        .await
        .map_err(|e| ApiError::new(StatusCode::SERVICE_UNAVAILABLE, format!("{e:#}")))?;
    if let Some(declared) = session.take_declared() {
        crate::settings::remember(&state.db, &core, &declared).await;
    }
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
    Ok(session.info())
}

pub fn current_session(state: &AppState, user: &User) -> Option<SessionInfo> {
    state.workers.for_user(user.id).map(|s| s.info())
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

/// A client's SDP offer for the session; returns the worker's answer.
pub async fn offer(state: &AppState, user: &User, id: &str, sdp: String) -> ApiResult<String> {
    let session = owned_session(state, user.id, id)?;
    let answer = session
        .offer(SessionDescription { kind: SdpKind::Offer, sdp })
        .await
        .map_err(|e| ApiError::bad_request(format!("{e:#}")))?;
    Ok(answer.sdp)
}

pub async fn reset(state: &AppState, user: &User, id: &str) -> ApiResult<()> {
    let session = owned_session(state, user.id, id)?;
    session
        .request(|id| sovemu_protocol::ipc::Request::Reset { id })
        .await
        .map_err(|e| ApiError::new(StatusCode::BAD_GATEWAY, format!("{e:#}")))?;
    Ok(())
}

pub async fn stop_session(state: &AppState, user: &User, id: &str) -> ApiResult<()> {
    owned_session(state, user.id, id)?;
    state.workers.stop(id).await;
    Ok(())
}

/// Where a user's saves for a game live: battery saves in
/// `saves/<user>/<system>/<game>.srm`, states in `saves/<user>/<system>/states/<game>/`.
pub(crate) fn save_dirs(state: &AppState, user_id: i64, game: &Game) -> (PathBuf, PathBuf) {
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
    pub slot: u32,
    /// Unix seconds.
    pub saved_at: u64,
    pub screenshot: bool,
}

/// A game's save slots (0 is the auto save), from the files in its state folder.
pub(crate) fn slots(state: &AppState, user_id: i64, game: &Game) -> Vec<SaveSlot> {
    let (_, dir) = save_dirs(state, user_id, game);
    (0..=MAX_SLOT)
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
        .collect()
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

pub async fn delete_state(state: &AppState, user: &User, id: i64, slot: u32) -> ApiResult<()> {
    let game = fetch_game(state, user.id, id).await?;
    let (_, dir) = save_dirs(state, user.id, &game);
    let state_file = dir.join(format!("slot{slot}.state"));
    if !state_file.is_file() {
        return Err(ApiError::not_found("save state"));
    }
    let _ = tokio::fs::remove_file(&state_file).await;
    let _ = tokio::fs::remove_file(dir.join(format!("slot{slot}.png"))).await;
    Ok(())
}

async fn session_request(
    state: &AppState,
    user_id: i64,
    id: &str,
    make: impl FnOnce(u64) -> sovemu_protocol::ipc::Request,
) -> ApiResult<()> {
    let session = owned_session(state, user_id, id)?;
    session.request(make).await.map_err(|e| ApiError::bad_request(format!("{e:#}")))?;
    Ok(())
}

pub async fn save_state(state: &AppState, user: &User, id: &str, slot: u32) -> ApiResult<()> {
    session_request(state, user.id, id, |rid| sovemu_protocol::ipc::Request::SaveState {
        id: rid,
        slot,
    })
    .await
}

pub async fn load_state(state: &AppState, user: &User, id: &str, slot: u32) -> ApiResult<()> {
    session_request(state, user.id, id, |rid| sovemu_protocol::ipc::Request::LoadState {
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
    let file = match media_relative(&state, &game, &kind) {
        Some(relative) => {
            let original = state.config.roms_dir.join(&relative);
            state.cache.local_or_fetch("media", &relative, &original)
        }
        // No local artwork: box art a metadata service supplied stands in for the thumb.
        None => match (kind.as_str(), &game.box_art) {
            ("thumb", Some(name)) => state.metadata.media_dir.join(name),
            _ => return Err(ApiError::not_found("media")),
        },
    };
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
