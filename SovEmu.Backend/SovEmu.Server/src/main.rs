//! `sovemu-server`: the SovEmu web client, its API, the ROM library and game sessions.
//!
//! One process serves `SovEmu.Web`'s static build and `/api/*` on one port. Each game runs in
//! its own `sovemu-worker session` process (see [`workers`]), which streams to the browser over
//! WebRTC on a UDP port of its own.

mod admin;
mod api;
mod auth;
mod cache;
mod cheats;
mod db;
mod error;
mod gpu;
mod home;
#[cfg(test)]
mod http_tests;
mod library;
mod metadata;
mod mirror;
mod ops;
mod settings;
mod starts;
mod structured;
mod systems;
mod workers;

use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use axum::Router;
use axum::extract::Request;
use axum::http::HeaderValue;
use axum::http::header::CACHE_CONTROL;
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::{delete, get, post};
use clap::Parser;
use sqlx::SqlitePool;
use tower_http::services::ServeDir;
use tracing_subscriber::EnvFilter;

use crate::workers::{WorkerConfig, Workers};

#[derive(Debug, Clone, Parser)]
#[command(name = "sovemu-server", version, about = "SovEmu: console emulation over WebRTC")]
pub struct Config {
    /// Address for the web client and API.
    #[arg(long, env = "SOVEMU_HTTP", default_value = "0.0.0.0:8080")]
    pub http: SocketAddr,

    /// Where SovEmu keeps its database, saves and BIOS files (`system/`).
    #[arg(long, env = "SOVEMU_DATA_DIR", default_value = "data")]
    pub data_dir: PathBuf,

    /// The ROM library: one folder per system (`nes/`, `snes/`, …). Default: `<data-dir>/roms`.
    #[arg(long = "roms-dir", env = "SOVEMU_ROMS_DIR")]
    roms: Option<PathBuf>,

    /// BIOS and system files for cores. Default: `<data-dir>/system`.
    #[arg(long = "system-dir", env = "SOVEMU_SYSTEM_DIR")]
    system: Option<PathBuf>,

    /// Directory of libretro cores (`<name>_libretro.so`).
    #[arg(long, env = "SOVEMU_CORES_DIR", default_value = "cores")]
    pub cores_dir: PathBuf,

    /// The built web client (`SovEmu.Web/dist`).
    #[arg(long, env = "SOVEMU_WEB_DIR")]
    pub web_dir: Option<PathBuf>,

    /// The worker binary. Default: `sovemu-worker` next to this binary.
    #[arg(long, env = "SOVEMU_WORKER")]
    worker: Option<PathBuf>,

    /// UDP ports for game sessions, one per concurrent session: `FIRST-LAST`.
    #[arg(long, env = "SOVEMU_UDP_PORTS", default_value = "50000-50009", value_parser = parse_ports)]
    pub udp_ports: (u16, u16),

    /// Address workers bind their UDP ports on.
    #[arg(long, env = "SOVEMU_UDP_BIND", default_value = "0.0.0.0")]
    pub udp_bind: IpAddr,

    /// The IP address browsers send media to: in a container, the host's address.
    #[arg(long, env = "SOVEMU_ADVERTISE_IP")]
    pub advertise_ip: Option<IpAddr>,

    /// How to use the library: `direct` (play from it as is: local disk or a share on the LAN),
    /// `cache` (a slow, remote share: keep what is played on local disk) or `mirror` (keep a
    /// full local copy, synced in the background, and play from that).
    #[arg(long, env = "SOVEMU_LIBRARY_MODE", value_enum, default_value_t = LibraryMode::Direct)]
    pub library_mode: LibraryMode,

    /// `cache` mode: keep up to this many GiB on local disk (default 20 in that mode).
    #[arg(long, env = "SOVEMU_CACHE_GB")]
    pub cache_gb: Option<u64>,

    /// `mirror` mode: only these systems (ids, comma-separated, e.g. `nes,snes`). Default: all.
    #[arg(long, env = "SOVEMU_MIRROR_SYSTEMS", value_delimiter = ',')]
    pub mirror_systems: Vec<String>,

    /// ScreenScraper developer id and password (request them from screenscraper.fr).
    #[arg(long, env = "SOVEMU_SCREENSCRAPER_DEVID")]
    pub screenscraper_devid: Option<String>,
    #[arg(long, env = "SOVEMU_SCREENSCRAPER_DEVPASSWORD")]
    pub screenscraper_devpassword: Option<String>,
    /// A ScreenScraper user account (optional; raises the daily quota).
    #[arg(long, env = "SOVEMU_SCREENSCRAPER_USER")]
    pub screenscraper_user: Option<String>,
    #[arg(long, env = "SOVEMU_SCREENSCRAPER_PASSWORD")]
    pub screenscraper_password: Option<String>,
    #[arg(long, env = "SOVEMU_SCREENSCRAPER_URL", default_value = metadata::screenscraper::DEFAULT_BASE)]
    pub screenscraper_url: String,

    /// TheGamesDB API key (request one at forums.thegamesdb.net).
    #[arg(long, env = "SOVEMU_THEGAMESDB_KEY")]
    pub thegamesdb_key: Option<String>,
    #[arg(long, env = "SOVEMU_THEGAMESDB_URL", default_value = metadata::thegamesdb::DEFAULT_BASE)]
    pub thegamesdb_url: String,

    /// Resolved: in `mirror` mode the share being mirrored (and `roms_dir` is the mirror).
    #[arg(skip)]
    pub source_dir: Option<PathBuf>,

    /// End a session after this many seconds with no client connected.
    #[arg(long, env = "SOVEMU_IDLE_TIMEOUT", default_value_t = 300)]
    pub idle_timeout: u64,

    /// Mark the session cookie `Secure` (set when served over HTTPS).
    #[arg(long, env = "SOVEMU_SECURE_COOKIES")]
    pub secure_cookies: bool,

    /// STUN/TURN servers for browsers, as a JSON array of `RTCIceServer` objects, e.g.
    /// `[{"urls":"turn:turn.example:3478","username":"u","credential":"p"}]`. Only needed when
    /// clients cannot reach the session UDP ports directly.
    #[arg(long, env = "SOVEMU_ICE_SERVERS", default_value = "[]", value_parser = parse_ice_servers)]
    pub ice_servers: serde_json::Value,

    /// Bearer token that enables `GET /metrics` (Prometheus). Unset: metrics are off.
    #[arg(long, env = "SOVEMU_METRICS_TOKEN")]
    pub metrics_token: Option<String>,

    /// The admin account, created at startup if missing; its password is kept in step with
    /// this setting. With it set there is no first-run setup page.
    #[arg(long, env = "SOVEMU_ADMIN_USERNAME")]
    pub admin_username: Option<String>,
    #[arg(long, env = "SOVEMU_ADMIN_PASSWORD")]
    pub admin_password: Option<String>,

    /// Rescan the library every this many minutes (0: only at startup).
    #[arg(long, env = "SOVEMU_SCAN_INTERVAL_MINUTES", default_value_t = 60)]
    pub scan_interval_minutes: u64,

    /// `mirror` mode: sync every this many minutes (0: only at startup).
    #[arg(long, env = "SOVEMU_MIRROR_INTERVAL_MINUTES", default_value_t = 360)]
    pub mirror_interval_minutes: u64,

    /// Look up game information for new games after every scan.
    #[arg(long, env = "SOVEMU_METADATA_AUTO", default_value_t = true, action = clap::ArgAction::Set)]
    pub metadata_auto: bool,

    /// Video bitrate cap for game streams, kbit/s (adaptation stays at or below it).
    #[arg(long, env = "SOVEMU_VIDEO_KBPS", default_value_t = 2500)]
    pub video_kbps: u32,

    /// Opus audio bitrate for game streams, kbit/s.
    #[arg(long, env = "SOVEMU_AUDIO_KBPS", default_value_t = 128)]
    pub audio_kbps: u32,

    /// How many days a sign-in lasts.
    #[arg(long, env = "SOVEMU_SESSION_DAYS", default_value_t = 30)]
    pub session_days: u32,

    /// Failed sign-ins per username before it is locked out (0: never).
    #[arg(long, env = "SOVEMU_LOGIN_MAX_FAILURES", default_value_t = ops::MAX_FAILURES)]
    pub login_max_failures: u32,

    /// How long a locked-out username waits, and the window failures are counted in.
    #[arg(long, env = "SOVEMU_LOGIN_LOCKOUT_MINUTES", default_value_t = 5)]
    pub login_lockout_minutes: u64,

    /// Delete accounts not declared in `SOVEMU_USERS__*` (or `SOVEMU_ADMIN_USERNAME`) at
    /// startup, so the environment is the whole list of users.
    #[arg(long, env = "SOVEMU_USERS_PRUNE")]
    pub users_prune: bool,

    /// How long a worker may take to load a core and game, seconds.
    #[arg(long, env = "SOVEMU_WORKER_START_TIMEOUT", default_value_t = 120)]
    pub worker_start_timeout: u64,

    /// Rewind history per session, seconds (0 turns rewind off).
    #[arg(long, env = "SOVEMU_REWIND_SECONDS", default_value_t = 60)]
    pub rewind_seconds: u32,

    /// Memory cap for one session's rewind history, MiB.
    #[arg(long, env = "SOVEMU_REWIND_MAX_MB", default_value_t = 64)]
    pub rewind_max_mb: u32,

    /// Fast-forward speed (frames emulated per frame shown).
    #[arg(long, env = "SOVEMU_FAST_FORWARD", default_value_t = 4)]
    pub fast_forward: u32,

    /// Games in the home screen's hero (0 hides it).
    #[arg(long, env = "SOVEMU_HOME_FEATURED", default_value_t = 8)]
    pub home_featured: u32,

    /// Games in each of the home screen's rows (recently played, recently added).
    #[arg(long, env = "SOVEMU_HOME_ROW", default_value_t = 20)]
    pub home_row: u32,

    /// Frames between VP8 keyframes (lost packets recover sooner with fewer).
    #[arg(long, env = "SOVEMU_KEYFRAME_INTERVAL", default_value_t = 600)]
    pub keyframe_interval: u32,

    /// `cache` mode: games larger than this (MiB) are copied before starting, smaller ones
    /// are played from the share while the copy runs.
    #[arg(long, env = "SOVEMU_CACHE_LARGE_GAME_MB", default_value_t = 64)]
    pub cache_large_game_mb: u64,

    /// Files copied at once by the mirror sync and the artwork cache.
    #[arg(long, env = "SOVEMU_COPY_THREADS", default_value_t = 4)]
    pub copy_threads: usize,

    /// Region preference for game information, most preferred first.
    #[arg(
        long,
        env = "SOVEMU_METADATA_REGIONS",
        value_delimiter = ',',
        default_value = "us,wor,eu,ss,uk,jp"
    )]
    pub metadata_regions: Vec<String>,

    /// Language preference for descriptions, most preferred first.
    #[arg(
        long,
        env = "SOVEMU_METADATA_LANGUAGES",
        value_delimiter = ',',
        default_value = "en,wor"
    )]
    pub metadata_languages: Vec<String>,

    /// Pause between ScreenScraper requests, milliseconds (free accounts: one at a time).
    #[arg(long, env = "SOVEMU_SCREENSCRAPER_DELAY_MS", default_value_t = 1200)]
    pub screenscraper_delay_ms: u64,

    /// The GPU for 3D systems (N64, Dreamcast, GameCube, PS2): `auto` (the first GPU that
    /// works, else software rendering), `software` (Mesa's llvmpipe), `off` (no 3D systems) or
    /// a render node such as `/dev/dri/renderD128`. Probed at startup.
    #[arg(long, env = "SOVEMU_GPU", default_value = "auto")]
    pub gpu: String,

    /// Users declared as `SOVEMU_USERS__<n>__USERNAME` / `__PASSWORD` / `__ADMIN`.
    #[arg(skip)]
    pub users: Vec<structured::UserSpec>,

    /// Per-system overrides declared as `SOVEMU_SYSTEMS__<id>__CORE` / `__ENABLED` /
    /// `__OPTIONS__<name>`.
    #[arg(skip)]
    pub systems: std::collections::BTreeMap<String, structured::SystemSpec>,

    /// Resolved from `roms` after parsing.
    #[arg(skip)]
    pub roms_dir: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LibraryMode {
    Direct,
    Cache,
    Mirror,
}

/// Settings that hold secrets: each may instead come from a file named by `<VAR>_FILE` (Docker
/// and Kubernetes secrets), and is redacted in the startup log.
const SECRETS: &[&str] = &[
    "SOVEMU_ADMIN_PASSWORD",
    "SOVEMU_METRICS_TOKEN",
    "SOVEMU_SCREENSCRAPER_DEVPASSWORD",
    "SOVEMU_SCREENSCRAPER_PASSWORD",
    "SOVEMU_THEGAMESDB_KEY",
];

impl Config {
    fn secret_mut(&mut self, var: &str) -> Option<&mut Option<String>> {
        Some(match var {
            "SOVEMU_ADMIN_PASSWORD" => &mut self.admin_password,
            "SOVEMU_METRICS_TOKEN" => &mut self.metrics_token,
            "SOVEMU_SCREENSCRAPER_DEVPASSWORD" => &mut self.screenscraper_devpassword,
            "SOVEMU_SCREENSCRAPER_PASSWORD" => &mut self.screenscraper_password,
            "SOVEMU_THEGAMESDB_KEY" => &mut self.thegamesdb_key,
            _ => return None,
        })
    }

    /// Fills unset secrets from `<VAR>_FILE`, and treats empty values as unset.
    pub fn resolve_secrets(&mut self, env: impl Fn(&str) -> Option<String>) -> Result<()> {
        for var in SECRETS {
            let slot = self.secret_mut(var).expect("every secret has a field");
            if slot.as_deref().is_some_and(|v| v.is_empty()) {
                *slot = None;
            }
            if slot.is_none()
                && let Some(path) = env(&format!("{var}_FILE")).filter(|p| !p.is_empty())
            {
                let value = std::fs::read_to_string(&path)
                    .with_context(|| format!("reading {var}_FILE ({path})"))?;
                *slot = Some(value.trim_end_matches(['\n', '\r']).to_owned());
            }
        }
        Ok(())
    }

    /// The effective settings, one per line, secrets redacted: logged at startup so a
    /// deployment's configuration can be checked without guessing.
    pub fn summary(&self) -> String {
        let secret = |v: &Option<String>| if v.is_some() { "(set)" } else { "(unset)" };
        let opt = |v: &Option<String>| v.clone().unwrap_or_else(|| "(unset)".into());
        let lines = [
            format!("http={}", self.http),
            format!("data_dir={}", self.data_dir.display()),
            format!("roms_dir={}", self.roms_dir.display()),
            format!("library_mode={:?}", self.library_mode),
            format!("cache_gb={}", self.cache_gb.map_or("(default)".into(), |g| g.to_string())),
            format!(
                "mirror_systems={}",
                if self.mirror_systems.is_empty() {
                    "(all)".into()
                } else {
                    self.mirror_systems.join(",")
                }
            ),
            format!("scan_interval_minutes={}", self.scan_interval_minutes),
            format!("mirror_interval_minutes={}", self.mirror_interval_minutes),
            format!("metadata_auto={}", self.metadata_auto),
            format!("cores_dir={}", self.cores_dir.display()),
            format!(
                "web_dir={}",
                self.web_dir.as_ref().map_or("(none)".into(), |d| d.display().to_string())
            ),
            format!("udp_ports={}-{}", self.udp_ports.0, self.udp_ports.1),
            format!(
                "advertise_ip={}",
                self.advertise_ip.map_or("(detected)".into(), |ip| ip.to_string())
            ),
            format!("video_kbps={} audio_kbps={}", self.video_kbps, self.audio_kbps),
            format!("idle_timeout={}s", self.idle_timeout),
            format!(
                "admin_username={} admin_password={}",
                opt(&self.admin_username),
                secret(&self.admin_password)
            ),
            format!(
                "screenscraper_devid={} devpassword={} user={} password={}",
                opt(&self.screenscraper_devid),
                secret(&self.screenscraper_devpassword),
                opt(&self.screenscraper_user),
                secret(&self.screenscraper_password)
            ),
            format!("thegamesdb_key={}", secret(&self.thegamesdb_key)),
            format!("metrics_token={}", secret(&self.metrics_token)),
            format!(
                "session_days={} login_max_failures={} login_lockout_minutes={}",
                self.session_days, self.login_max_failures, self.login_lockout_minutes
            ),
            format!(
                "users={} users_prune={}",
                if self.users.is_empty() {
                    "(none declared)".into()
                } else {
                    self.users
                        .iter()
                        .map(|u| format!("{}{}", u.username, if u.admin { "(admin)" } else { "" }))
                        .collect::<Vec<_>>()
                        .join(",")
                },
                self.users_prune
            ),
            format!(
                "worker_start_timeout={}s rewind_seconds={} rewind_max_mb={} fast_forward={} \
                 keyframe_interval={}",
                self.worker_start_timeout,
                self.rewind_seconds,
                self.rewind_max_mb,
                self.fast_forward,
                self.keyframe_interval
            ),
            format!(
                "gpu={} home_featured={} home_row={}",
                self.gpu, self.home_featured, self.home_row
            ),
            format!(
                "cache_large_game_mb={} copy_threads={}",
                self.cache_large_game_mb, self.copy_threads
            ),
            format!(
                "metadata_regions={} metadata_languages={} screenscraper_delay_ms={}",
                self.metadata_regions.join(","),
                self.metadata_languages.join(","),
                self.screenscraper_delay_ms
            ),
        ];
        let mut lines = lines.to_vec();
        for (id, spec) in &self.systems {
            let mut parts = Vec::new();
            if let Some(core) = &spec.core {
                parts.push(format!("core={core}"));
            }
            if let Some(enabled) = spec.enabled {
                parts.push(format!("enabled={enabled}"));
            }
            for (k, v) in &spec.options {
                parts.push(format!("{k}={v}"));
            }
            lines.push(format!("system {id}: {}", parts.join(" ")));
        }
        lines.join("\n")
    }
}

fn parse_ice_servers(s: &str) -> Result<serde_json::Value, String> {
    let value: serde_json::Value = serde_json::from_str(s).map_err(|e| format!("not JSON: {e}"))?;
    let ok =
        value.as_array().is_some_and(|servers| servers.iter().all(|s| s.get("urls").is_some()));
    if ok { Ok(value) } else { Err("expected an array of objects with \"urls\"".into()) }
}

fn parse_ports(s: &str) -> Result<(u16, u16), String> {
    let (a, b) = s.split_once('-').unwrap_or((s, s));
    let (a, b): (u16, u16) = (
        a.trim().parse().map_err(|_| format!("bad port {a:?}"))?,
        b.trim().parse().map_err(|_| format!("bad port {b:?}"))?,
    );
    if a == 0 || b < a {
        return Err(format!("bad port range {s:?}"));
    }
    Ok((a, b))
}

#[derive(Clone)]
pub struct AppState {
    pub db: SqlitePool,
    pub config: Arc<Config>,
    pub workers: Arc<Workers>,
    /// Verified against when a username does not exist, so failures take the same time.
    pub dummy_hash: String,
    pub login_throttle: Arc<ops::LoginThrottle>,
    pub cache: Arc<cache::Cache>,
    pub metadata: Arc<metadata::Metadata>,
    pub mirror: Arc<std::sync::Mutex<mirror::SyncStatus>>,
    /// Where 3D systems render, decided at startup.
    pub gpu: Arc<gpu::GpuStatus>,
    /// What each core offers as settings, probed once.
    pub core_info: Arc<settings::CoreInfoCache>,
}

impl std::fmt::Debug for AppState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppState").finish_non_exhaustive()
    }
}

pub fn router(state: AppState) -> Router {
    let api = Router::new()
        .route("/config", get(ops::client_config))
        .route("/auth/status", get(auth::status))
        .route("/auth/setup", post(auth::setup))
        .route("/auth/login", post(auth::login))
        .route("/auth/logout", post(auth::logout))
        .route("/users", get(auth::list_users).post(auth::add_user))
        .route("/home", get(home::home))
        .route("/systems", get(api::list_systems))
        .route("/systems/{id}/games", get(api::list_games))
        .route("/games/{id}", get(api::get_game))
        .route("/games/{id}/media/{kind}", get(api::game_media))
        .route("/games/{id}/cheats", get(cheats::list).post(cheats::add))
        .route(
            "/games/{id}/cheats/{cheat}",
            axum::routing::patch(cheats::update).delete(cheats::remove),
        )
        .route("/games/{id}/states", get(api::list_states))
        .route("/games/{id}/states/{slot}", delete(api::delete_state))
        .route("/games/{id}/states/{slot}/screenshot", get(api::state_screenshot))
        .route("/library/scan", post(api::scan))
        .route("/library/status", get(admin::library_status))
        .route("/library/sync", post(admin::library_sync))
        .route("/metadata/status", get(admin::metadata_status))
        .route("/metadata/fetch", post(admin::metadata_fetch))
        .route("/gpu", get(admin::gpu_status))
        .route("/systems/{id}/settings", get(settings::get_settings).put(settings::put_settings))
        .route("/sessions", post(api::start_session))
        .route("/sessions/current", get(api::current_session))
        .route("/sessions/starting/{start_id}", get(api::start_progress))
        .route("/sessions/{id}", delete(api::stop_session))
        .route("/sessions/{id}/offer", post(api::offer))
        .route("/sessions/{id}/reset", post(api::reset))
        .route("/sessions/{id}/states/{slot}", post(api::save_state))
        .route("/sessions/{id}/states/{slot}/load", post(api::load_state))
        // For `navigator.sendBeacon` when a player closes the page: beacons can only POST.
        .route("/sessions/{id}/stop", post(api::stop_session))
        .fallback(|| async { error::ApiError::not_found("route") });

    let mut app = Router::new()
        .route("/health", get(ops::health))
        .route("/metrics", get(ops::metrics))
        .nest("/api", api)
        .with_state(state.clone());
    if let Some(dir) = &state.config.web_dir {
        app = app.fallback_service(ServeDir::new(dir).append_index_html_on_directories(true));
    }
    app.layer(middleware::from_fn(cache_headers)).layer(middleware::from_fn(ops::security_headers))
}

/// Astro fingerprints everything under `/_astro/`, so those are immutable; pages and the API
/// must always revalidate.
async fn cache_headers(request: Request, next: Next) -> Response {
    let immutable = request.uri().path().starts_with("/_astro/");
    let mut response = next.run(request).await;
    let value = if immutable { "public, max-age=31536000, immutable" } else { "no-cache" };
    response.headers_mut().entry(CACHE_CONTROL).or_insert(HeaderValue::from_static(value));
    response
}

fn default_worker() -> PathBuf {
    let name = format!("sovemu-worker{}", std::env::consts::EXE_SUFFIX);
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(&name)))
        .filter(|p| p.is_file())
        .unwrap_or_else(|| PathBuf::from(name))
}

pub async fn build_state(mut config: Config) -> Result<AppState> {
    let library = config.roms.clone().unwrap_or_else(|| config.data_dir.join("roms"));
    // `SOVEMU_MIRROR_SYSTEMS=` (empty) means all systems, not a system called "".
    config.mirror_systems.retain(|s| !s.trim().is_empty());
    for s in &mut config.mirror_systems {
        *s = s.trim().to_lowercase();
    }
    if config.library_mode == LibraryMode::Mirror {
        config.roms_dir = mirror::mirror_dir(&config.data_dir);
        config.source_dir = Some(library);
    } else {
        config.roms_dir = library;
    }
    std::fs::create_dir_all(&config.data_dir)
        .with_context(|| format!("creating {}", config.data_dir.display()))?;
    std::fs::create_dir_all(&config.roms_dir).ok();
    let db = db::open(&config.data_dir.join("sovemu.db")).await?;
    reconcile_users(&db, &config).await?;
    let worker_bin = config.worker.clone().unwrap_or_else(default_worker);
    let gpu = gpu::detect(&worker_bin, &config.gpu, &workers::worker_env()).await;
    tracing::info!("GPU probe:\n{}", gpu.summary());
    let workers = Workers::new(WorkerConfig {
        worker_bin,
        ports: config.udp_ports,
        bind_ip: config.udp_bind,
        advertise_ip: config.advertise_ip,
        system_dir: config.system.clone().unwrap_or_else(|| config.data_dir.join("system")),
        idle_timeout_secs: config.idle_timeout,
        video_kbps: config.video_kbps,
        audio_kbps: config.audio_kbps,
        start_timeout: std::time::Duration::from_secs(config.worker_start_timeout),
        rewind_seconds: config.rewind_seconds,
        rewind_max_mb: config.rewind_max_mb,
        fast_forward: config.fast_forward,
        keyframe_interval: config.keyframe_interval,
        gpu: gpu.worker_arg.clone(),
    });
    let cache_dir = config.data_dir.join("cache");
    let cache_bytes = match config.library_mode {
        LibraryMode::Cache => config.cache_gb.unwrap_or(20).saturating_mul(1 << 30),
        _ => 0,
    };
    match config.library_mode {
        LibraryMode::Direct => {
            tracing::info!("library {}: played directly", config.roms_dir.display())
        }
        LibraryMode::Cache => tracing::info!(
            "library {}: caching up to {} GiB in {}",
            config.roms_dir.display(),
            cache_bytes >> 30,
            cache_dir.display()
        ),
        LibraryMode::Mirror => tracing::info!(
            "library {}: mirrored into {}",
            config.source_dir.as_deref().unwrap_or(&config.roms_dir).display(),
            config.roms_dir.display()
        ),
    }
    let http = metadata::http_client();
    let screenscraper = match (&config.screenscraper_devid, &config.screenscraper_devpassword) {
        (Some(id), Some(pw)) if !id.is_empty() && !pw.is_empty() => Some(
            metadata::screenscraper::ScreenScraper::new(
                config.screenscraper_url.clone(),
                metadata::screenscraper::Credentials {
                    dev_id: id.clone(),
                    dev_password: pw.clone(),
                    user: config
                        .screenscraper_user
                        .clone()
                        .filter(|u| !u.is_empty())
                        .zip(config.screenscraper_password.clone()),
                },
                http.clone(),
            )
            .with_preferences(config.metadata_regions.clone(), config.metadata_languages.clone()),
        ),
        _ => None,
    };
    let thegamesdb = config.thegamesdb_key.clone().filter(|k| !k.is_empty()).map(|key| {
        metadata::thegamesdb::TheGamesDb::new(config.thegamesdb_url.clone(), key, http.clone())
    });
    let metadata = metadata::Metadata::new(
        config.data_dir.join("metadata"),
        screenscraper,
        thegamesdb,
        std::time::Duration::from_millis(config.screenscraper_delay_ms),
    );
    let login_throttle = ops::LoginThrottle::new(
        config.login_max_failures,
        std::time::Duration::from_secs(config.login_lockout_minutes * 60),
    );
    let copy_threads = config.copy_threads;
    Ok(AppState {
        db,
        config: Arc::new(config),
        workers,
        dummy_hash: auth::hash_password("x")?,
        login_throttle: Arc::new(login_throttle),
        cache: cache::Cache::new(cache_dir, cache_bytes, copy_threads),
        metadata,
        mirror: Arc::default(),
        gpu: Arc::new(gpu),
        core_info: Arc::default(),
    })
}

/// Brings the accounts in line with the environment: the admin and every declared user exist
/// with their declared passwords, and with `SOVEMU_USERS_PRUNE` nobody else does.
async fn reconcile_users(db: &SqlitePool, config: &Config) -> Result<()> {
    let mut declared = Vec::new();
    if let (Some(user), Some(password)) = (&config.admin_username, &config.admin_password) {
        auth::ensure_admin(db, user, password).await?;
        declared.push(user.clone());
    }
    for user in &config.users {
        auth::ensure_user(db, &user.username, &user.password, user.admin).await?;
        declared.push(user.username.clone());
    }
    if config.users_prune {
        if declared.is_empty() {
            anyhow::bail!(
                "SOVEMU_USERS_PRUNE is set but no users are declared: that would delete everyone"
            );
        }
        auth::prune_users(db, &declared).await?;
    }
    Ok(())
}

/// Scans the library in the background: a network library can take minutes, and the last
/// scan's games are already in the database meanwhile. In `mirror` mode, syncs first (scanning
/// what is already mirrored while it runs), then scans again.
fn spawn_initial_scan(state: &AppState) {
    spawn_periodic(state);
    let state = state.clone();
    tokio::spawn(async move {
        admin::scan_and_log(&state).await;
        if state.config.library_mode == LibraryMode::Mirror {
            admin::sync_then_scan(state.clone()).await;
        }
        admin::after_scan(&state).await;
    });
}

/// Rescans (and in `mirror` mode syncs) on the configured intervals, so the library keeps up
/// with the share on its own.
fn spawn_periodic(state: &AppState) {
    let every = |minutes: u64| (minutes > 0).then(|| std::time::Duration::from_secs(minutes * 60));
    if let Some(period) = every(state.config.scan_interval_minutes) {
        let state = state.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(period);
            tick.tick().await;
            loop {
                tick.tick().await;
                admin::scan_and_log(&state).await;
                admin::after_scan(&state).await;
            }
        });
    }
    if state.config.library_mode == LibraryMode::Mirror
        && let Some(period) = every(state.config.mirror_interval_minutes)
    {
        let state = state.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(period);
            tick.tick().await;
            loop {
                tick.tick().await;
                admin::sync_then_scan(state.clone()).await;
                admin::after_scan(&state).await;
            }
        });
    }
}

/// Tidies the environment before the settings are parsed:
/// - an empty `SOVEMU_*` variable means "not set" (a blank line in `.env`), so it is removed
///   rather than rejected as an invalid value;
/// - `SOVEMU_NAS_*` are compose's, for mounting the share; the server has no use for the NAS
///   password, so it does not keep it.
fn unset_empty_settings() {
    let empty: Vec<String> = std::env::vars_os()
        .filter_map(|(k, v)| {
            let k = k.into_string().ok()?;
            (k.starts_with("SOVEMU_") && (v.is_empty() || k.starts_with("SOVEMU_NAS_")))
                .then_some(k)
        })
        .collect();
    for key in empty {
        // SAFETY: called first thing in `main`, before the runtime or any other thread exists,
        // so nothing can be reading the environment concurrently.
        unsafe { std::env::remove_var(key) };
    }
}

/// Ctrl-C, or SIGTERM: what `docker stop` and Kubernetes send. Either way the server stops
/// every worker properly afterwards, so players' saves are written.
async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut term =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).ok();
        let term = async {
            match term.as_mut() {
                Some(t) => {
                    t.recv().await;
                }
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            () = term => {}
        }
    }
    #[cfg(not(unix))]
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("shutting down: stopping game sessions");
}

fn main() -> Result<()> {
    unset_empty_settings();
    tokio::runtime::Builder::new_multi_thread().enable_all().build()?.block_on(run())
}

async fn run() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();
    let mut config = Config::parse();
    config.resolve_secrets(|var| std::env::var(var).ok())?;
    let vars: Vec<(String, String)> = std::env::vars().collect();
    config.users = structured::users(&vars, |path| {
        std::fs::read_to_string(path)
            .map(|s| s.trim_end_matches(['\n', '\r']).to_owned())
            .with_context(|| format!("reading {path}"))
    })?;
    config.systems = structured::systems(&vars)?;
    let addr = config.http;
    let state = build_state(config).await?;
    tracing::info!("configuration:\n{}", state.config.summary());
    spawn_initial_scan(&state);
    let workers = state.workers.clone();
    if state.config.web_dir.is_none() {
        tracing::warn!("no --web-dir: serving the API only");
    }

    let listener =
        tokio::net::TcpListener::bind(addr).await.with_context(|| format!("binding {addr}"))?;
    let local = listener.local_addr()?;
    println!("http={local}");
    tracing::info!("SovEmu on http://{local}/");
    axum::serve(listener, router(state)).with_graceful_shutdown(shutdown_signal()).await?;
    workers.stop_all().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ice_server_config() {
        assert!(parse_ice_servers("[]").is_ok());
        assert!(parse_ice_servers(r#"[{"urls":"stun:stun.example:3478"}]"#).is_ok());
        assert!(parse_ice_servers(r#"[{"username":"x"}]"#).is_err());
        assert!(parse_ice_servers("{}").is_err());
        assert!(parse_ice_servers("nope").is_err());
    }

    #[test]
    fn secrets_come_from_files_and_are_redacted() {
        let file = std::env::temp_dir().join(format!("sovemu-secret-{}", std::process::id()));
        std::fs::write(&file, "s3cret\n").unwrap();
        let mut config = Config::parse_from([
            "sovemu-server",
            "--admin-username",
            "boss",
            "--thegamesdb-key",
            "",
        ]);
        let path = file.display().to_string();
        config
            .resolve_secrets(|var| (var == "SOVEMU_ADMIN_PASSWORD_FILE").then(|| path.clone()))
            .unwrap();
        assert_eq!(config.admin_password.as_deref(), Some("s3cret"), "trailing newline trimmed");
        assert_eq!(config.thegamesdb_key, None, "empty means unset");
        let summary = config.summary();
        assert!(summary.contains("admin_password=(set)"));
        assert!(!summary.contains("s3cret"));
        let missing = Config::parse_from(["sovemu-server"]).resolve_secrets(|v| {
            (v == "SOVEMU_METRICS_TOKEN_FILE").then(|| "/nonexistent/token".to_owned())
        });
        assert!(missing.is_err(), "a named secret file that cannot be read is an error");
        std::fs::remove_file(file).ok();
    }

    #[test]
    fn port_ranges() {
        assert_eq!(parse_ports("50000-50009"), Ok((50000, 50009)));
        assert_eq!(parse_ports("6000"), Ok((6000, 6000)));
        assert!(parse_ports("10-5").is_err());
        assert!(parse_ports("0-5").is_err());
        assert!(parse_ports("x").is_err());
    }
}
