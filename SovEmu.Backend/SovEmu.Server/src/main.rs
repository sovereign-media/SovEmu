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
#[cfg(test)]
mod http_tests;
mod library;
mod metadata;
mod mirror;
mod ops;
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
        .route("/sessions", post(api::start_session))
        .route("/sessions/current", get(api::current_session))
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
    let workers = Workers::new(WorkerConfig {
        worker_bin: config.worker.clone().unwrap_or_else(default_worker),
        ports: config.udp_ports,
        bind_ip: config.udp_bind,
        advertise_ip: config.advertise_ip,
        system_dir: config.system.clone().unwrap_or_else(|| config.data_dir.join("system")),
        idle_timeout_secs: config.idle_timeout,
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
        (Some(id), Some(pw)) if !id.is_empty() && !pw.is_empty() => {
            Some(metadata::screenscraper::ScreenScraper::new(
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
            ))
        }
        _ => None,
    };
    let thegamesdb = config.thegamesdb_key.clone().filter(|k| !k.is_empty()).map(|key| {
        metadata::thegamesdb::TheGamesDb::new(config.thegamesdb_url.clone(), key, http.clone())
    });
    let metadata =
        metadata::Metadata::new(config.data_dir.join("metadata"), screenscraper, thegamesdb);
    Ok(AppState {
        db,
        config: Arc::new(config),
        workers,
        dummy_hash: auth::hash_password("x")?,
        login_throttle: Arc::default(),
        cache: cache::Cache::new(cache_dir, cache_bytes),
        metadata,
        mirror: Arc::default(),
    })
}

/// Scans the library in the background: a network library can take minutes, and the last
/// scan's games are already in the database meanwhile. In `mirror` mode, syncs first (scanning
/// what is already mirrored while it runs), then scans again.
fn spawn_initial_scan(state: &AppState) {
    let state = state.clone();
    tokio::spawn(async move {
        admin::scan_and_log(&state).await;
        if state.config.library_mode == LibraryMode::Mirror {
            admin::sync_then_scan(state).await;
        }
    });
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();
    let config = Config::parse();
    let addr = config.http;
    let state = build_state(config).await?;
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
    axum::serve(listener, router(state))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
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
    fn port_ranges() {
        assert_eq!(parse_ports("50000-50009"), Ok((50000, 50009)));
        assert_eq!(parse_ports("6000"), Ok((6000, 6000)));
        assert!(parse_ports("10-5").is_err());
        assert!(parse_ports("0-5").is_err());
        assert!(parse_ports("x").is_err());
    }
}
