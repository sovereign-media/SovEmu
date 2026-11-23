//! `sovemu-server`: the SovEmu web client, its API, the ROM library and game sessions.
//!
//! One process serves `SovEmu.Web`'s static build and `/api/*` on one port. Each game runs in
//! its own `sovemu-worker session` process (see [`workers`]), which streams to the browser over
//! WebRTC on a UDP port of its own.

mod about;
mod activity;
mod admin;
mod api;
mod appearance;
mod assets;
mod attention;
mod auth;
mod backup;
mod capabilities;
mod cheats;
mod client_events;
mod client_ip;
mod codec;
mod collections;
mod configuration;
mod controllers;
mod controls;
mod cores;
mod db;
mod decode_times;
mod error;
mod failures;
mod graphql;
mod home;
#[cfg(test)]
mod http_tests;
mod jobs;
mod library;
mod logs;
mod metadata;
mod notices;
mod ops;
mod patches;
mod paths;
mod play_stats;
#[cfg(test)]
mod query_plans;
mod redact;
mod releases;
mod resources;
mod search;
mod settings;
mod storage;
mod streaming;
mod structured;
mod telemetry;
mod user_details;

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
use axum::routing::get;
use clap::Parser;
use sovemu_library::{bios, cache, decorations, mirror};
use sovemu_sessions::{encoders, gpu, workers};
use sqlx::SqlitePool;
use tower_http::services::{ServeDir, ServeFile};

use sovemu_sessions::workers::{WorkerConfig, Workers};

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

    /// BIOS files, one folder per system named like its ROM folder (`psx/`, `ps2/` …).
    /// Default: `<data-dir>/bios`.
    #[arg(long = "bios-dir", env = "SOVEMU_BIOS_DIR")]
    bios: Option<PathBuf>,

    /// Directory of libretro cores (`<name>_libretro.so`).
    #[arg(long, env = "SOVEMU_CORES_DIR", default_value = "cores")]
    pub cores_dir: PathBuf,

    /// The built web client (`SovEmu.Web/dist`).
    #[arg(long, env = "SOVEMU_WEB_DIR")]
    pub web_dir: Option<PathBuf>,

    /// Decoration (bezel) folders in Batocera's layout, besides `<data>/decorations` and the
    /// built-in one: a `decorations` folder on the NAS, say. Comma-separated; for the same file
    /// the first folder wins, and the built-in set comes last.
    #[arg(long, env = "SOVEMU_DECORATIONS_DIRS", value_delimiter = ',')]
    pub decorations_dirs: Vec<PathBuf>,

    /// The built-in decorations (the image sets `/opt/sovemu/decorations`). Unset: they are
    /// drawn into `<data>/cache/decorations` at startup.
    #[arg(long, env = "SOVEMU_DECORATIONS_BUILTIN")]
    pub decorations_builtin: Option<PathBuf>,

    /// The decoration set games are framed with: `sovemu` (built in), a set from the
    /// decoration folders, or `none`. Per system: `SOVEMU_SYSTEMS__<id>__DECORATION`.
    #[arg(long, env = "SOVEMU_DECORATION", default_value = decorations::BUILTIN)]
    pub decoration: String,

    /// The background behind every screen, for everyone: a background's name (`dither`,
    /// `lattice`, `screens`, or a file's name in the background folders), `random` (a different
    /// one each visit) or `none`. Unset: the admin's choice on the Appearance page, else `random`.
    #[arg(long, env = "SOVEMU_BACKGROUND")]
    pub background: Option<String>,

    /// Background folders besides `<data>/backgrounds`: images (WebP, PNG, JPEG) named as they
    /// are offered (`sunset-beach.png` is "Sunset beach"). Comma-separated; for the same name the
    /// first folder wins, and any wins over a built-in one.
    #[arg(long, env = "SOVEMU_BACKGROUNDS_DIRS", value_delimiter = ',')]
    pub backgrounds_dirs: Vec<PathBuf>,

    /// Draws the built-in decorations into this folder and exits (the image's build runs it).
    #[arg(long, hide = true)]
    pub write_decorations: Option<PathBuf>,

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

    /// Look games up in the LaunchBox Games Database dump (free, no account; about 110 MB,
    /// downloaded into `<data>/metadata/launchbox/`).
    #[arg(long, env = "SOVEMU_LAUNCHBOX", default_value_t = true, action = clap::ArgAction::Set)]
    pub launchbox: bool,
    #[arg(long, env = "SOVEMU_LAUNCHBOX_URL", default_value = sovemu_library::info::launchbox::DEFAULT_URL)]
    pub launchbox_url: String,
    /// Where the dump's image file names are served from (box art).
    #[arg(
        long,
        env = "SOVEMU_LAUNCHBOX_IMAGES_URL",
        default_value = sovemu_library::info::launchbox::DEFAULT_IMAGES_URL
    )]
    pub launchbox_images_url: String,
    /// Look for a newer LaunchBox dump when a metadata job starts and the last check is this
    /// many hours old (0: download it once, never again).
    #[arg(long, env = "SOVEMU_LAUNCHBOX_REFRESH_HOURS", default_value_t = 168)]
    pub launchbox_refresh_hours: u64,

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
    #[arg(long, env = "SOVEMU_SCREENSCRAPER_URL", default_value = sovemu_library::info::screenscraper::DEFAULT_BASE)]
    pub screenscraper_url: String,

    /// TheGamesDB API key (request one at forums.thegamesdb.net).
    #[arg(long, env = "SOVEMU_THEGAMESDB_KEY")]
    pub thegamesdb_key: Option<String>,
    #[arg(long, env = "SOVEMU_THEGAMESDB_URL", default_value = sovemu_library::info::thegamesdb::DEFAULT_BASE)]
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

    /// How log lines are written to stderr: `text` for a person, `json` (one object per line,
    /// with the request's or session's fields) for a log collector. Workers log the same way.
    #[arg(long, env = "SOVEMU_LOG_FORMAT", value_enum, default_value_t = telemetry::LogFormat::Text)]
    pub log_format: telemetry::LogFormat,

    /// An OpenTelemetry collector's OTLP/HTTP address (`http://otel-collector:4318`): traces
    /// go to `/v1/traces` and metrics to `/v1/metrics`. Unset: nothing is exported. The
    /// standard `OTEL_*` variables (service name, resource attributes, headers, export
    /// interval) apply on top.
    #[arg(long, env = "SOVEMU_OTEL_ENDPOINT")]
    pub otel_endpoint: Option<String>,

    /// The admin account, created at startup if missing; its password is kept in step with
    /// this setting. With it set there is no first-run setup page.
    #[arg(long, env = "SOVEMU_ADMIN_USERNAME")]
    pub admin_username: Option<String>,
    #[arg(long, env = "SOVEMU_ADMIN_PASSWORD")]
    pub admin_password: Option<String>,

    /// The key that signs the file URLs the API hands out (artwork, videos, screenshots). Unset:
    /// one is generated on first run and kept in `<data>/server.json`. Changing it invalidates
    /// every URL handed out until clients ask again.
    #[arg(long, env = "SOVEMU_ASSET_KEY")]
    pub asset_key: Option<String>,

    /// The secret the server's sign-in (OpenID Connect) key is derived from: it signs the ID
    /// tokens apps receive. Unset: one is generated on first run and kept in
    /// `<data>/server.json`.
    #[arg(long, env = "SOVEMU_TOKEN_KEY")]
    pub token_key: Option<String>,

    /// The addresses people reach the server at, comma-separated
    /// (`https://games.example.com,http://192.168.1.10:8080`). Each is an issuer of sign-in
    /// tokens, and a browser can sign in only at one of them. Unset: `localhost`, `127.0.0.1`
    /// and `SOVEMU_ADVERTISE_IP` on the HTTP port.
    #[arg(long, env = "SOVEMU_PUBLIC_URL", value_delimiter = ',')]
    pub public_url: Vec<String>,

    /// Whether the apps may sign in with a username and password (`POST /oauth/password`),
    /// beside signing in with a code shown on the screen (the device grant).
    #[arg(long, env = "SOVEMU_PASSWORD_SIGN_IN", default_value_t = true, action = clap::ArgAction::Set)]
    pub password_sign_in: bool,

    /// Whether what goes wrong on a client (a connection that failed, a stalled picture, a
    /// screen that crashed), sent with `reportClientEvents`, is logged and kept for the admin
    /// Client errors page. Off, they are dropped.
    #[arg(long, env = "SOVEMU_CLIENT_EVENTS", default_value_t = true, action = clap::ArgAction::Set)]
    pub client_events: bool,

    /// How long an app's access token lasts, in minutes, before it refreshes it. Signing out
    /// and role changes apply at once regardless: every request checks its token.
    #[arg(long, env = "SOVEMU_ACCESS_TOKEN_MINUTES", default_value_t = 60)]
    pub access_token_minutes: u32,

    /// How long a just-used refresh token is still honoured, in seconds: two tabs or requests
    /// refreshing at once get the same new tokens. Presenting it after this ends the sign-in (a
    /// copied token). 0: strictly once.
    #[arg(long, env = "SOVEMU_REFRESH_RETRY_SECONDS", default_value_t = 30)]
    pub refresh_retry_seconds: u64,

    /// How long a signed file URL stays valid, in hours (rounded to the hour, so URLs are
    /// cacheable).
    #[arg(long, env = "SOVEMU_ASSET_URL_HOURS", default_value_t = 24)]
    pub asset_url_hours: u64,

    /// Rescan the library every this many minutes (0: only at startup).
    #[arg(long, env = "SOVEMU_SCAN_INTERVAL_MINUTES", default_value_t = 60)]
    pub scan_interval_minutes: u64,

    /// `mirror` mode: sync every this many minutes (0: only at startup).
    #[arg(long, env = "SOVEMU_MIRROR_INTERVAL_MINUTES", default_value_t = 360)]
    pub mirror_interval_minutes: u64,

    /// Look up game information for new games after every scan.
    #[arg(long, env = "SOVEMU_METADATA_AUTO", default_value_t = true, action = clap::ArgAction::Set)]
    pub metadata_auto: bool,

    /// Video bitrate a game stream starts at, kbit/s. It then follows the connection (up
    /// while there is headroom, down on congestion) and the client (down while its decoder
    /// falls behind), between the floor and the ceiling.
    #[arg(long, env = "SOVEMU_VIDEO_KBPS", default_value_t = 5000)]
    pub video_kbps: u32,

    /// The lowest video bitrate a stream adapts down to, kbit/s.
    #[arg(long, env = "SOVEMU_VIDEO_MIN_KBPS", default_value_t = 300)]
    pub video_min_kbps: u32,

    /// The highest video bitrate a stream adapts up to, kbit/s (never below the starting rate).
    #[arg(long, env = "SOVEMU_VIDEO_MAX_KBPS", default_value_t = 12000)]
    pub video_max_kbps: u32,

    /// Video codec for game streams: `auto`, `vp8` or `h264`. `auto` chooses for each client
    /// as it connects: the cheapest of the codecs this server encodes and the client decodes,
    /// by what the startup encoder test measured here and what the client's decoder measured
    /// last time (or, until it has, what it declares). A client that sends no capabilities
    /// gets VP8. Naming a codec sends it to every client.
    #[arg(long, env = "SOVEMU_VIDEO_CODEC", default_value = "auto",
          value_parser = ["auto", "h264", "vp8"])]
    pub video_codec: String,

    /// H.264 encoder: `auto` (the GPU's, VA-API or NVENC, else x264), `vaapi`, `nvenc` or
    /// `software` (x264). One that does not open, or holds frames back, falls back to x264.
    #[arg(long, env = "SOVEMU_VIDEO_ENCODER", default_value = "auto",
          value_parser = ["auto", "vaapi", "nvenc", "software"])]
    pub video_encoder: String,

    /// Opus audio bitrate for game streams, kbit/s.
    #[arg(long, env = "SOVEMU_AUDIO_KBPS", default_value_t = 128)]
    pub audio_kbps: u32,

    /// How many days a sign-in lasts before signing in again (an app's, from signing in:
    /// refreshing does not extend it; the browser's at the server). 0: until signed out.
    #[arg(long, env = "SOVEMU_SESSION_DAYS", default_value_t = 90)]
    pub session_days: u32,

    /// Failed sign-ins per username before it is locked out (0: never).
    #[arg(long, env = "SOVEMU_LOGIN_MAX_FAILURES", default_value_t = sovemu_identity::MAX_FAILURES)]
    pub login_max_failures: u32,

    /// How long a locked-out username waits, and the window failures are counted in.
    #[arg(long, env = "SOVEMU_LOGIN_LOCKOUT_MINUTES", default_value_t = 5)]
    pub login_lockout_minutes: u64,

    /// Failed sign-ins from one address, across usernames, before it is locked out (0: never).
    /// Behind a reverse proxy, list it in `SOVEMU_TRUSTED_PROXIES`, or every client is the proxy.
    #[arg(long, env = "SOVEMU_LOGIN_MAX_FAILURES_PER_IP", default_value_t = sovemu_identity::MAX_FAILURES_PER_IP)]
    pub login_max_failures_per_ip: u32,

    /// Reverse proxies whose `X-Forwarded-For` is believed (addresses or ranges,
    /// comma-separated, e.g. `10.0.0.0/8,172.16.0.0/12`). Default: none, so the client is the
    /// connection's address.
    #[arg(long, env = "SOVEMU_TRUSTED_PROXIES", value_delimiter = ',')]
    pub trusted_proxies: Vec<client_ip::Network>,

    /// Database upkeep every this many minutes: expired sign-ins deleted, the query planner's
    /// statistics refreshed and the write-ahead log folded back into the database (0: never).
    #[arg(long, env = "SOVEMU_MAINTENANCE_INTERVAL_MINUTES", default_value_t = 60)]
    pub maintenance_interval_minutes: u64,

    /// Where backups are kept (default `<data>/backups`). A backup holds the database, the
    /// signing keys and every save: keep it somewhere as safe as they are.
    #[arg(long, env = "SOVEMU_BACKUP_DIR")]
    pub backup_dir: Option<PathBuf>,

    /// A backup every this many hours (0: none on a schedule; the admin's button still makes one).
    #[arg(long, env = "SOVEMU_BACKUP_INTERVAL_HOURS", default_value_t = 0)]
    pub backup_interval_hours: u64,

    /// Backups kept, the newest (0: every one).
    #[arg(long, env = "SOVEMU_BACKUP_KEEP", default_value_t = 7)]
    pub backup_keep: u32,

    /// A backup to restore at startup, before the database opens. What was there is moved into
    /// `<data>/before-restore-<time>/`, and each backup is restored once, however often the
    /// server restarts with this set.
    #[arg(long, env = "SOVEMU_RESTORE_FROM")]
    pub restore_from: Option<PathBuf>,

    /// Days of background-job history kept (each scan, sync, lookup and upkeep run, on the
    /// admin Jobs page); maintenance deletes older runs (0: kept for ever).
    #[arg(long, env = "SOVEMU_JOB_HISTORY_DAYS", default_value_t = 30)]
    pub job_history_days: u32,

    /// Days of the activity log kept (sign-ins, games started and ended, admin changes, on the
    /// admin Activity page); maintenance deletes older entries (0: kept for ever).
    #[arg(long, env = "SOVEMU_ACTIVITY_HISTORY_DAYS", default_value_t = 90)]
    pub activity_history_days: u32,

    /// Lines of the server's log kept in memory for the admin Logs page (its own and its game
    /// sessions'); 0 keeps none and turns the page's view off. Lines go to stderr regardless.
    #[arg(long, env = "SOVEMU_LOG_BUFFER_LINES", default_value_t = 2000)]
    pub log_buffer_lines: usize,

    /// Days of client errors kept (what the apps reported, on the admin Client errors page);
    /// maintenance deletes older ones (0: kept for ever).
    #[arg(long, env = "SOVEMU_CLIENT_EVENT_DAYS", default_value_t = 30)]
    pub client_event_days: u32,

    /// Seconds between readings of the container's CPU and memory, for the admin Resources page
    /// and the OpenTelemetry gauges (0: not read).
    #[arg(long, env = "SOVEMU_RESOURCE_SAMPLE_SECONDS", default_value_t = 5)]
    pub resource_sample_seconds: u32,

    /// Days of CPU and memory history kept (a row per minute); maintenance deletes older rows
    /// (0: kept for ever).
    #[arg(long, env = "SOVEMU_RESOURCE_HISTORY_DAYS", default_value_t = 30)]
    pub resource_history_days: u32,

    /// Delete accounts not declared in `SOVEMU_USERS__*` (or `SOVEMU_ADMIN_USERNAME`) at
    /// startup, so the environment is the whole list of users.
    #[arg(long, env = "SOVEMU_USERS_PRUNE")]
    pub users_prune: bool,

    /// How long a worker may take to load a core and game, seconds.
    #[arg(long, env = "SOVEMU_WORKER_START_TIMEOUT", default_value_t = 120)]
    pub worker_start_timeout: u64,

    /// How long a worker may take to write its saves and exit when its session ends, seconds,
    /// before it is killed. A 3D core (PCSX2) can take several seconds to tear down.
    #[arg(long, env = "SOVEMU_WORKER_STOP_TIMEOUT", default_value_t = 15)]
    pub worker_stop_timeout: u64,

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

    /// Testing only: throw away this share (percent) of every game's outgoing video packets,
    /// as a lossy link would, to see how a stream recovers. Audio and the connection's own
    /// packets always go. Leave it at 0.
    #[arg(long, env = "SOVEMU_DEBUG_DROP_VIDEO_PERCENT", default_value_t = 0.0)]
    pub debug_drop_video_percent: f64,

    /// Testing only: hold each of every game's outgoing video packets this many milliseconds,
    /// as a distant client's link would, so a retransmission arrives late. Leave it at 0.
    #[arg(long, env = "SOVEMU_DEBUG_DELAY_VIDEO_MS", default_value_t = 0)]
    pub debug_delay_video_ms: u32,

    /// Seconds between each game's `session health` log line while someone is playing (frame
    /// rate, overrun ticks, encode time, bitrate, input); 0 turns them off. Every game still
    /// logs a `session summary` when it ends.
    #[arg(long, env = "SOVEMU_SESSION_HEALTH_SECS", default_value_t = 10)]
    pub session_health_secs: u32,

    /// How many failed games (a load error, a crash, a worker that had to be killed) the admin
    /// Sessions page keeps, with the core's last log lines; 0 keeps none. In memory: the log
    /// has every one.
    #[arg(long, env = "SOVEMU_SESSION_FAILURES", default_value_t = 50)]
    pub session_failures: usize,

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

    /// Box art downloaded at once by the background job that follows a lookup.
    #[arg(long, env = "SOVEMU_ARTWORK_DOWNLOADS", default_value_t = 4)]
    pub artwork_downloads: usize,

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

    /// Default controller mappings declared as `SOVEMU_CONTROLLERS__<n>__ID` / `__BINDINGS` /
    /// `__NAME`, which win over the admin page's.
    #[arg(skip)]
    pub controllers: Vec<controllers::ControllerSpec>,

    /// Shared collections declared as `SOVEMU_COLLECTIONS__<n>__NAME` / `__GAMES__<m>`, kept in
    /// step after each scan and locked on the admin page.
    #[arg(skip)]
    pub collections: Vec<structured::CollectionSpec>,

    /// Resolved from `roms` after parsing.
    #[arg(skip)]
    pub roms_dir: PathBuf,

    /// Where each value came from, for the startup log and the admin Configuration page.
    #[arg(skip)]
    pub(crate) given: configuration::Given,
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
    "SOVEMU_ASSET_KEY",
    "SOVEMU_SCREENSCRAPER_DEVPASSWORD",
    "SOVEMU_SCREENSCRAPER_PASSWORD",
    "SOVEMU_THEGAMESDB_KEY",
    "SOVEMU_TOKEN_KEY",
];

impl Config {
    /// Parses the command line and the environment, keeping where each value came from.
    pub fn parse_with_sources<I, T>(args: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<std::ffi::OsString> + Clone,
    {
        use clap::{CommandFactory, FromArgMatches};
        let matches = Self::command().get_matches_from(args);
        let mut config = Self::from_arg_matches(&matches).unwrap_or_else(|e| e.exit());
        config.given = configuration::Given::from_matches(&matches);
        config
    }

    /// The addresses sign-in works at: `SOVEMU_PUBLIC_URL`, else this host's own on the HTTP
    /// port.
    pub fn public_urls(&self) -> Vec<String> {
        if !self.public_url.is_empty() {
            return self
                .public_url
                .iter()
                .map(|u| u.trim().trim_end_matches('/').to_owned())
                .collect();
        }
        let port = self.http.port();
        let mut urls = vec![format!("http://localhost:{port}"), format!("http://127.0.0.1:{port}")];
        if let Some(ip) = self.advertise_ip.filter(|ip| !ip.is_loopback() && !ip.is_unspecified()) {
            let host = if ip.is_ipv6() { format!("[{ip}]") } else { ip.to_string() };
            urls.push(format!("http://{host}:{port}"));
        }
        urls
    }

    /// The decoration set a system's games are framed with (`none`: no frame).
    pub fn decoration_for(&self, system: &str) -> &str {
        self.systems.get(system).and_then(|s| s.decoration.as_deref()).unwrap_or(&self.decoration)
    }

    /// Where everything lives in the data folder.
    pub fn paths(&self) -> paths::DataPaths {
        paths::DataPaths::new(&self.data_dir)
    }

    /// The decoration folders, first wins: the configured ones, `<data>/decorations`, then the
    /// built-in one.
    /// The background folders, first wins: the configured ones, then `<data>/backgrounds`.
    pub fn background_dirs(&self) -> Vec<PathBuf> {
        let mut dirs = self.backgrounds_dirs.clone();
        dirs.push(self.paths().backgrounds());
        dirs
    }

    pub fn decoration_dirs(&self) -> Vec<PathBuf> {
        let mut dirs = self.decorations_dirs.clone();
        dirs.push(self.paths().decorations());
        dirs.push(self.builtin_decorations());
        dirs
    }

    pub fn builtin_decorations(&self) -> PathBuf {
        self.decorations_builtin
            .clone()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| self.paths().builtin_decorations())
    }

    /// Where BIOS files live: `<bios_dir>/<system>/`.
    pub fn bios_dir(&self) -> PathBuf {
        // `SOVEMU_BIOS_DIR=` (empty) means the default, not the working directory.
        self.bios
            .clone()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| self.paths().bios())
    }

    /// Where backups are kept: `SOVEMU_BACKUP_DIR`, else `<data>/backups`.
    pub fn backup_dir(&self) -> PathBuf {
        self.backup_dir
            .clone()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| self.paths().backups())
    }

    /// A secret's value, by its variable.
    fn secret(&self, var: &str) -> Option<&str> {
        match var {
            "SOVEMU_ADMIN_PASSWORD" => &self.admin_password,
            "SOVEMU_ASSET_KEY" => &self.asset_key,
            "SOVEMU_TOKEN_KEY" => &self.token_key,
            "SOVEMU_SCREENSCRAPER_DEVPASSWORD" => &self.screenscraper_devpassword,
            "SOVEMU_SCREENSCRAPER_PASSWORD" => &self.screenscraper_password,
            "SOVEMU_THEGAMESDB_KEY" => &self.thegamesdb_key,
            _ => return None,
        }
        .as_deref()
    }

    fn secret_mut(&mut self, var: &str) -> Option<&mut Option<String>> {
        Some(match var {
            "SOVEMU_ADMIN_PASSWORD" => &mut self.admin_password,
            "SOVEMU_ASSET_KEY" => &mut self.asset_key,
            "SOVEMU_TOKEN_KEY" => &mut self.token_key,
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
                self.given.from_files.push(var);
            }
        }
        Ok(())
    }

    /// The effective settings, grouped, secrets redacted: logged at startup so a deployment's
    /// configuration can be checked without guessing (`configuration::summary`).
    pub fn summary(&self) -> String {
        configuration::summary(self)
    }
}

/// Moves copies from where the cache kept them before it had a folder of its own (beside the
/// disc playlists and the built-in decorations, which its eviction then deleted) into
/// `cache/games`, so nothing is copied again. A rename: the same filesystem.
fn move_legacy_cache(paths: &paths::DataPaths) {
    for old in paths.legacy_cache_kinds() {
        let Some(kind) = old.file_name() else { continue };
        let new = paths.cache_games().join(kind);
        if !old.is_dir() || new.exists() {
            continue;
        }
        let moved =
            std::fs::create_dir_all(paths.cache_games()).and_then(|()| std::fs::rename(&old, &new));
        match moved {
            Ok(()) => tracing::info!(from = %old.display(), to = %new.display(), "cache moved"),
            Err(e) => tracing::warn!(from = %old.display(), error = %e, "cache not moved"),
        }
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
    pub cache: Arc<cache::Cache>,
    pub metadata: Arc<metadata::Metadata>,
    pub mirror: Arc<std::sync::Mutex<mirror::SyncStatus>>,
    /// Where 3D systems render, decided at startup.
    pub gpu: Arc<gpu::GpuStatus>,
    /// What the startup encoder test found.
    pub encoders: Arc<encoders::EncoderTest>,
    /// What each core offers as settings, probed once.
    pub core_info: Arc<settings::CoreInfoCache>,
    /// The GraphQL schema (cheap to clone: shared).
    pub(crate) schema: graphql::ApiSchema,
    /// Signs the file URLs the API hands out, and checks them on `/media/…`.
    pub assets: Arc<sovemu_assets::AssetSigner>,
    /// The decoration sets found, listed again after each library scan.
    pub decorations: Arc<std::sync::RwLock<Arc<decorations::Decorations>>>,
    /// The backgrounds and system photos found, listed again after each library scan.
    pub(crate) appearance: Arc<std::sync::RwLock<Arc<appearance::Appearance>>>,
    /// Accounts and sign-in (`sovemu_identity`).
    pub identity: sovemu_identity::Identity,
    /// Each user's allowance of client events (`client_events`).
    pub(crate) client_events: Arc<client_events::ClientEvents>,
    /// Background jobs: scans, syncs, lookups, upkeep (`jobs`).
    pub(crate) jobs: jobs::Jobs,
    /// The host's facts and the container's CPU and memory readings (`resources`).
    pub(crate) resources: Arc<resources::Monitor>,
    /// The data folder's last measurement, for the Storage page (`storage`).
    pub(crate) storage: Arc<storage::StorageCache>,
    /// Games per system, counted when the library changes (`library`).
    pub(crate) game_counts: Arc<library::GameCounts>,
    /// The most recent games that failed, for the Sessions page (`failures`).
    pub(crate) failures: Arc<failures::Failures>,
    /// Who did what, and when (`activity`).
    pub(crate) activity: activity::Activity,
    /// When this server process started (Unix seconds), for its uptime.
    pub(crate) started_at: f64,
    /// The server's recent log, when `SOVEMU_LOG_BUFFER_LINES` keeps any (`logs`).
    pub(crate) logs: Option<Arc<logs::LogBuffer>>,
}

impl AppState {
    /// The decoration sets as last listed.
    pub fn decorations(&self) -> Arc<decorations::Decorations> {
        self.decorations.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// The backgrounds and system photos as last listed.
    pub(crate) fn appearance(&self) -> Arc<appearance::Appearance> {
        self.appearance.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Lists the background and photo folders again (an image may have been added).
    pub(crate) fn reload_appearance(&self) {
        let fresh = Arc::new(appearance::Appearance::load(
            &self.config.background_dirs(),
            &self.config.paths().system_photos(),
        ));
        *self.appearance.write().unwrap_or_else(|e| e.into_inner()) = fresh;
    }

    /// Lists the decoration folders again (a set may have been added).
    pub fn reload_decorations(&self) {
        let fresh = Arc::new(decorations::Decorations::load(&self.config.decoration_dirs()));
        *self.decorations.write().unwrap_or_else(|e| e.into_inner()) = fresh;
    }
}

impl std::fmt::Debug for AppState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppState").finish_non_exhaustive()
    }
}

pub fn router(state: AppState) -> Router {
    // The API is GraphQL. Files (artwork, videos, save screenshots) are plain HTTP, at URLs
    // the API hands out; so are sign-in (OpenID Connect defines its own routes) and the health
    // probe. Every route runs in a traced `request` span
    // (the web client's static files, the fallback, do not).
    let mut app = Router::new()
        .route("/graphql", get(graphql::graphiql).post(graphql::handler))
        // Files, at the signed URLs the API hands out (assets.rs): no cookie, the signature says
        // whose they are.
        .merge(
            Router::new()
                .route("/media/games/{id}/{kind}", get(api::game_media))
                .route("/media/games/{id}/saves/{slot}", get(api::state_screenshot))
                .route("/media/decorations/{set}/{*path}", get(api::decoration_file))
                .route("/media/backgrounds/{id}", get(appearance::background_file))
                .route("/media/systems/{id}", get(appearance::photo_file))
                .route("/media/backups/{name}", get(admin::backup_file))
                .route_layer(middleware::from_fn_with_state(
                    state.clone(),
                    assets::require_signature,
                )),
        )
        .route("/health", get(ops::health))
        .with_state(state.clone())
        // Sign-in: OpenID Connect discovery, `/oauth/…` and the device page (sovemu_identity).
        .merge(state.identity.router())
        .route_layer(middleware::from_fn(telemetry::trace_request))
        .route_layer(middleware::from_fn_with_state(state.clone(), client_ip::remember));
    // The web client is one page (`index.html`) that routes its screens itself, so every other
    // path is answered with it; its fingerprinted assets are served as files, a missing one 404.
    if let Some(dir) = &state.config.web_dir {
        let page = ServeFile::new(dir.join("index.html"));
        app = app.nest_service("/assets", ServeDir::new(dir.join("assets"))).fallback_service(
            ServeDir::new(dir).append_index_html_on_directories(true).fallback(page),
        );
    }
    app.layer(middleware::from_fn(cache_headers))
        .layer(middleware::from_fn(ops::security_headers))
        .layer(ops::compression())
}

/// Vite fingerprints everything under `/assets/`, so those are immutable; the page and the API
/// must always revalidate.
async fn cache_headers(request: Request, next: Next) -> Response {
    let immutable = request.uri().path().starts_with("/assets/");
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
    let paths = config.paths();
    let library = config.roms.clone().unwrap_or_else(|| paths.roms());
    // `SOVEMU_MIRROR_SYSTEMS=` (empty) means all systems, not a system called "".
    config.mirror_systems.retain(|s| !s.trim().is_empty());
    for s in &mut config.mirror_systems {
        *s = s.trim().to_lowercase();
    }
    if config.library_mode == LibraryMode::Mirror {
        config.roms_dir = paths.mirror();
        config.source_dir = Some(library);
    } else {
        config.roms_dir = library;
    }
    std::fs::create_dir_all(&config.data_dir)
        .with_context(|| format!("creating {}", config.data_dir.display()))?;
    std::fs::create_dir_all(&config.roms_dir).ok();
    bios::prepare(&config.bios_dir());
    // A restore replaces the database, so it comes before the database opens.
    if let Some(file) = config.restore_from.as_ref().filter(|p| !p.as_os_str().is_empty()) {
        match backup::restore(&paths, file)? {
            backup::Restored::Done { aside } => tracing::warn!(
                backup = %file.display(),
                replaced = %aside.display(),
                "restored a backup; what it replaced is kept"
            ),
            backup::Restored::Already => tracing::info!(
                backup = %file.display(),
                "this backup was restored already; SOVEMU_RESTORE_FROM can be removed"
            ),
        }
    }
    let db = db::open(&paths.database()).await?;
    let activity = activity::Activity::new(db.clone());
    let worker_bin = config.worker.clone().unwrap_or_else(default_worker);
    let gpu = gpu::detect(&worker_bin, &config.gpu, &workers::worker_env()).await;
    tracing::info!(summary = %gpu.summary(), "GPU probe");
    // Every video encoder, run and timed before the server listens: no game starts without it.
    let encoders = encoders::test(&worker_bin, &gpu.worker_arg, &workers::worker_env()).await;
    tracing::info!(
        took_ms = encoders.took_ms,
        working = encoders.encoders.iter().filter(|e| e.works).count(),
        tried = encoders.encoders.len(),
        summary = %format!("\n{}", encoders.summary()),
        "encoder test"
    );
    let session_failures = Arc::new(failures::Failures::new(config.session_failures));
    // Told as sessions start and end, and of each worker's log lines.
    let mut observers: Vec<Arc<dyn sovemu_sessions::Observer>> = vec![
        Arc::new(telemetry::SessionMetrics),
        session_failures.clone(),
        Arc::new(activity.clone()),
        Arc::new(play_stats::PlayLog::new(db.clone())),
        Arc::new(decode_times::DecodeTimes::new(db.clone())),
    ];
    if let Some(buffer) = logs::installed() {
        observers.push(buffer);
    }
    let workers = Workers::new(WorkerConfig {
        worker_bin,
        ports: config.udp_ports,
        bind_ip: config.udp_bind,
        advertise_ip: config.advertise_ip,
        bios_dir: config.bios_dir(),
        idle_timeout_secs: config.idle_timeout,
        video_kbps: config.video_kbps,
        video_min_kbps: config.video_min_kbps,
        video_max_kbps: config.video_max_kbps,
        audio_kbps: config.audio_kbps,
        video_codec: config.video_codec.clone(),
        video_encoder: config.video_encoder.clone(),
        start_timeout: std::time::Duration::from_secs(config.worker_start_timeout),
        stop_timeout: std::time::Duration::from_secs(config.worker_stop_timeout),
        rewind_seconds: config.rewind_seconds,
        rewind_max_mb: config.rewind_max_mb,
        fast_forward: config.fast_forward,
        keyframe_interval: config.keyframe_interval,
        drop_video_percent: config.debug_drop_video_percent,
        delay_video_ms: config.debug_delay_video_ms,
        health_interval_secs: config.session_health_secs,
        gpu: gpu.worker_arg.clone(),
        log_format: config.log_format.as_arg(),
        observer: Arc::new(sovemu_sessions::Observers(observers)),
    });
    let cache_dir = paths.cache_games();
    move_legacy_cache(&paths);
    let cache_bytes = match config.library_mode {
        LibraryMode::Cache => config.cache_gb.unwrap_or(20).saturating_mul(1 << 30),
        _ => 0,
    };
    match config.library_mode {
        LibraryMode::Direct => {
            tracing::info!(path = %config.roms_dir.display(), "library played directly")
        }
        LibraryMode::Cache => tracing::info!(
            path = %config.roms_dir.display(),
            bytes = cache_bytes,
            cache = %cache_dir.display(),
            "library cached"
        ),
        LibraryMode::Mirror => tracing::info!(
            source = %config.source_dir.as_deref().unwrap_or(&config.roms_dir).display(),
            path = %config.roms_dir.display(),
            "library mirrored"
        ),
    }
    let http = sovemu_library::info::http_client();
    let screenscraper = match (&config.screenscraper_devid, &config.screenscraper_devpassword) {
        (Some(id), Some(pw)) if !id.is_empty() && !pw.is_empty() => Some(
            sovemu_library::info::screenscraper::ScreenScraper::new(
                config.screenscraper_url.clone(),
                sovemu_library::info::screenscraper::Credentials {
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
        sovemu_library::info::thegamesdb::TheGamesDb::new(
            config.thegamesdb_url.clone(),
            key,
            http.clone(),
        )
    });
    let launchbox = config.launchbox.then(|| {
        sovemu_library::info::launchbox::LaunchBox::new(
            config.launchbox_url.clone(),
            config.launchbox_images_url.clone(),
            paths.launchbox(),
            config.launchbox_refresh_hours,
            config.metadata_regions.clone(),
        )
    });
    let metadata = metadata::Metadata::new(
        &paths,
        launchbox,
        screenscraper,
        thegamesdb,
        std::time::Duration::from_millis(config.screenscraper_delay_ms),
        config.artwork_downloads,
    );
    let copy_threads = config.copy_threads;
    // The built-in set: drawn here unless the image already holds it (it does, once per version).
    if config.decorations_builtin.is_none()
        && let Err(e) = decorations::write_builtin(&config.builtin_decorations())
    {
        tracing::warn!(error = %e, "drawing the built-in decorations");
    }
    let decorations = Arc::new(decorations::Decorations::load(&config.decoration_dirs()));
    let appearance = Arc::new(appearance::Appearance::load(
        &config.background_dirs(),
        &config.paths().system_photos(),
    ));
    let asset_key = sovemu_assets::load_key(config.asset_key.as_deref(), &config.data_dir)?;
    let assets = Arc::new(sovemu_assets::AssetSigner::new(asset_key, config.asset_url_hours));
    let token_secret = sovemu_assets::load_secret(
        config.token_key.as_deref(),
        &config.data_dir,
        "tokenSigningKey",
        "token signing key",
    )?;
    let identity = sovemu_identity::Identity::new(
        db.clone(),
        sovemu_identity::Config {
            public_urls: config.public_urls(),
            token_secret,
            access_token_minutes: config.access_token_minutes,
            session_days: config.session_days,
            secure_cookies: config.secure_cookies,
            password_sign_in: config.password_sign_in,
            max_failures: config.login_max_failures,
            max_failures_per_ip: config.login_max_failures_per_ip,
            lockout: std::time::Duration::from_secs(config.login_lockout_minutes * 60),
            refresh_retry: std::time::Duration::from_secs(config.refresh_retry_seconds),
            observer: Arc::new(activity.clone()),
        },
    )
    .await?;
    reconcile_users(&identity, &config).await?;
    let jobs = jobs::Jobs::new(db.clone()).await?;
    let host = tokio::task::spawn_blocking(resources::host::read_system).await?;
    tracing::info!(
        cpu = host.cpu_model.as_deref().unwrap_or("unknown"),
        cpus = host.usable_cpus,
        memory_bytes = host.memory_bytes.unwrap_or_default(),
        features = %host.features.join(","),
        "host"
    );
    let resources = Arc::new(resources::Monitor::new(host, config.resource_sample_seconds));
    let core_info = Arc::new(settings::CoreInfoCache::from_manifest(&config.cores_dir));
    let game_counts = Arc::new(library::GameCounts::load(&db).await?);
    Ok(AppState {
        db,
        config: Arc::new(config),
        workers,
        cache: cache::Cache::new(cache_dir, cache_bytes, copy_threads),
        metadata,
        mirror: Arc::default(),
        gpu: Arc::new(gpu),
        encoders: Arc::new(encoders),
        core_info,
        schema: graphql::schema(),
        decorations: Arc::new(std::sync::RwLock::new(decorations)),
        appearance: Arc::new(std::sync::RwLock::new(appearance)),
        assets,
        identity,
        client_events: Arc::default(),
        storage: Arc::default(),
        game_counts,
        failures: session_failures,
        activity,
        logs: logs::installed(),
        started_at: client_events::now_unix() as f64,
        jobs,
        resources,
    })
}

/// Brings the accounts in line with the environment: the admin and every declared user exist
/// with their declared passwords, and with `SOVEMU_USERS_PRUNE` nobody else does.
async fn reconcile_users(identity: &sovemu_identity::Identity, config: &Config) -> Result<()> {
    if let (Some(user), Some(password)) = (&config.admin_username, &config.admin_password) {
        identity.ensure_user(user, password, auth::Role::Admin).await?;
    }
    for user in &config.users {
        identity.ensure_user(&user.username, &user.password, user.role).await?;
    }
    let declared = auth::declared_usernames(config);
    if config.users_prune {
        if declared.is_empty() {
            anyhow::bail!(
                "SOVEMU_USERS_PRUNE is set but no users are declared: that would delete everyone"
            );
        }
        identity.prune_users(&declared).await?;
    }
    Ok(())
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

/// How long shutdown waits for cancelled jobs to record it. A cancelled job stops at its next
/// await, so this is a bound, not a wait anyone tunes.
const JOBS_SHUTDOWN_GRACE: std::time::Duration = std::time::Duration::from_secs(5);

fn main() -> Result<()> {
    unset_empty_settings();
    let config = Config::parse_with_sources(std::env::args_os());
    if let Some(dir) = &config.write_decorations {
        decorations::write_builtin(dir).with_context(|| format!("writing {}", dir.display()))?;
        println!("decorations: {}", dir.join(decorations::BUILTIN).display());
        return Ok(());
    }
    let buffer = logs::install(config.log_buffer_lines);
    let telemetry = telemetry::init(config.log_format, config.otel_endpoint.as_deref(), buffer)?;
    let result =
        tokio::runtime::Builder::new_multi_thread().enable_all().build()?.block_on(run(config));
    // Outside the runtime: the exporters' HTTP client blocks while it flushes.
    telemetry.shutdown();
    result
}

async fn run(mut config: Config) -> Result<()> {
    config.resolve_secrets(|var| std::env::var(var).ok())?;
    let vars: Vec<(String, String)> = std::env::vars().collect();
    config.given.keep_environment(&vars);
    config.users = structured::users(&vars, |path| {
        std::fs::read_to_string(path)
            .map(|s| s.trim_end_matches(['\n', '\r']).to_owned())
            .with_context(|| format!("reading {path}"))
    })?;
    config.systems = structured::systems(&vars)?;
    config.controllers = structured::controllers(&vars)?;
    config.collections = structured::collections(&vars)?;
    let addr = config.http;
    let state = build_state(config).await?;
    tracing::info!(summary = %state.config.summary(), "configuration");
    let _gauges = telemetry::observe(&state);
    // Scans the library in the background: a network library can take minutes, and the last
    // scan's games are already in the database meanwhile.
    jobs::spawn(&state);
    let (workers, jobs) = (state.workers.clone(), state.jobs.clone());
    if state.config.web_dir.is_none() {
        tracing::warn!("no --web-dir: serving the API only");
    }

    let listener =
        tokio::net::TcpListener::bind(addr).await.with_context(|| format!("binding {addr}"))?;
    let local = listener.local_addr()?;
    println!("http={local}");
    tracing::info!(url = %format!("http://{local}/"), "SovEmu listening");
    // With each connection's address, for `client_ip` (sign-in throttling, request spans).
    let app = router(state).into_make_service_with_connect_info::<std::net::SocketAddr>();
    axum::serve(listener, app).with_graceful_shutdown(shutdown_signal()).await?;
    // Both at once: the workers write their players' saves while the jobs record that they
    // were cancelled.
    tokio::join!(workers.stop_all(), jobs.shutdown(JOBS_SHUTDOWN_GRACE));
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
        assert!(summary.contains("SOVEMU_ADMIN_PASSWORD=(set) (from file)"), "{summary}");
        assert!(!summary.contains("s3cret"));
        let missing = Config::parse_from(["sovemu-server"]).resolve_secrets(|v| {
            (v == "SOVEMU_THEGAMESDB_KEY_FILE").then(|| "/nonexistent/key".to_owned())
        });
        assert!(missing.is_err(), "a named secret file that cannot be read is an error");
        std::fs::remove_file(file).ok();
    }

    #[test]
    fn logs_and_telemetry_settings() {
        let config = Config::parse_from(["sovemu-server"]);
        assert_eq!(config.log_format, telemetry::LogFormat::Text);
        assert_eq!(config.otel_endpoint, None, "nothing is exported unless asked");
        let config = Config::parse_with_sources([
            "sovemu-server",
            "--log-format",
            "json",
            "--otel-endpoint",
            "http://collector:4318",
        ]);
        assert_eq!(config.log_format, telemetry::LogFormat::Json);
        let summary = config.summary();
        assert!(summary.contains("SOVEMU_LOG_FORMAT=json (command line)"), "{summary}");
        assert!(summary.contains("SOVEMU_OTEL_ENDPOINT=http://collector:4318/"), "{summary}");
        let config = Config::parse_with_sources([
            "sovemu-server",
            "--otel-endpoint",
            "https://user:t0k3n@collector:4318/otlp?api-key=k3y",
            "--ice-servers",
            r#"[{"urls":"turn:turn.example:3478","username":"u","credential":"pa55"}]"#,
        ]);
        let summary = config.summary();
        assert!(
            summary.contains("SOVEMU_OTEL_ENDPOINT=https://***@collector:4318/otlp?***"),
            "{summary}"
        );
        assert!(summary.contains("turn:turn.example:3478"), "{summary}");
        for secret in ["t0k3n", "k3y", "pa55"] {
            assert!(!summary.contains(secret), "{secret} in {summary}");
        }
        assert!(Config::try_parse_from(["sovemu-server", "--log-format", "xml"]).is_err());
    }

    #[test]
    fn copies_move_out_of_the_shared_cache_folder() {
        let root = std::env::temp_dir().join(format!("sovemu-legacy-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let paths = paths::DataPaths::new(&root);
        std::fs::create_dir_all(paths.cache().join("roms/nes")).unwrap();
        std::fs::write(paths.cache().join("roms/nes/a.nes"), [1u8]).unwrap();
        std::fs::create_dir_all(paths.disc_playlists()).unwrap();
        std::fs::write(paths.disc_playlists().join("g.m3u"), "x").unwrap();
        move_legacy_cache(&paths);
        assert!(paths.cache_games().join("roms/nes/a.nes").exists());
        assert!(!paths.cache().join("roms").exists());
        assert!(paths.disc_playlists().join("g.m3u").exists(), "playlists stay where they are");
        move_legacy_cache(&paths); // nothing left to move
        std::fs::remove_dir_all(root).ok();
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
