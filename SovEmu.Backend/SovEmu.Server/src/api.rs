//! The library and session routes. Auth routes are in [`crate::auth`].

use std::path::PathBuf;
use std::time::Instant;

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use serde::Serialize;
use sovemu_protocol::{SdpKind, SessionDescription};

use crate::AppState;
use crate::assets::SignedUser;
use crate::auth::User;
use crate::capabilities::ClientCapabilities;
use crate::error::{ApiError, ApiResult};
use crate::library::ScanReport;
use sovemu_library::systems::{self, System};
use sovemu_sessions::workers::{Launch, SessionInfo};

#[derive(Debug, Clone, Serialize)]
pub(crate) struct SystemSummary {
    #[serde(flatten)]
    pub(crate) system: System,
    pub(crate) game_count: i64,
    /// Whether a core that plays it is installed (and, for a 3D system, it has a renderer).
    pub(crate) playable: bool,
    /// For a 3D system: `gpu`, `software` (slow) or `off` (unplayable here).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) rendering: Option<sovemu_sessions::gpu::Mode>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub(crate) struct Game {
    pub(crate) id: i64,
    pub(crate) system: String,
    pub(crate) title: String,
    pub(crate) tags: String,
    pub(crate) path: String,
    pub(crate) size: i64,
    pub(crate) last_played_at: Option<String>,
    pub(crate) play_count: Option<i64>,
    /// Artwork downloaded for it (`image`, `thumb`, `marquee`, `video`), comma-separated.
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
    /// The ESRB's age rating, as `sovemu_library::info::Esrb::code` writes it.
    pub(crate) esrb: Option<String>,
    /// Ways to play, series and themes: comma-separated names.
    pub(crate) modes: Option<String>,
    pub(crate) series: Option<String>,
    pub(crate) themes: Option<String>,
    /// Downloaded box art (a file under `<data>/artwork/games`), served as `thumb`.
    #[serde(skip)]
    pub(crate) box_art: Option<String>,
    /// A downloaded screenshot (the same folder), served as `image`.
    #[serde(skip)]
    pub(crate) screenshot: Option<String>,
    /// The downloaded logo (the same folder), served as `marquee`.
    #[serde(skip)]
    pub(crate) marquee: Option<String>,
    /// A downloaded gameplay video (the same folder), served as `video`.
    #[serde(skip)]
    pub(crate) box_video: Option<String>,
    /// How many discs (1 for most games).
    pub(crate) discs: i64,
    /// One of this user's favourites (filled in by `collections::mark_favourites`).
    #[sqlx(skip)]
    pub(crate) favourite: bool,
    /// What tells it apart from games of the same title on its system (`releases::refresh`).
    pub(crate) release: Option<String>,
    /// What a multi-part game's files are (`disc`, `disk`, `tape`, `side`, `part`).
    pub(crate) parts: Option<String>,
    /// The soft patch this user plays it with, by name (filled in by `patches::mark`).
    #[sqlx(skip)]
    pub(crate) patch: Option<String>,
}

impl Game {
    /// The downloaded file of its artwork of `kind` (`image`, `thumb`, `marquee`, `video`).
    fn media_file_name(&self, kind: &str) -> Option<&str> {
        match kind {
            "image" => self.screenshot.as_deref(),
            "thumb" => self.box_art.as_deref(),
            "marquee" => self.marquee.as_deref(),
            "video" => self.box_video.as_deref(),
            _ => None,
        }
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

/// The installed core for a system, if any: the one `SovEmu__Systems__<id>__Core` names, or
/// else the first of SovEmu's choices that is installed.
pub(crate) fn find_core(state: &AppState, system: &System) -> Option<PathBuf> {
    let chosen = state.config.systems.get(system.id).and_then(|s| s.core.as_deref());
    let names: Vec<&str> = chosen.map_or_else(|| system.cores.to_vec(), |c| vec![c]);
    names.iter().find_map(|name| {
        ["so", "dylib", "dll"]
            .iter()
            .map(|ext| state.config.cores_dir.join(format!("{name}_libretro.{ext}")))
            .find(|p| p.is_file())
    })
}

/// Whether a system is offered (`SovEmu__Systems__<id>__Enabled=false` hides it).
pub(crate) fn enabled(state: &AppState, id: &str) -> bool {
    state.config.systems.get(id).and_then(|s| s.enabled).unwrap_or(true)
}

/// The system, if it exists and is enabled.
fn system_by_id(state: &AppState, id: &str) -> ApiResult<&'static System> {
    systems::by_id(id).filter(|s| enabled(state, s.id)).ok_or_else(|| ApiError::not_found("system"))
}

pub(crate) async fn list_systems(state: &AppState) -> ApiResult<Vec<SystemSummary>> {
    let counts = state.game_counts.get();
    let summaries = systems::SYSTEMS
        .iter()
        .filter(|s| enabled(state, s.id))
        .map(|s| SystemSummary {
            system: *s,
            game_count: counts.get(s.id).copied().unwrap_or(0),
            playable: find_core(state, s).is_some()
                && (!s.needs_gpu || state.gpu.mode != sovemu_sessions::gpu::Mode::Off),
            rendering: s.needs_gpu.then_some(state.gpu.mode),
        })
        .collect();
    Ok(summaries)
}

/// The systems a player is shown: the enabled ones with at least one game. A system with no
/// games is left out everywhere it would be listed (browsing, search, BIOS, the admin's
/// per-system pages), so the list stays short however many systems SovEmu can play.
pub(crate) async fn systems_with_games(state: &AppState) -> ApiResult<Vec<SystemSummary>> {
    Ok(list_systems(state).await?.into_iter().filter(|s| s.game_count > 0).collect())
}

pub(crate) const GAME_COLUMNS: &str =
    "g.id, g.system, COALESCE(m.title, g.title) AS title, g.tags, g.path,
     g.size, h.last_played_at, h.play_count,
     TRIM(CASE WHEN m.screenshot IS NOT NULL THEN 'image,' ELSE '' END
       || CASE WHEN m.box_art IS NOT NULL THEN 'thumb,' ELSE '' END
       || CASE WHEN m.marquee IS NOT NULL THEN 'marquee,' ELSE '' END
       || CASE WHEN m.video IS NOT NULL THEN 'video' ELSE '' END, ',') AS media,
     CASE WHEN m.title IS NOT NULL AND m.title != g.title THEN g.title END AS file_title,
     m.description, m.release_date, m.developer, m.publisher, m.genre, m.players, m.rating,
     m.esrb, m.modes, m.series, m.themes,
     m.box_art, m.screenshot, m.marquee, m.video AS box_video, g.release, g.parts,
     MAX(1, (SELECT COUNT(*) FROM game_discs d WHERE d.game_id = g.id)) AS discs";

/// What a list of games carries for this user beyond its rows: which are favourites, and the
/// patch each is played with. Every list the API returns goes through here.
pub(crate) async fn annotate(state: &AppState, user_id: i64, games: &mut [Game]) -> ApiResult<()> {
    crate::collections::mark_favourites(state, user_id, games).await?;
    crate::patches::mark(state, user_id, games).await
}

/// The games of one system, by title: the system page's list.
pub(crate) const LIST_GAMES: &str =
    "WHERE g.system = ? ORDER BY COALESCE(m.title, g.title) COLLATE NOCASE, g.path";
/// One game.
pub(crate) const ONE_GAME: &str = "WHERE g.id = ?";

/// A query for games with their user's play history and game information: `tail` is the
/// `WHERE … ORDER BY …` (binds: the user's id, then the tail's).
pub(crate) fn games_sql(tail: &str) -> String {
    format!(
        "SELECT {GAME_COLUMNS} FROM games g
         LEFT JOIN play_history h ON h.game_id = g.id AND h.user_id = ?
         LEFT JOIN game_metadata m ON m.game_id = g.id
         {tail}"
    )
}

pub(crate) async fn list_games(
    state: &AppState,
    user: &User,
    system: &str,
) -> ApiResult<Vec<Game>> {
    system_by_id(state, system)?;
    let games = sqlx::query_as::<_, Game>(&games_sql(LIST_GAMES))
        .bind(user.id)
        .bind(system)
        .fetch_all(&state.db)
        .await?;
    let mut games = games;
    annotate(state, user.id, &mut games).await?;
    Ok(games)
}

/// One game, as this user sees it (favourite, patch).
async fn fetch_game(state: &AppState, user_id: i64, id: i64) -> ApiResult<Game> {
    let mut game = [fetch_row(state, user_id, id).await?];
    annotate(state, user_id, &mut game).await?;
    let [game] = game;
    Ok(game)
}

/// One game's row. A game of a hidden system (`SovEmu__Systems__<id>__Enabled=false`) is not
/// found, as its system is not: by id as by list.
async fn fetch_row(state: &AppState, user_id: i64, id: i64) -> ApiResult<Game> {
    sqlx::query_as::<_, Game>(&games_sql(ONE_GAME))
        .bind(user_id)
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .filter(|g| enabled(state, &g.system))
        .ok_or_else(|| ApiError::not_found("game"))
}

/// That a game exists and its system is offered, for a change that names it (a favourite, a
/// collection entry, a cheat, a patch choice).
pub(crate) async fn require_game(state: &AppState, id: i64) -> ApiResult<()> {
    let system: Option<String> = sqlx::query_scalar("SELECT system FROM games WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.db)
        .await?;
    system.filter(|s| enabled(state, s)).map(|_| ()).ok_or_else(|| ApiError::not_found("game"))
}

pub(crate) async fn get_game(state: &AppState, user: &User, id: i64) -> ApiResult<Game> {
    fetch_game(state, user.id, id).await
}

/// An admin's rescan, waited for. Refused while a mirror sync runs (it ends with a scan of its
/// own) rather than holding the request until the sync is done.
pub(crate) async fn scan(state: &AppState) -> ApiResult<ScanReport> {
    use crate::jobs::{Job, JobError, Trigger};
    if state.jobs.is_running(Job::MirrorSync) {
        return Err(ApiError::conflict("a mirror sync is running; it rescans when it finishes"));
    }
    let report = state
        .jobs
        .run(Job::Scan, Trigger::Admin, crate::admin::scan(state.clone()))
        .await
        .map_err(|e| match e {
            JobError::Running => ApiError::conflict("a library scan is already running"),
            e => e.into(),
        })?;
    crate::admin::after_scan_in_background(state);
    Ok(report)
}

/// Starts a game. Ends the user's current session first. `start_id`, chosen by the client, lets
/// it follow the start's progress ([`start_progress`]) while this waits.
pub(crate) async fn start_session(
    state: &AppState,
    user: &User,
    game_id: i64,
    start_id: Option<&str>,
) -> ApiResult<SessionInfo> {
    let start_id = match start_id {
        Some(id) if !sovemu_sessions::starts::valid_id(id) => {
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

/// How far the start `start_id` has got (see [`sovemu_sessions::starts`]).
pub(crate) fn start_progress(
    state: &AppState,
    user: &User,
    start_id: &str,
) -> ApiResult<sovemu_sessions::starts::StartStatus> {
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
    let refuse =
        |core: Option<&std::path::Path>, error: ApiError| refused(&state, user, &game, core, error);
    if system.needs_gpu && state.gpu.mode == sovemu_sessions::gpu::Mode::Off {
        return Err(refuse(
            None,
            ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                format!(
                    "{} needs 3D rendering, which is unavailable on this server ({})",
                    system.name,
                    state.gpu.note.as_deref().unwrap_or("SovEmu__Rendering__Gpu")
                ),
            ),
        ));
    }
    let core = find_core(&state, system).ok_or_else(|| {
        refuse(
            None,
            ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                format!("no core installed for {}", system.name),
            ),
        )
    })?;
    let original = state.config.roms_dir.join(&game.path);
    if !original.is_file() {
        return Err(refuse(
            Some(&core),
            ApiError::new(StatusCode::GONE, "the game file is missing; rescan the library"),
        ));
    }
    let (save_dir, state_dir) = save_dirs(&state, user.id, &game);
    // A multi-disc game plays from an .m3u of its discs, which the core reads and swaps
    // between. Its files are read where they are, as a .cue's tracks are (never cached).
    let discs: Vec<String> = if game.discs > 1 {
        sqlx::query_scalar("SELECT path FROM game_discs WHERE game_id = ? ORDER BY disc")
            .bind(game.id)
            .fetch_all(&state.db)
            .await?
    } else {
        Vec::new()
    };
    // A local copy when the library is on a slow share and the game has been played before.
    // Otherwise the original: small games are copied for next time straight away, large ones
    // (disc images) only once this session ends, so the copy never competes with the game.
    let cached =
        if discs.is_empty() { state.cache.cached("roms", &game.path, &original) } else { None };
    let large =
        game.size.max(0).unsigned_abs() > state.config.cache_large_game_mb.saturating_mul(1 << 20);
    let rom = match &cached {
        _ if !discs.is_empty() => write_playlist(&state, game.id, &discs)
            .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")))?,
        Some(copy) => copy.clone(),
        None if large => original.clone(),
        None => state.cache.local_or_fetch("roms", &game.path, &original),
    };
    let after_exit: Option<Box<dyn FnOnce() + Send + Sync>> =
        (discs.is_empty() && cached.is_none() && large).then(|| {
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
            patch: crate::patches::effective(&state, user.id, game.id).await?.map(|p| {
                (state.config.roms_dir.join(&p.path), crate::patches::file_label(&p.name))
            }),
            variables: crate::settings::for_launch(&state, system).await?,
            run_ahead: crate::settings::run_ahead_for_launch(&state, system).await?,
            after_exit,
            start_id,
        })
        .await
        .map_err(|e| {
            let error = ApiError::new(StatusCode::SERVICE_UNAVAILABLE, format!("{e:#}"));
            // A worker that ran reports its own failure (`failures`, as the workers' observer).
            if e.before_worker() { refuse(Some(&core), error) } else { error }
        })?;
    if let Some(declared) = session.take_declared() {
        crate::settings::remember(&state.db, &core, &declared).await;
    }
    let codes = crate::cheats::enabled_codes(&state, user.id, game.id).await?;
    if !codes.is_empty()
        && let Err(e) =
            session.request(|id| sovemu_protocol::ipc::Request::SetCheats { id, codes }).await
    {
        tracing::warn!(error = %format!("{e:#}"), "could not apply cheats");
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

/// Keeps a start the server refused, before any worker ran, for the admin's failures list.
fn refused(
    state: &AppState,
    user: &User,
    game: &Game,
    core: Option<&std::path::Path>,
    error: ApiError,
) -> ApiError {
    let start = crate::failures::Refused {
        user_id: user.id,
        game_id: game.id,
        title: &game.title,
        system: &game.system,
        core,
    };
    state.failures.refused(start, &error.message);
    error
}

/// Writes `<data>/cache/discs/<game>.m3u`, listing a multi-disc game's files (absolute paths,
/// as every disc core reads them), and returns its path.
fn write_playlist(
    state: &AppState,
    game_id: i64,
    discs: &[String],
) -> anyhow::Result<std::path::PathBuf> {
    let dir = state.config.paths().disc_playlists();
    std::fs::create_dir_all(&dir)?;
    let mut text = String::new();
    for disc in discs {
        text.push_str(&state.config.roms_dir.join(disc).to_string_lossy());
        text.push('\n');
    }
    let path = dir.join(format!("{game_id}.m3u"));
    // Its own temp file per writer: two players starting the same game at once each write
    // theirs, and the last rename wins with the same content.
    let temp = dir.join(format!("{game_id}.m3u.{}.tmp", uuid_like()));
    std::fs::write(&temp, text)?;
    std::fs::rename(&temp, &path)?;
    Ok(path)
}

/// A name unique to this process and moment, for a temp file.
fn uuid_like() -> String {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("{}-{n}", std::process::id())
}

/// Changes a multi-disc game's disc (1-based), as a player swaps discs when the game asks.
pub(crate) async fn set_disc(state: &AppState, user: &User, id: &str, disc: u32) -> ApiResult<()> {
    let session = owned_session(state, user.id, id)?;
    let count = session.info().discs;
    if disc == 0 || disc > count {
        return Err(ApiError::bad_request(format!("disc: 1 to {count}")));
    }
    session_request(state, user.id, id, |rid| sovemu_protocol::ipc::Request::SetDisc {
        id: rid,
        disc: disc - 1,
    })
    .await?;
    session.set_current_disc(disc);
    Ok(())
}

/// A running session as the admin's sessions page shows it.
pub(crate) struct ActiveSession {
    pub(crate) info: SessionInfo,
    pub(crate) user_id: i64,
    /// The player's name; none if the account has gone.
    pub(crate) username: Option<String>,
    pub(crate) started_at: f64,
    pub(crate) live: sovemu_sessions::workers::Live,
}

/// Every account's name by its id, for the admin's lists of sessions and failures.
pub(crate) async fn usernames(
    state: &AppState,
) -> ApiResult<std::collections::HashMap<i64, String>> {
    Ok(state.identity.users().await?.into_iter().map(|u| (u.id, u.username)).collect())
}

/// Every running session, oldest first, with what its worker and client last reported.
pub(crate) async fn active_sessions(state: &AppState) -> ApiResult<Vec<ActiveSession>> {
    let sessions = state.workers.all();
    if sessions.is_empty() {
        return Ok(Vec::new());
    }
    let names = usernames(state).await?;
    Ok(sessions
        .into_iter()
        .map(|s| ActiveSession {
            info: s.info(),
            user_id: s.user_id,
            username: names.get(&s.user_id).cloned(),
            started_at: s.started_at(),
            live: s.live(),
        })
        .collect())
}

pub(crate) fn current_session(state: &AppState, user: &User) -> Option<SessionInfo> {
    state.workers.for_user(user.id).map(|s| s.info())
}

fn owned_session(
    state: &AppState,
    user_id: i64,
    id: &str,
) -> ApiResult<std::sync::Arc<sovemu_sessions::workers::Session>> {
    state
        .workers
        .get(id)
        .filter(|s| s.user_id == user_id)
        .ok_or_else(|| ApiError::not_found("session"))
}

/// A client's SDP offer for the session; returns the worker's answer.
/// A client's offer for a session: its SDP, what it calls itself, and what it can decode.
pub(crate) struct Offer {
    pub(crate) sdp: String,
    pub(crate) client: Option<String>,
    pub(crate) capabilities: Option<ClientCapabilities>,
}

/// Answers a client's offer. Its capabilities are checked first and kept against the sign-in
/// that `headers`' token belongs to, so a document the server cannot read stops the offer.
pub(crate) async fn offer(
    state: &AppState,
    user: &User,
    headers: &HeaderMap,
    id: &str,
    offer: Offer,
) -> ApiResult<String> {
    let Offer { sdp, client, capabilities } = offer;
    let session = owned_session(state, user.id, id)?;
    let capabilities =
        capabilities.map(ClientCapabilities::checked).transpose().map_err(ApiError::bad_request)?;
    // The sign-in the document is kept under: the device its decoder's measurements belong to.
    let mut device = None;
    if let Some(capabilities) = &capabilities {
        device = state.identity.keep_capabilities(headers, &capabilities.to_json()).await?;
        tracing::info!(
            session.id = id,
            client = client.as_deref().unwrap_or_default(),
            decoders = %capabilities.summary(),
            display.width = capabilities.display.width,
            display.height = capabilities.display.height,
            display.refresh_hz = capabilities.display.refresh_hz,
            kept = device.is_some(),
            "client capabilities"
        );
    }
    let measured = match &device {
        Some(device) => crate::decode_times::of(&state.db, device).await?,
        None => Vec::new(),
    };
    let info = session.info();
    let choice = crate::codec::choose(&crate::codec::Inputs {
        setting: &state.config.video_codec,
        encoder: &state.config.video_encoder,
        test: &state.encoders,
        capabilities: capabilities.as_ref(),
        sdp: &sdp,
        measured: &measured,
        size: (info.width, info.height),
    });
    tracing::info!(
        session.id = id,
        codec = choice.codec.map_or("the setting's", |c| c.name()),
        reason = %choice.reason,
        "video codec chosen"
    );
    let codec = choice.codec.map(|c| c.name().to_owned());
    let answer = session
        .offer(SessionDescription { kind: SdpKind::Offer, sdp }, codec, Some(choice.reason), device)
        .await
        .map_err(|e| ApiError::bad_request(format!("{e:#}")))?;
    // A name, not a message: one line, and short.
    let client = client
        .map(|c| c.lines().next().unwrap_or_default().chars().take(80).collect::<String>())
        .filter(|c| !c.trim().is_empty());
    session.set_client(client);
    Ok(answer.sdp)
}

pub(crate) async fn reset(state: &AppState, user: &User, id: &str) -> ApiResult<()> {
    let session = owned_session(state, user.id, id)?;
    session
        .request(|id| sovemu_protocol::ipc::Request::Reset { id })
        .await
        .map_err(|e| ApiError::new(StatusCode::BAD_GATEWAY, format!("{e:#}")))?;
    Ok(())
}

/// Ends a session: the player's own, or, for an admin, anyone's (the Sessions page). Through
/// `Workers::stop`, which asks the worker to write its saves before anything is killed; the
/// player's client sees the game end as after an idle timeout.
/// Ends a session: the player's own, or anyone's for an admin. Whose it was, and its game.
pub(crate) async fn stop_session(
    state: &AppState,
    user: &User,
    id: &str,
) -> ApiResult<(i64, String)> {
    let session = if user.is_admin() {
        state.workers.get(id).ok_or_else(|| ApiError::not_found("session"))?
    } else {
        owned_session(state, user.id, id)?
    };
    if session.user_id != user.id {
        tracing::info!(by = %user.username, player = session.user_id, "session stopped by an admin");
    }
    state.workers.stop(id).await;
    Ok((session.user_id, session.info().title))
}

/// Where a user's saves for a game live: battery saves in
/// `saves/<user>/<system>/<game>.srm`, states in `saves/<user>/<system>/states/<game>/`. A
/// patched game is `<game> [<patch>]`: its saves are its own.
pub(crate) fn save_dirs(state: &AppState, user_id: i64, game: &Game) -> (PathBuf, PathBuf) {
    let save_dir = state.config.paths().saves(user_id, &game.system);
    let mut stem = std::path::Path::new(&game.path)
        .file_stem()
        .map_or_else(|| game.id.to_string(), |s| s.to_string_lossy().into_owned());
    if let Some(patch) = &game.patch {
        stem = format!("{stem} [{}]", crate::patches::file_label(patch));
    }
    let state_dir = save_dir.join("states").join(stem);
    (save_dir, state_dir)
}

/// The highest save-state slot (slot 0 is the auto state).
const MAX_SLOT: u32 = 9;

#[derive(Debug, Serialize)]
pub(crate) struct SaveSlot {
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

pub(crate) async fn state_screenshot(
    State(state): State<AppState>,
    SignedUser(user_id): SignedUser,
    Path((id, slot)): Path<(i64, u32)>,
) -> ApiResult<axum::response::Response> {
    use axum::response::IntoResponse;
    let game = fetch_game(&state, user_id, id).await?;
    let (_, dir) = save_dirs(&state, user_id, &game);
    let bytes = tokio::fs::read(dir.join(format!("slot{slot}.png")))
        .await
        .map_err(|_| ApiError::not_found("screenshot"))?;
    Ok(([(axum::http::header::CONTENT_TYPE, "image/png")], bytes).into_response())
}

pub(crate) async fn delete_state(
    state: &AppState,
    user: &User,
    id: i64,
    slot: u32,
) -> ApiResult<()> {
    check_slot(slot)?;
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

/// Slots a player saves to: 1 to [`MAX_SLOT`]. Slot 0 is the auto save, which only the worker
/// writes (as a session ends); a slot above the last is never listed, so a save there is lost.
fn check_save_slot(slot: u32) -> ApiResult<()> {
    if (1..=MAX_SLOT).contains(&slot) {
        Ok(())
    } else {
        Err(ApiError::bad_request(format!("save slots are 1 to {MAX_SLOT}")))
    }
}

/// Slots a player loads or deletes: the auto save (0) too.
fn check_slot(slot: u32) -> ApiResult<()> {
    if slot <= MAX_SLOT {
        Ok(())
    } else {
        Err(ApiError::bad_request(format!("slots are 0 to {MAX_SLOT}")))
    }
}

pub(crate) async fn save_state(
    state: &AppState,
    user: &User,
    id: &str,
    slot: u32,
) -> ApiResult<()> {
    check_save_slot(slot)?;
    session_request(state, user.id, id, |rid| sovemu_protocol::ipc::Request::SaveState {
        id: rid,
        slot,
    })
    .await
}

pub(crate) async fn load_state(
    state: &AppState,
    user: &User,
    id: &str,
    slot: u32,
) -> ApiResult<()> {
    check_slot(slot)?;
    session_request(state, user.id, id, |rid| sovemu_protocol::ipc::Request::LoadState {
        id: rid,
        slot,
    })
    .await
}

/// A decoration's image (`/media/decorations/<set>/<path>`, the signed URL `Game.decoration`
/// hands out). Only files the decoration folders list are served, so the path cannot leave them.
pub(crate) async fn decoration_file(
    State(state): State<AppState>,
    SignedUser(_user): SignedUser,
    Path((set, path)): Path<(String, String)>,
    request: axum::extract::Request,
) -> ApiResult<axum::response::Response> {
    use axum::response::IntoResponse;
    use tower::ServiceExt;
    let decorations = state.decorations();
    let file = decorations
        .file(&set, &path)
        .filter(|_| path.ends_with(".png"))
        .ok_or_else(|| ApiError::not_found("decoration"))?
        .to_owned();
    let mut response = tower_http::services::ServeFile::new(file)
        .oneshot(request)
        .await
        .map_err(ApiError::internal)?
        .into_response();
    // Decorations change only when a folder's files do; let browsers keep them a day.
    response.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("private, max-age=86400"),
    );
    Ok(response)
}

/// An ESRB rating's icon (`/media/ratings/esrb/<rating>.png`, the signed URL `Game.esrbBadgeUrl`
/// hands out): the data folder's, else the built-in one.
pub(crate) async fn esrb_file(
    State(state): State<AppState>,
    SignedUser(_user): SignedUser,
    Path(file): Path<String>,
    request: axum::extract::Request,
) -> ApiResult<axum::response::Response> {
    let rating = sovemu_library::info::Esrb::ALL
        .into_iter()
        .find(|r| file == format!("{}.png", r.file_stem()))
        .ok_or_else(|| ApiError::not_found("rating"))?;
    let path = sovemu_library::layout::esrb_file(&state.config.artwork_dirs(), rating)
        .ok_or_else(|| ApiError::not_found("rating"))?;
    serve_media(path, request).await
}

/// The file of a game's artwork or video (`image`, `thumb`, `marquee`, `video`) for a signed
/// user: what a metadata service supplied, downloaded under `<data>/artwork/games`.
async fn media_file(
    state: &AppState,
    user_id: i64,
    id: i64,
    kind: &str,
) -> ApiResult<std::path::PathBuf> {
    // The row alone: an image needs no favourites or patch (one query per thumbnail, not three).
    let game = fetch_row(state, user_id, id).await?;
    game.media_file_name(kind)
        .map(|name| state.metadata.media_dir.join(name))
        .ok_or_else(|| ApiError::not_found("media"))
}

/// Serves `file`, with range support (so browsers can stream a video), kept a day by browsers:
/// artwork changes only on a rescrape.
async fn serve_media(
    file: std::path::PathBuf,
    request: axum::extract::Request,
) -> ApiResult<axum::response::Response> {
    use axum::response::IntoResponse;
    use tower::ServiceExt;
    let mut response = tower_http::services::ServeFile::new(file)
        .oneshot(request)
        .await
        .map_err(ApiError::internal)?
        .into_response();
    response.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("private, max-age=86400"),
    );
    Ok(response)
}

/// A game's artwork or video (`image`, `thumb`, `marquee`, `video`) as it is.
pub(crate) async fn game_media(
    State(state): State<AppState>,
    SignedUser(user_id): SignedUser,
    Path((id, kind)): Path<(i64, String)>,
    request: axum::extract::Request,
) -> ApiResult<axum::response::Response> {
    let file = media_file(&state, user_id, id, &kind).await?;
    serve_media(file, request).await
}

/// Copies being made at once: resizing is CPU work, so no more than the machine has cores.
fn copy_slots() -> &'static tokio::sync::Semaphore {
    static SLOTS: std::sync::OnceLock<tokio::sync::Semaphore> = std::sync::OnceLock::new();
    SLOTS.get_or_init(|| {
        tokio::sync::Semaphore::new(std::thread::available_parallelism().map_or(2, |n| n.get()))
    })
}

/// A game's artwork at a width (`/media/games/{id}/{kind}/w{width}`): a lossy WebP copy at most
/// that wide, made from the original the first time it is asked for and kept
/// (`<data>/cache/artwork`), made again once the original is newer. Only widths on the
/// grid are made (`sovemu_library::artwork::on_grid`); the width is in the signed path, so only
/// widths the API handed out reach here. A picture that cannot be read is served as it is.
pub(crate) async fn game_media_width(
    State(state): State<AppState>,
    SignedUser(user_id): SignedUser,
    Path((id, kind, width)): Path<(i64, String, String)>,
    request: axum::extract::Request,
) -> ApiResult<axum::response::Response> {
    let width: u32 = width
        .strip_prefix('w')
        .and_then(|w| w.parse().ok())
        .filter(|w| sovemu_library::artwork::on_grid(*w))
        .ok_or_else(|| ApiError::not_found("media"))?;
    if kind == "video" {
        return Err(ApiError::not_found("media"));
    }
    let original = media_file(&state, user_id, id, &kind).await?;
    let copy =
        state.config.paths().artwork_widths().join(format!("games/{id}/{kind}/w{width}.webp"));
    let made = {
        let _slot = copy_slots().acquire().await.map_err(ApiError::internal)?;
        let (src, dst) = (original.clone(), copy.clone());
        let quality = state.config.artwork_webp_quality as f32;
        tokio::task::spawn_blocking(move || {
            sovemu_library::artwork::webp_copy(&src, &dst, width, quality)
        })
        .await
        .map_err(ApiError::internal)?
    };
    match made {
        Ok(()) => serve_media(copy, request).await,
        Err(e) => {
            tracing::warn!(game_id = id, kind = %kind, width, error = %e, "cannot make an artwork copy");
            serve_media(original, request).await
        }
    }
}
