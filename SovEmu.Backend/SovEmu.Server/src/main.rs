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
mod relay;
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
    #[arg(long, env = "SovEmu__Http__Listen", default_value = "0.0.0.0:8080")]
    pub http: SocketAddr,

    /// Where SovEmu keeps its database, saves and BIOS files (`system/`).
    #[arg(long, env = "SovEmu__Paths__Data", default_value = "data")]
    pub data_dir: PathBuf,

    /// The ROM library: one folder per system (`nes/`, `snes/`, …). Default: `<data-dir>/roms`.
    #[arg(long = "roms-dir", env = "SovEmu__Paths__Library")]
    roms: Option<PathBuf>,

    /// BIOS files, one folder per system named like its ROM folder (`psx/`, `ps2/` …).
    /// Default: `<data-dir>/bios`.
    #[arg(long = "bios-dir", env = "SovEmu__Paths__Bios")]
    bios: Option<PathBuf>,

    /// Directory of libretro cores (`<name>_libretro.so`).
    #[arg(long, env = "SovEmu__Paths__Cores", default_value = "cores")]
    pub cores_dir: PathBuf,

    /// The built web client (`SovEmu.Web/dist`).
    #[arg(long, env = "SovEmu__Paths__Web")]
    pub web_dir: Option<PathBuf>,

    /// Decoration (bezel) folders in Batocera's layout, besides `<data>/artwork/decorations` and
    /// the built-in one: a `decorations` folder on the NAS, say. One per index (`__0`, `__1`); for
    /// the same file the first folder wins, and the built-in set comes last.
    #[arg(long, env = "SovEmu__Decorations__Directories", value_delimiter = ',')]
    pub decorations_dirs: Vec<PathBuf>,

    /// The built-in artwork, laid out as `<data>/artwork` (the image sets
    /// `/opt/sovemu/artwork`): backgrounds, system photos, the `sovemu` decorations and the
    /// ESRB icons. Unset: written into `<data>/cache/builtin-artwork` at startup.
    #[arg(long, env = "SovEmu__Paths__BuiltInArtwork")]
    pub artwork_builtin: Option<PathBuf>,

    /// The decoration set games are framed with: `sovemu` (built in), a set from the
    /// decoration folders, or `none`. Per system: `SovEmu__Systems__<id>__Decoration`.
    #[arg(long, env = "SovEmu__Decorations__Set", default_value = decorations::BUILTIN)]
    pub decoration: String,

    /// The background behind every screen, for everyone: a background's name (`dither`,
    /// `lattice`, `screens`, or a file's name in the background folders), `random` (a different
    /// one each visit) or `none`. Unset: the admin's choice on the Appearance page, else `random`.
    #[arg(long, env = "SovEmu__Appearance__Background")]
    pub background: Option<String>,

    /// Background folders besides `<data>/artwork/backgrounds`: images (WebP, PNG, JPEG) named as they
    /// are offered (`sunset-beach.png` is "Sunset beach"). One per index (`__0`, `__1`); for the same name
    /// the first folder wins, and any wins over a built-in one.
    #[arg(long, env = "SovEmu__Appearance__BackgroundDirectories", value_delimiter = ',')]
    pub backgrounds_dirs: Vec<PathBuf>,

    /// Writes the built-in artwork into this folder and exits (the image's build runs it, into
    /// `/opt/sovemu/artwork`).
    #[arg(long, hide = true)]
    pub write_artwork: Option<PathBuf>,

    /// The worker binary. Default: `sovemu-worker` next to this binary.
    #[arg(long, env = "SovEmu__Paths__Worker")]
    worker: Option<PathBuf>,

    /// UDP ports for game sessions, one per concurrent session: `FIRST-LAST`.
    #[arg(long, env = "SovEmu__Streaming__Udp__Ports", default_value = "50000-50009", value_parser = parse_ports)]
    pub udp_ports: (u16, u16),

    /// Address workers bind their UDP ports on.
    #[arg(long, env = "SovEmu__Streaming__Udp__Bind", default_value = "0.0.0.0")]
    pub udp_bind: IpAddr,

    /// The DSCP value every game's packets are marked with, so Wi-Fi access points (and
    /// routers that honour it) send them ahead of ordinary traffic: 34 (AF41, interactive
    /// video, the access point's video queue) by default, 46 for its voice queue, 0 for none.
    #[arg(long, env = "SovEmu__Streaming__Udp__Dscp", default_value_t = 34,
          value_parser = clap::value_parser!(u8).range(0..=63))]
    pub udp_dscp: u8,

    /// The IP address browsers send media to: in a container, the host's address.
    #[arg(long, env = "SovEmu__Streaming__AdvertiseAddress")]
    pub advertise_ip: Option<IpAddr>,

    /// How to use the library: `direct` (play from it as is: local disk or a share on the LAN),
    /// `cache` (a slow, remote share: keep what is played on local disk) or `mirror` (keep a
    /// full local copy, synced in the background, and play from that).
    #[arg(long, env = "SovEmu__Library__Mode", value_enum, default_value_t = LibraryMode::Direct)]
    pub library_mode: LibraryMode,

    /// `cache` mode: keep up to this many GiB on local disk (default 20 in that mode).
    #[arg(long, env = "SovEmu__Library__Cache__MaxGigabytes")]
    pub cache_gb: Option<u64>,

    /// `mirror` mode: only these systems, by id, one per index (`__0=nes`, `__1=snes`). Default:
    /// all.
    #[arg(long, env = "SovEmu__Library__Mirror__Systems", value_delimiter = ',')]
    pub mirror_systems: Vec<String>,

    /// The game-information sources, asked in index order (`__0`, `__1`): `fbneo` (FinalBurn
    /// Neo's DATs, arcade and Neo Geo names) and `screenscraper` (needs its developer
    /// credentials).
    #[arg(
        long,
        env = "SovEmu__Metadata__Sources",
        value_delimiter = ',',
        default_value = "fbneo,screenscraper"
    )]
    pub metadata_sources: Vec<String>,

    /// Look games up again once their information is this many days old, with the lookup that
    /// follows each scan (0: never; a game is looked up once).
    #[arg(long, env = "SovEmu__Metadata__RefreshDays", default_value_t = 0)]
    pub metadata_refresh_days: u32,

    /// ScreenScraper developer id and password (request them from screenscraper.fr).
    #[arg(long, env = "SovEmu__Metadata__ScreenScraper__DevId")]
    pub screenscraper_devid: Option<String>,
    #[arg(long, env = "SovEmu__Metadata__ScreenScraper__DevPassword")]
    pub screenscraper_devpassword: Option<String>,
    /// A ScreenScraper user account (optional; raises the daily quota).
    #[arg(long, env = "SovEmu__Metadata__ScreenScraper__Username")]
    pub screenscraper_user: Option<String>,
    #[arg(long, env = "SovEmu__Metadata__ScreenScraper__Password")]
    pub screenscraper_password: Option<String>,
    #[arg(long, env = "SovEmu__Metadata__ScreenScraper__Url", default_value = sovemu_library::info::screenscraper::DEFAULT_BASE)]
    pub screenscraper_url: String,

    /// Resolved: in `mirror` mode the share being mirrored (and `roms_dir` is the mirror).
    #[arg(skip)]
    pub source_dir: Option<PathBuf>,

    /// End a session after this many seconds with no client connected.
    #[arg(long, env = "SovEmu__Sessions__IdleTimeoutSeconds", default_value_t = 300)]
    pub idle_timeout: u64,

    /// Mark the session cookie `Secure` (set when served over HTTPS).
    #[arg(long, env = "SovEmu__Http__SecureCookies")]
    pub secure_cookies: bool,

    /// How log lines are written to stderr: `text` for a person, `json` (one object per line,
    /// with the request's or session's fields) for a log collector. Workers log the same way.
    #[arg(long, env = "SovEmu__Logging__Format", value_enum, default_value_t = telemetry::LogFormat::Text)]
    pub log_format: telemetry::LogFormat,

    /// An OpenTelemetry collector's OTLP/HTTP address (`http://otel-collector:4318`): traces
    /// go to `/v1/traces` and metrics to `/v1/metrics`. Unset: nothing is exported. The
    /// standard `OTEL_*` variables (service name, resource attributes, headers, export
    /// interval) apply on top.
    #[arg(long, env = "SovEmu__Telemetry__Otlp__Endpoint")]
    pub otel_endpoint: Option<String>,

    /// The admin account, created at startup if missing; its password is kept in step with
    /// this setting. With it set there is no first-run setup page.
    #[arg(long, env = "SovEmu__Authentication__AdminUsername")]
    pub admin_username: Option<String>,
    #[arg(long, env = "SovEmu__Authentication__AdminPassword")]
    pub admin_password: Option<String>,

    /// The key that signs the file URLs the API hands out (artwork, videos, screenshots). Unset:
    /// one is generated on first run and kept in `<data>/server.json`. Changing it invalidates
    /// every URL handed out until clients ask again.
    #[arg(long, env = "SovEmu__Assets__SecretKey")]
    pub asset_key: Option<String>,

    /// The secret the server's sign-in (OpenID Connect) key is derived from: it signs the ID
    /// tokens apps receive. Unset: one is generated on first run and kept in
    /// `<data>/server.json`.
    #[arg(long, env = "SovEmu__Authentication__TokenSigningKey")]
    pub token_key: Option<String>,

    /// The addresses people reach the server at, one per index (`__0=https://games.example.com`,
    /// `__1=http://192.168.1.10:8080`; a single one may be given without an index). Each is an
    /// issuer of sign-in tokens, and a browser can sign in only at one of them. Unset: `localhost`, `127.0.0.1`
    /// and `SovEmu__Streaming__AdvertiseAddress` on the HTTP port.
    #[arg(long, env = "SovEmu__Http__PublicUrl", value_delimiter = ',')]
    pub public_url: Vec<String>,

    /// Whether the apps may sign in with a username and password (`POST /oauth/password`),
    /// beside signing in with a code shown on the screen (the device grant).
    #[arg(long, env = "SovEmu__Authentication__PasswordSignIn", default_value_t = true, action = clap::ArgAction::Set)]
    pub password_sign_in: bool,

    /// Whether what goes wrong on a client (a connection that failed, a stalled picture, a
    /// screen that crashed), sent with `reportClientEvents`, is logged and kept for the admin
    /// Client errors page. Off, they are dropped.
    #[arg(long, env = "SovEmu__ClientErrors__Enabled", default_value_t = true, action = clap::ArgAction::Set)]
    pub client_events: bool,

    /// How long an app's access token lasts, in minutes, before it refreshes it. Signing out
    /// and role changes apply at once regardless: every request checks its token.
    #[arg(long, env = "SovEmu__Authentication__Tokens__AccessTokenMinutes", default_value_t = 60)]
    pub access_token_minutes: u32,

    /// How long a just-used refresh token is still honoured, in seconds: two tabs or requests
    /// refreshing at once get the same new tokens. Presenting it after this ends the sign-in (a
    /// copied token). 0: strictly once.
    #[arg(long, env = "SovEmu__Authentication__Tokens__RefreshRetrySeconds", default_value_t = 30)]
    pub refresh_retry_seconds: u64,

    /// How long a signed file URL stays valid, in hours (rounded to the hour, so URLs are
    /// cacheable).
    #[arg(long, env = "SovEmu__Assets__UrlHours", default_value_t = 24)]
    pub asset_url_hours: u64,

    /// Rescan the library every this many minutes (0: only at startup).
    #[arg(long, env = "SovEmu__Library__ScanIntervalMinutes", default_value_t = 60)]
    pub scan_interval_minutes: u64,

    /// `mirror` mode: sync every this many minutes (0: only at startup).
    #[arg(long, env = "SovEmu__Library__Mirror__IntervalMinutes", default_value_t = 360)]
    pub mirror_interval_minutes: u64,

    /// Look up game information for new games after every scan.
    #[arg(long, env = "SovEmu__Metadata__AutoLookup", default_value_t = true, action = clap::ArgAction::Set)]
    pub metadata_auto: bool,

    /// Video bitrate a game stream starts at, kbit/s. It then follows the connection (up
    /// while there is headroom, down on congestion) and the client (down while its decoder
    /// falls behind), between the floor and the ceiling.
    #[arg(long, env = "SovEmu__Streaming__Video__Kbps", default_value_t = 5000)]
    pub video_kbps: u32,

    /// The lowest video bitrate a stream adapts down to, kbit/s.
    #[arg(long, env = "SovEmu__Streaming__Video__MinKbps", default_value_t = 300)]
    pub video_min_kbps: u32,

    /// The highest video bitrate a stream adapts up to, kbit/s (never below the starting rate).
    #[arg(long, env = "SovEmu__Streaming__Video__MaxKbps", default_value_t = 12000)]
    pub video_max_kbps: u32,

    /// Video codec for game streams: `auto`, `vp8` or `h264`. `auto` chooses for each client
    /// as it connects: the cheapest of the codecs this server encodes and the client decodes,
    /// by what the startup encoder test measured here and what the client's decoder measured
    /// last time (or, until it has, what it declares). A client that sends no capabilities
    /// gets VP8. Naming a codec sends it to every client.
    #[arg(long, env = "SovEmu__Streaming__Video__Codec", default_value = "auto",
          value_parser = ["auto", "h264", "vp8"])]
    pub video_codec: String,

    /// H.264 encoder: `auto` (the GPU's, VA-API or NVENC, else x264), `vaapi`, `nvenc` or
    /// `software` (x264). One that does not open, or holds frames back, falls back to x264.
    #[arg(long, env = "SovEmu__Streaming__Video__Encoder", default_value = "auto",
          value_parser = ["auto", "vaapi", "nvenc", "software"])]
    pub video_encoder: String,

    /// Opus audio bitrate for game streams, kbit/s.
    #[arg(long, env = "SovEmu__Streaming__Audio__Kbps", default_value_t = 128)]
    pub audio_kbps: u32,

    /// How many days a sign-in lasts before signing in again (an app's, from signing in:
    /// refreshing does not extend it; the browser's at the server). 0: until signed out.
    #[arg(long, env = "SovEmu__Authentication__SessionDays", default_value_t = 90)]
    pub session_days: u32,

    /// Failed sign-ins per username before it is locked out (0: never).
    #[arg(long, env = "SovEmu__Authentication__Lockout__MaxFailures", default_value_t = sovemu_identity::MAX_FAILURES)]
    pub login_max_failures: u32,

    /// How long a locked-out username waits, and the window failures are counted in.
    #[arg(long, env = "SovEmu__Authentication__Lockout__Minutes", default_value_t = 5)]
    pub login_lockout_minutes: u64,

    /// Failed sign-ins from one address, across usernames, before it is locked out (0: never).
    /// Behind a reverse proxy, list it in `SovEmu__Network__TrustedProxyNetwork`, or every client is the proxy.
    #[arg(long, env = "SovEmu__Authentication__Lockout__MaxFailuresPerAddress", default_value_t = sovemu_identity::MAX_FAILURES_PER_IP)]
    pub login_max_failures_per_ip: u32,

    /// Reverse proxies whose `X-Forwarded-For` is believed: addresses or ranges, one per index
    /// (`__0=10.0.0.0/8`, `__1=172.16.0.0/12`). Default: none, so the client is the
    /// connection's address.
    #[arg(long, env = "SovEmu__Network__TrustedProxyNetwork", value_delimiter = ',')]
    pub trusted_proxies: Vec<client_ip::Network>,

    /// Database upkeep every this many minutes: expired sign-ins deleted, the query planner's
    /// statistics refreshed and the write-ahead log folded back into the database (0: never).
    #[arg(long, env = "SovEmu__Jobs__MaintenanceIntervalMinutes", default_value_t = 60)]
    pub maintenance_interval_minutes: u64,

    /// Where backups are kept (default `<data>/backups`). A backup holds the database, the
    /// signing keys and every save: keep it somewhere as safe as they are.
    #[arg(long, env = "SovEmu__Backups__Directory")]
    pub backup_dir: Option<PathBuf>,

    /// A backup every this many hours (0: none on a schedule; the admin's button still makes one).
    #[arg(long, env = "SovEmu__Backups__IntervalHours", default_value_t = 0)]
    pub backup_interval_hours: u64,

    /// Backups kept, the newest (0: every one).
    #[arg(long, env = "SovEmu__Backups__Keep", default_value_t = 7)]
    pub backup_keep: u32,

    /// A backup to restore at startup, before the database opens. What was there is moved into
    /// `<data>/before-restore-<time>/`, and each backup is restored once, however often the
    /// server restarts with this set.
    #[arg(long, env = "SovEmu__Backups__RestoreFrom")]
    pub restore_from: Option<PathBuf>,

    /// Days of background-job history kept (each scan, sync, lookup and upkeep run, on the
    /// admin Jobs page); maintenance deletes older runs (0: kept for ever).
    #[arg(long, env = "SovEmu__Jobs__RetentionDays", default_value_t = 30)]
    pub job_history_days: u32,

    /// Days of the activity log kept (sign-ins, games started and ended, admin changes, on the
    /// admin Activity page); maintenance deletes older entries (0: kept for ever).
    #[arg(long, env = "SovEmu__ActivityLog__RetentionDays", default_value_t = 90)]
    pub activity_history_days: u32,

    /// Lines of the server's log kept in memory for the admin Logs page (its own and its game
    /// sessions'); 0 keeps none and turns the page's view off. Lines go to stderr regardless.
    #[arg(long, env = "SovEmu__Logging__BufferLines", default_value_t = 2000)]
    pub log_buffer_lines: usize,

    /// Days of client errors kept (what the apps reported, on the admin Client errors page);
    /// maintenance deletes older ones (0: kept for ever).
    #[arg(long, env = "SovEmu__ClientErrors__RetentionDays", default_value_t = 30)]
    pub client_event_days: u32,

    /// Seconds between readings of the container's CPU and memory, for the admin Resources page
    /// and the OpenTelemetry gauges (0: not read).
    #[arg(long, env = "SovEmu__SystemResources__SampleSeconds", default_value_t = 5)]
    pub resource_sample_seconds: u32,

    /// Days of CPU and memory history kept (a row per minute); maintenance deletes older rows
    /// (0: kept for ever).
    #[arg(long, env = "SovEmu__SystemResources__RetentionDays", default_value_t = 30)]
    pub resource_history_days: u32,

    /// Delete accounts not declared in `SovEmu__Users__*` (or `SovEmu__Authentication__AdminUsername`) at
    /// startup, so the environment is the whole list of users.
    #[arg(long, env = "SovEmu__Users__Prune")]
    pub users_prune: bool,

    /// How long a worker may take to load a core and game, seconds.
    #[arg(long, env = "SovEmu__Sessions__StartTimeoutSeconds", default_value_t = 120)]
    pub worker_start_timeout: u64,

    /// How long a worker may take to write its saves and exit when its session ends, seconds,
    /// before it is killed. A 3D core (PCSX2) can take several seconds to tear down.
    #[arg(long, env = "SovEmu__Sessions__StopTimeoutSeconds", default_value_t = 15)]
    pub worker_stop_timeout: u64,

    /// Rewind history per session, seconds (0 turns rewind off).
    #[arg(long, env = "SovEmu__Sessions__Rewind__Seconds", default_value_t = 60)]
    pub rewind_seconds: u32,

    /// Memory cap for one session's rewind history, MiB.
    #[arg(long, env = "SovEmu__Sessions__Rewind__MaxMegabytes", default_value_t = 64)]
    pub rewind_max_mb: u32,

    /// Fast-forward speed (frames emulated per frame shown).
    #[arg(long, env = "SovEmu__Sessions__FastForward", default_value_t = 4)]
    pub fast_forward: u32,

    /// Games in the home screen's hero (0 hides it).
    #[arg(long, env = "SovEmu__Home__FeaturedCount", default_value_t = 8)]
    pub home_featured: u32,

    /// Games in each of the home screen's rows (recently played, recently added, top rated).
    #[arg(long, env = "SovEmu__Home__RowLength", default_value_t = 20)]
    pub home_row: u32,

    /// Frames between VP8 keyframes (lost packets recover sooner with fewer).
    #[arg(long, env = "SovEmu__Streaming__Video__KeyframeInterval", default_value_t = 600)]
    pub keyframe_interval: u32,

    /// Testing only: throw away this share (percent) of every game's outgoing video packets,
    /// as a lossy link would, to see how a stream recovers. Audio and the connection's own
    /// packets always go. Leave it at 0.
    #[arg(long, env = "SovEmu__Streaming__Debug__VideoDropPercent", default_value_t = 0.0)]
    pub debug_drop_video_percent: f64,

    /// Testing only: hold each of every game's outgoing video packets this many milliseconds,
    /// as a distant client's link would, so a retransmission arrives late. Leave it at 0.
    #[arg(long, env = "SovEmu__Streaming__Debug__VideoDelayMilliseconds", default_value_t = 0)]
    pub debug_delay_video_ms: u32,

    /// Testing only: throw away all of every game's outgoing video packets for this many
    /// milliseconds once every 10 seconds, as a Wi-Fi dropout would. Leave it at 0.
    #[arg(long, env = "SovEmu__Streaming__Debug__VideoDropBurstMilliseconds", default_value_t = 0)]
    pub debug_drop_video_burst_ms: u32,

    /// Testing only: every game's link carries at most this much video, kbit/s, as a slow
    /// internet connection would: more than that waits, then is thrown away. 0: no limit.
    #[arg(long, env = "SovEmu__Streaming__Debug__VideoLimitKbps", default_value_t = 0)]
    pub debug_limit_video_kbps: u32,

    /// Testing only: hold each of every game's outgoing video packets a different time, up
    /// to this many milliseconds, as Wi-Fi and busy routes do. Leave it at 0.
    #[arg(long, env = "SovEmu__Streaming__Debug__VideoJitterMilliseconds", default_value_t = 0)]
    pub debug_jitter_video_ms: u32,

    /// A client that has been silent this many milliseconds and is then heard again was cut
    /// off (a Wi-Fi dropout): it is sent a keyframe at once, where it would otherwise stall
    /// until it asked for one. 0 leaves it to the client.
    #[arg(
        long,
        env = "SovEmu__Streaming__Video__DropoutKeyframeMilliseconds",
        default_value_t = 700
    )]
    pub dropout_keyframe_ms: u32,

    /// Seconds between each game's `session health` log line while someone is playing (frame
    /// rate, overrun ticks, encode time, bitrate, input); 0 turns them off. Every game still
    /// logs a `session summary` when it ends.
    #[arg(long, env = "SovEmu__Sessions__HealthSeconds", default_value_t = 10)]
    pub session_health_secs: u32,

    /// How many failed games (a load error, a crash, a worker that had to be killed) the admin
    /// Sessions page keeps, with the core's last log lines; 0 keeps none. In memory: the log
    /// has every one.
    #[arg(long, env = "SovEmu__Sessions__FailuresKept", default_value_t = 50)]
    pub session_failures: usize,

    /// `cache` mode: games larger than this (MiB) are copied before starting, smaller ones
    /// are played from the share while the copy runs.
    #[arg(long, env = "SovEmu__Library__Cache__LargeGameMegabytes", default_value_t = 64)]
    pub cache_large_game_mb: u64,

    /// Files copied at once by the mirror sync.
    #[arg(long, env = "SovEmu__Library__CopyThreads", default_value_t = 4)]
    pub copy_threads: usize,

    /// Region preference for game information, one per index, most preferred first.
    #[arg(
        long,
        env = "SovEmu__Metadata__Regions",
        value_delimiter = ',',
        default_value = "us,wor,eu,ss,uk,jp"
    )]
    pub metadata_regions: Vec<String>,

    /// Language preference for descriptions, one per index, most preferred first.
    #[arg(
        long,
        env = "SovEmu__Metadata__Languages",
        value_delimiter = ',',
        default_value = "en,wor"
    )]
    pub metadata_languages: Vec<String>,

    /// Box art and videos downloaded at once by the background job that follows a lookup
    /// (never more than the sources' accounts allow).
    #[arg(long, env = "SovEmu__Artwork__MaxConcurrentDownloads", default_value_t = 4)]
    pub artwork_downloads: usize,

    /// Box art is asked for at most this many pixels wide and high, the size the largest screen
    /// shows it at (0: full size, as scanned).
    #[arg(long, env = "SovEmu__Artwork__MaxPixels", default_value_t = 1000)]
    pub artwork_max_px: u32,

    /// Quality of the lossy WebP artwork is made in, box art on download and the copies made
    /// at each client's width (1-100).
    #[arg(long, env = "SovEmu__Artwork__WebpQuality", default_value_t = 80, value_parser = clap::value_parser!(u8).range(1..=100))]
    pub artwork_webp_quality: u8,

    /// Download gameplay videos for games with none of their own.
    #[arg(long, env = "SovEmu__Metadata__Videos", default_value_t = true, action = clap::ArgAction::Set)]
    pub metadata_videos: bool,

    /// Pause between ScreenScraper requests, milliseconds, when the account does not say how
    /// many it may make a minute.
    #[arg(
        long,
        env = "SovEmu__Metadata__ScreenScraper__DelayMilliseconds",
        default_value_t = 1200
    )]
    pub screenscraper_delay_ms: u64,

    /// At most this many ScreenScraper requests at once (0: as many as the account allows).
    #[arg(long, env = "SovEmu__Metadata__ScreenScraper__Threads", default_value_t = 0)]
    pub screenscraper_threads: usize,

    /// The GPU for 3D systems (N64, Dreamcast, GameCube, PS2): `auto` (the first GPU that
    /// works, else software rendering), `software` (Mesa's llvmpipe), `off` (no 3D systems) or
    /// a render node such as `/dev/dri/renderD128`. Probed at startup.
    #[arg(long, env = "SovEmu__Rendering__Gpu", default_value = "auto")]
    pub gpu: String,

    /// Users declared as `SovEmu__Users__<n>__Username` / `__PASSWORD` / `__ADMIN`.
    #[arg(skip)]
    pub users: Vec<structured::UserSpec>,

    /// Per-system overrides declared as `SovEmu__Systems__<id>__Core` / `__ENABLED` /
    /// `__OPTIONS__<name>`.
    #[arg(skip)]
    pub systems: std::collections::BTreeMap<String, structured::SystemSpec>,

    /// Default controller mappings declared as `SovEmu__Controllers__<n>__Id` / `__BINDINGS` /
    /// `__NAME`, which win over the admin page's.
    #[arg(skip)]
    pub controllers: Vec<controllers::ControllerSpec>,

    /// Shared collections declared as `SovEmu__Collections__<n>__Name` / `__GAMES__<m>`, kept in
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

/// Settings that hold secrets: redacted in the startup log and on the Configuration page. They
/// come as any other setting, from Docker or Kubernetes secrets put in the environment.
const SECRETS: &[&str] = &[
    "SovEmu__Authentication__AdminPassword",
    "SovEmu__Assets__SecretKey",
    "SovEmu__Metadata__ScreenScraper__DevPassword",
    "SovEmu__Metadata__ScreenScraper__Password",
    "SovEmu__Authentication__TokenSigningKey",
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

    /// The addresses sign-in works at: `SovEmu__Http__PublicUrl`, else this host's own on the HTTP
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

    /// The background folders, first wins: the configured ones, then
    /// `<data>/artwork/backgrounds` (the built-in ones come after, `appearance`).
    pub fn background_dirs(&self) -> Vec<PathBuf> {
        let mut dirs = self.backgrounds_dirs.clone();
        dirs.push(self.paths().artwork().backgrounds());
        dirs
    }

    /// The decoration folders, first wins: the configured ones, `<data>/artwork/decorations`,
    /// then the built-in one.
    pub fn decoration_dirs(&self) -> Vec<PathBuf> {
        let mut dirs = self.decorations_dirs.clone();
        dirs.push(self.paths().artwork().decorations());
        dirs.push(self.builtin_artwork().decorations());
        dirs
    }

    /// The built-in artwork folder (`SovEmu__Paths__BuiltInArtwork`, else written into the cache).
    pub fn builtin_artwork(&self) -> sovemu_library::layout::ArtworkDir {
        sovemu_library::layout::ArtworkDir::new(
            self.artwork_builtin
                .clone()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or_else(|| self.paths().builtin_artwork()),
        )
    }

    /// The artwork folders, first wins: the data folder's, then the built-in one.
    pub fn artwork_dirs(&self) -> [sovemu_library::layout::ArtworkDir; 2] {
        [self.paths().artwork(), self.builtin_artwork()]
    }

    /// Where BIOS files live: `<bios_dir>/<system>/`.
    pub fn bios_dir(&self) -> PathBuf {
        // `SovEmu__Paths__Bios=` (empty) means the default, not the working directory.
        self.bios
            .clone()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| self.paths().bios())
    }

    /// Where backups are kept: `SovEmu__Backups__Directory`, else `<data>/backups`.
    pub fn backup_dir(&self) -> PathBuf {
        self.backup_dir
            .clone()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| self.paths().backups())
    }

    /// A secret's value, by its variable.
    fn secret(&self, var: &str) -> Option<&str> {
        match var {
            "SovEmu__Authentication__AdminPassword" => &self.admin_password,
            "SovEmu__Assets__SecretKey" => &self.asset_key,
            "SovEmu__Authentication__TokenSigningKey" => &self.token_key,
            "SovEmu__Metadata__ScreenScraper__DevPassword" => &self.screenscraper_devpassword,
            "SovEmu__Metadata__ScreenScraper__Password" => &self.screenscraper_password,
            _ => return None,
        }
        .as_deref()
    }

    /// Treats empty secrets as unset (`--admin-password ""`).
    pub fn clear_empty_secrets(&mut self) {
        for var in SECRETS {
            let slot = self.secret_mut(var).expect("every secret has a field");
            if slot.as_deref().is_some_and(str::is_empty) {
                *slot = None;
            }
        }
    }

    fn secret_mut(&mut self, var: &str) -> Option<&mut Option<String>> {
        Some(match var {
            "SovEmu__Authentication__AdminPassword" => &mut self.admin_password,
            "SovEmu__Assets__SecretKey" => &mut self.asset_key,
            "SovEmu__Authentication__TokenSigningKey" => &mut self.token_key,
            "SovEmu__Metadata__ScreenScraper__DevPassword" => &mut self.screenscraper_devpassword,
            "SovEmu__Metadata__ScreenScraper__Password" => &mut self.screenscraper_password,
            _ => return None,
        })
    }

    /// The effective settings, grouped, secrets redacted: logged at startup so a deployment's
    /// configuration can be checked without guessing (`configuration::summary`).
    pub fn summary(&self) -> String {
        configuration::summary(self)
    }

    /// The STUN servers games and clients ask, as `stun:host:port` ([`STUN_SERVERS`]).
    pub fn stun_urls(&self) -> Vec<String> {
        STUN_SERVERS.iter().map(|s| (*s).to_owned()).collect()
    }
}

/// The STUN servers each game and client asks for its public address: free public ones that
/// need no account, Cloudflare's, then Google's. Not a setting, by decision: remote play works
/// with nothing configured, and there is nothing to get wrong. Tests ask none (no internet).
const STUN_SERVERS: &[&str] = if cfg!(test) {
    &[]
} else {
    &["stun:stun.cloudflare.com:3478", "stun:stun.l.google.com:19302"]
};

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
    /// The server's recent log, when `SovEmu__Logging__BufferLines` keeps any (`logs`).
    pub(crate) logs: Option<Arc<logs::LogBuffer>>,
    /// The built-in TURN relay, unless its port was taken (`relay`).
    pub(crate) relay: Option<Arc<relay::RelayState>>,
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
            &self.config.builtin_artwork(),
            &self.config.paths().artwork().systems(),
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
                .route("/media/games/{id}/{kind}/{width}", get(api::game_media_width))
                .route("/media/games/{id}/saves/{slot}", get(api::state_screenshot))
                .route("/media/decorations/{set}/{*path}", get(api::decoration_file))
                .route("/media/backgrounds/{id}", get(appearance::background_file))
                .route("/media/systems/{id}", get(appearance::photo_file))
                .route("/media/ratings/esrb/{file}", get(api::esrb_file))
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

include!(concat!(env!("OUT_DIR"), "/builtin_screenscraper.rs"));

/// Keep in step with `KEY` in `build.rs`.
const BUILTIN_KEY: &[u8] = b"SovEmu/ScreenScraper";

/// Undoes `build.rs`'s scrambling.
fn unscramble(bytes: &[u8]) -> String {
    bytes.iter().zip(BUILTIN_KEY.iter().cycle()).map(|(b, k)| (b ^ k) as char).collect()
}

/// SovEmu's own ScreenScraper developer credentials, if this build has them (release images do;
/// see `build.rs`).
fn builtin_screenscraper() -> Option<(String, String)> {
    let (id, password) = (unscramble(BUILTIN_DEVID), unscramble(BUILTIN_DEVPASSWORD));
    (!id.is_empty() && !password.is_empty()).then_some((id, password))
}

/// The ScreenScraper developer credentials to use: the settings when both are set, else the
/// ones this build carries. ScreenScraper answers no request without a pair; they say which
/// program asks, while the user account (`SovEmu__Metadata__ScreenScraper__Username`) says who.
fn screenscraper_developer(
    config: &Config,
    builtin: Option<(String, String)>,
) -> Option<((String, String), &'static str)> {
    match (&config.screenscraper_devid, &config.screenscraper_devpassword) {
        (Some(id), Some(pw)) if !id.is_empty() && !pw.is_empty() => {
            Some(((id.clone(), pw.clone()), "settings"))
        }
        _ => builtin.map(|pair| (pair, "built in")),
    }
}

/// The game-information providers `SovEmu__Metadata__Sources` names, in its order, and a note for
/// each one named but not usable (ScreenScraper without its developer credentials).
fn metadata_providers(
    config: &Config,
    paths: &paths::DataPaths,
) -> Result<(Vec<metadata::SharedProvider>, Vec<String>)> {
    use sovemu_library::info::{fbneo, screenscraper};
    const KNOWN: &[&str] = &["fbneo", "screenscraper"];
    let http = sovemu_library::info::http_client();
    let mut providers: Vec<metadata::SharedProvider> = Vec::new();
    let mut notes = Vec::new();
    for id in config.metadata_sources.iter().map(|s| s.trim().to_lowercase()) {
        if providers.iter().any(|p| p.id() == id) {
            continue;
        }
        match id.as_str() {
            "" => {}
            "fbneo" => {
                providers.push(Arc::new(fbneo::FbNeoDats::new(paths.fbneo_dats(), http.clone())))
            }
            "screenscraper" => match screenscraper_developer(config, builtin_screenscraper()) {
                Some(((dev_id, dev_password), from)) => {
                    tracing::info!(developer_credentials = from, "ScreenScraper");
                    let credentials = screenscraper::Credentials {
                        dev_id: dev_id.clone(),
                        dev_password: dev_password.clone(),
                        user: config
                            .screenscraper_user
                            .clone()
                            .filter(|u| !u.is_empty())
                            .zip(config.screenscraper_password.clone()),
                    };
                    let ss = screenscraper::ScreenScraper::new(
                        config.screenscraper_url.clone(),
                        credentials,
                        http.clone(),
                    )
                    .with_preferences(
                        config.metadata_regions.clone(),
                        config.metadata_languages.clone(),
                    )
                    .with_delay(std::time::Duration::from_millis(config.screenscraper_delay_ms))
                    .with_threads(config.screenscraper_threads);
                    providers.push(Arc::new(ss));
                }
                None => notes.push(
                    "ScreenScraper: this build has no developer credentials of its own \
                         (release images do), and SovEmu__Metadata__ScreenScraper__DevId and \
                         SovEmu__Metadata__ScreenScraper__DevPassword are not set, so it is not asked"
                        .to_owned(),
                ),
            },
            other => anyhow::bail!(
                "SovEmu__Metadata__Sources: unknown source {other:?} (known: {})",
                KNOWN.join(", ")
            ),
        }
    }
    for note in &notes {
        tracing::warn!(note = %note, "game information");
    }
    Ok((providers, notes))
}

/// Writes the built-in artwork into `dir`, laid out as `<data>/artwork`: the backgrounds and
/// system photos, the `sovemu` decorations, and the ESRB icons.
fn write_builtin_artwork(dir: &sovemu_library::layout::ArtworkDir) -> std::io::Result<()> {
    appearance::write_builtin(dir)?;
    decorations::write_builtin(&dir.decorations())?;
    sovemu_library::layout::write_esrb(dir)
}

pub async fn build_state(mut config: Config) -> Result<AppState> {
    let paths = config.paths();
    let library = config.roms.clone().unwrap_or_else(|| paths.roms());
    // `SovEmu__Library__Mirror__Systems=` (empty) means all systems, not a system called "".
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
                "this backup was restored already; SovEmu__Backups__RestoreFrom can be removed"
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
        stun_servers: config.stun_urls(),
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
        drop_video_burst_ms: config.debug_drop_video_burst_ms,
        limit_video_kbps: config.debug_limit_video_kbps,
        jitter_video_ms: config.debug_jitter_video_ms,
        dropout_keyframe_ms: config.dropout_keyframe_ms,
        udp_dscp: config.udp_dscp,
        health_interval_secs: config.session_health_secs,
        gpu: gpu.worker_arg.clone(),
        log_format: config.log_format.as_arg(),
        observer: Arc::new(sovemu_sessions::Observers(observers)),
    });
    let cache_dir = paths.cache_games();
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
    let (providers, setup_notes) = metadata_providers(&config, &paths)?;
    let metadata = metadata::Metadata::new(
        &paths,
        providers,
        setup_notes,
        config.artwork_downloads,
        config.artwork_max_px,
        config.artwork_webp_quality,
        config.metadata_videos,
    );
    // The built-in artwork: written here unless the image already holds it.
    if config.artwork_builtin.is_none()
        && let Err(e) = write_builtin_artwork(&config.builtin_artwork())
    {
        tracing::warn!(error = %e, "writing the built-in artwork");
    }
    let decorations = Arc::new(decorations::Decorations::load(&config.decoration_dirs()));
    let appearance = Arc::new(appearance::Appearance::load(
        &config.background_dirs(),
        &config.builtin_artwork(),
        &config.paths().artwork().systems(),
    ));
    let clients = client_ids(&config.data_dir)?;
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
            clients,
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
    let relay = relay::start(&config).await?;
    Ok(AppState {
        db,
        config: Arc::new(config),
        workers,
        cache: cache::Cache::new(cache_dir, cache_bytes),
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
        relay,
    })
}

/// Brings the accounts in line with the environment: the admin and every declared user exist
/// with their declared passwords, and with `SovEmu__Users__Prune` nobody else does.
/// The OAuth clients' ids, per install, kept in `server.json` (never a setting: the apps learn
/// them from `bootstrapConfig`), generated on first run.
fn client_ids(data_dir: &std::path::Path) -> Result<sovemu_identity::ClientIds> {
    Ok(sovemu_identity::ClientIds {
        web: sovemu_assets::load_id(data_dir, "webClientId", "web client id")?,
        device: sovemu_assets::load_id(data_dir, "deviceClientId", "device client id")?,
        mobile: sovemu_assets::load_id(data_dir, "mobileClientId", "mobile client id")?,
    })
}

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
                "SovEmu__Users__Prune is set but no users are declared: that would delete everyone"
            );
        }
        identity.prune_users(&declared).await?;
    }
    Ok(())
}

/// One change to the environment ([`environment_changes`]).
#[derive(Debug, PartialEq, Eq)]
enum EnvChange {
    Set(String, String),
    Remove(String),
}

/// How the environment becomes what the options read, as .NET reads its configuration:
/// - a setting in any letter case is renamed to its own spelling (`SOVEMU__HTTP__LISTEN`);
/// - a list setting's entries, one per index (`SovEmu__Network__TrustedProxyNetwork__0`, `__1`),
///   are joined into the setting, in index order (a single plain value still works);
/// - an empty setting is unset, as are the NAS mount's credentials, which compose passes in
///   (`SOVEMU_NAS_*`).
///
/// `known` are the options' variables; `lists` those that take several values.
fn environment_changes(
    known: &[String],
    lists: &[String],
    vars: &[(String, String)],
) -> Vec<EnvChange> {
    let find =
        |name: &str, among: &[String]| among.iter().find(|k| k.eq_ignore_ascii_case(name)).cloned();
    let mut changes = Vec::new();
    let mut entries: std::collections::BTreeMap<String, Vec<(u32, String)>> = Default::default();
    for (name, value) in vars {
        if name.starts_with("SOVEMU_NAS_") {
            changes.push(EnvChange::Remove(name.clone()));
            continue;
        }
        if !name.get(..8).is_some_and(|p| p.eq_ignore_ascii_case("SovEmu__")) {
            continue;
        }
        if value.is_empty() {
            changes.push(EnvChange::Remove(name.clone()));
            continue;
        }
        if let Some(own) = find(name, known) {
            if own != *name {
                changes.push(EnvChange::Remove(name.clone()));
                changes.push(EnvChange::Set(own, value.clone()));
            }
            continue;
        }
        if let Some((list, index)) = name.rsplit_once("__")
            && let Ok(index) = index.parse::<u32>()
            && let Some(own) = find(list, lists)
        {
            entries.entry(own).or_default().push((index, value.clone()));
            changes.push(EnvChange::Remove(name.clone()));
        }
    }
    for (list, mut values) in entries {
        values.sort();
        let joined = values.into_iter().map(|(_, v)| v).collect::<Vec<_>>().join(",");
        changes.push(EnvChange::Set(list, joined));
    }
    changes
}

/// Puts the environment as the options read it ([`environment_changes`]).
fn normalize_environment() {
    use clap::CommandFactory;
    let command = Config::command();
    let env_of = |a: &clap::Arg| a.get_env().and_then(|e| e.to_str()).map(str::to_owned);
    let known: Vec<String> = command.get_arguments().filter_map(env_of).collect();
    let lists: Vec<String> = command
        .get_arguments()
        .filter(|a| a.get_value_delimiter() == Some(','))
        .filter_map(env_of)
        .collect();
    let vars: Vec<(String, String)> = std::env::vars_os()
        .filter_map(|(k, v)| Some((k.into_string().ok()?, v.into_string().ok()?)))
        .collect();
    for change in environment_changes(&known, &lists, &vars) {
        match change {
            // SAFETY: called first thing in `main`, before the runtime or any other thread
            // exists, so nothing can be reading the environment concurrently.
            EnvChange::Set(k, v) => unsafe { std::env::set_var(k, v) },
            // SAFETY: as above.
            EnvChange::Remove(k) => unsafe { std::env::remove_var(k) },
        }
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
    normalize_environment();
    let config = Config::parse_with_sources(std::env::args_os());
    if let Some(dir) = &config.write_artwork {
        let art = sovemu_library::layout::ArtworkDir::new(dir);
        write_builtin_artwork(&art).with_context(|| format!("writing {}", dir.display()))?;
        println!("built-in artwork: {}", dir.display());
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
    config.clear_empty_secrets();
    let vars: Vec<(String, String)> = std::env::vars().collect();
    config.given.keep_environment(&vars);
    config.users = structured::users(&vars)?;
    config.systems = structured::systems(&vars)?;
    config.controllers = structured::controllers(&vars)?;
    config.collections = structured::collections(&vars)?;
    let addr = config.http;
    let state = build_state(config).await?;
    tracing::info!(summary = %state.config.summary(), "configuration");
    let _gauges = telemetry::observe(&state);
    // The sources' accounts (level, limits), for the Library page before the first lookup.
    let metadata = state.metadata.clone();
    tokio::spawn(async move { metadata.check_accounts().await });
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
    fn metadata_sources_are_built_in_order() {
        let paths = paths::DataPaths::new(std::env::temp_dir().join("sovemu-sources"));
        let ids = |args: &[&str]| {
            let config = Config::parse_from(["sovemu-server"].iter().chain(args));
            let (providers, notes) = metadata_providers(&config, &paths).unwrap();
            (providers.iter().map(|p| p.id()).collect::<Vec<_>>(), notes.len())
        };
        assert_eq!(ids(&[]), (vec!["fbneo"], 1), "ScreenScraper needs credentials: a note");
        let creds = ["--screenscraper-devid", "d", "--screenscraper-devpassword", "p"];
        assert_eq!(ids(&creds), (vec!["fbneo", "screenscraper"], 0));
        let reversed = [&creds[..], &["--metadata-sources", "screenscraper,fbneo"]].concat();
        assert_eq!(ids(&reversed), (vec!["screenscraper", "fbneo"], 0));
        let config = Config::parse_from(["sovemu-server", "--metadata-sources", "launchbox"]);
        let err = metadata_providers(&config, &paths).err().unwrap().to_string();
        assert!(err.contains("launchbox") && err.contains("screenscraper"), "{err}");
    }

    #[test]
    fn developer_credentials_come_from_settings_else_the_build() {
        let builtin = Some(("sovemu".to_owned(), "secret".to_owned()));
        let none = Config::parse_from(["sovemu-server"]);
        assert_eq!(screenscraper_developer(&none, None), None);
        assert_eq!(
            screenscraper_developer(&none, builtin.clone()),
            Some((("sovemu".into(), "secret".into()), "built in"))
        );
        let own = Config::parse_from([
            "sovemu-server",
            "--screenscraper-devid",
            "mine",
            "--screenscraper-devpassword",
            "pw",
        ]);
        assert_eq!(
            screenscraper_developer(&own, builtin.clone()),
            Some((("mine".into(), "pw".into()), "settings")),
            "the settings win"
        );
        // Half a pair is not a pair: the build's are used.
        let half = Config::parse_from(["sovemu-server", "--screenscraper-devid", "mine"]);
        assert_eq!(screenscraper_developer(&half, builtin).unwrap().1, "built in");
    }

    #[test]
    fn built_in_credentials_unscramble() {
        let scrambled: Vec<u8> =
            "dev-42".bytes().zip(BUILTIN_KEY.iter().cycle()).map(|(b, k)| b ^ k).collect();
        assert_ne!(scrambled, b"dev-42", "not stored as they are");
        assert_eq!(unscramble(&scrambled), "dev-42");
        assert_eq!(unscramble(&[]), "");
    }

    #[tokio::test]
    async fn the_thread_cap_reaches_screenscraper() {
        // An account that may have four requests open; the setting allows two.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let app = axum::Router::new().route(
            "/ssuserInfos.php",
            axum::routing::get(|| async {
                "<Data><ssuser><niveau>1</niveau><maxthreads>4</maxthreads>\
                 <requeststoday>0</requeststoday></ssuser></Data>"
            }),
        );
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let paths = paths::DataPaths::new(std::env::temp_dir().join("sovemu-threads"));
        let config = Config::parse_from([
            "sovemu-server",
            "--screenscraper-devid",
            "d",
            "--screenscraper-devpassword",
            "p",
            "--screenscraper-url",
            &base,
            "--screenscraper-threads",
            "2",
            // ssuserInfos.php is only asked with a user account.
            "--screenscraper-user",
            "u",
            "--screenscraper-password",
            "p",
        ]);
        let (providers, _) = metadata_providers(&config, &paths).unwrap();
        let ss = providers.iter().find(|p| p.id() == "screenscraper").unwrap();
        ss.prepare().await.unwrap();
        assert_eq!(ss.account().unwrap().max_threads, Some(4));
        assert_eq!(ss.concurrency(), 2, "capped by SovEmu__Metadata__ScreenScraper__Threads");
    }

    #[test]
    fn the_environment_is_read_as_dotnet_reads_its_configuration() {
        let known: Vec<String> = ["SovEmu__Http__Listen", "SovEmu__Network__TrustedProxyNetwork"]
            .map(String::from)
            .to_vec();
        let lists = vec!["SovEmu__Network__TrustedProxyNetwork".to_owned()];
        let vars: Vec<(String, String)> = [
            ("SOVEMU__HTTP__LISTEN", "0.0.0.0:9000"),
            ("SovEmu__Network__TrustedProxyNetwork__1", "10.1.0.0/16"),
            ("sovemu__network__trustedproxynetwork__0", "10.0.0.0/8"),
            ("SovEmu__Metadata__Videos", ""),
            ("SovEmu__Users__0__Username", "kid"),
            ("SOVEMU_NAS_PASSWORD", "hunter2"),
            ("HOME", "/sov-data"),
        ]
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .to_vec();
        let changes = environment_changes(&known, &lists, &vars);
        let set = |k: &str, v: &str| EnvChange::Set(k.to_owned(), v.to_owned());
        let removed = |k: &str| EnvChange::Remove(k.to_owned());
        for expected in [
            removed("SOVEMU__HTTP__LISTEN"),
            set("SovEmu__Http__Listen", "0.0.0.0:9000"),
            set("SovEmu__Network__TrustedProxyNetwork", "10.0.0.0/8,10.1.0.0/16"),
            removed("SovEmu__Metadata__Videos"),
            removed("SOVEMU_NAS_PASSWORD"),
        ] {
            assert!(changes.contains(&expected), "{expected:?} in {changes:?}");
        }
        for untouched in ["SovEmu__Users__0__Username", "HOME"] {
            assert!(!changes.iter().any(
                |c| matches!(c, EnvChange::Remove(k) | EnvChange::Set(k, _) if k == untouched)
            ));
        }
    }

    #[test]
    fn secrets_are_redacted_and_empty_ones_unset() {
        let mut config = Config::parse_from([
            "sovemu-server",
            "--admin-username",
            "boss",
            "--admin-password",
            "s3cret",
            "--screenscraper-password",
            "",
        ]);
        config.clear_empty_secrets();
        assert_eq!(config.screenscraper_password, None, "empty means unset");
        let summary = config.summary();
        assert!(summary.contains("SovEmu__Authentication__AdminPassword=(set)"), "{summary}");
        assert!(!summary.contains("s3cret"));
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
        assert!(summary.contains("SovEmu__Logging__Format=json (command line)"), "{summary}");
        assert!(
            summary.contains("SovEmu__Telemetry__Otlp__Endpoint=http://collector:4318/"),
            "{summary}"
        );
        let config = Config::parse_with_sources([
            "sovemu-server",
            "--otel-endpoint",
            "https://user:t0k3n@collector:4318/otlp?api-key=k3y",
        ]);
        let summary = config.summary();
        assert!(
            summary
                .contains("SovEmu__Telemetry__Otlp__Endpoint=https://***@collector:4318/otlp?***"),
            "{summary}"
        );
        for secret in ["t0k3n", "k3y"] {
            assert!(!summary.contains(secret), "{secret} in {summary}");
        }
        assert!(Config::try_parse_from(["sovemu-server", "--log-format", "xml"]).is_err());
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
