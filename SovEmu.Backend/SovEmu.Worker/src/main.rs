//! `sovemu-worker`: the per-session process that hosts one libretro core.
//!
//! Modes:
//! - `session` runs one game session for `sovemu-server`: JSON-line requests on stdin (offers,
//!   control), replies and events on stdout, media over WebRTC (VP8 + Opus, input over a
//!   DataChannel).
//! - `serve` is the same streaming loop standalone, serving the web client and taking offers
//!   over HTTP itself; for development and tests.
//! - `headless` runs a core with no client and writes what it produced to disk (PNG frames, a
//!   WAV file and a timing summary).
//! - `probe` loads cores and reports what they say about themselves.
//! - `gpu` finds the GPUs this process can render with, by rendering with each.
//! - `connection-test` answers one WebRTC offer on a game port and echoes its data channel, so
//!   an admin can check a browser reaches the streams.

mod adapt;
mod connection_test;
mod encoders;
mod gpu;
mod headless;
mod health;
mod keyframes;
mod loading;
mod loss;
mod pacing;
mod patch;
mod persist;
mod priority;
mod probe;
mod replay;
mod rewind;
mod runahead;
mod schedule;
mod serve;
mod session;
mod standin;
mod stream;
mod stun;
mod sysdir;
mod video;
mod watchdog;
mod web;

use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

use crate::schedule::InputHold;

#[derive(Debug, Parser)]
#[command(name = "sovemu-worker", version, about = "Hosts one libretro core per process")]
struct Cli {
    #[command(subcommand)]
    command: Command,

    /// How log lines are written to stderr: `text`, or `json` (one object per line) for a log
    /// collector. The server passes its own `SovEmu__Logging__Format`.
    #[arg(long, global = true, value_enum, default_value_t = LogFormat::Text)]
    log_format: LogFormat,

    /// The server's id for this game session, on every log line (in a `session` span).
    #[arg(long, global = true)]
    session_id: Option<String>,

    /// The session's system (`snes`, `psx`), on every log line beside the session id.
    #[arg(long, global = true)]
    system: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum LogFormat {
    Text,
    Json,
}

/// Logs to stderr (stdout is the server's channel), as text or JSON lines.
fn init_logs(format: LogFormat) {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    // Timestamps in the local time zone (`TZ`, which the server passes on), as the server's.
    let builder = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_timer(tracing_subscriber::fmt::time::ChronoLocal::rfc_3339());
    match format {
        LogFormat::Text => builder.init(),
        LogFormat::Json => {
            builder.json().flatten_event(true).with_current_span(true).with_span_list(false).init()
        }
    }
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run one game session for sovemu-server (JSON lines on stdin/stdout).
    Session(SessionArgs),
    /// Run a core and stream it to a WebRTC client; serves the web client on --http.
    Serve(ServeArgs),
    /// Run a core with no client and dump its video, audio and timings to a directory.
    Headless(HeadlessArgs),
    /// Load each core, print its name, version and extensions as JSON, and fail if any core
    /// cannot be loaded.
    Probe(ProbeArgs),
    /// Try every GPU (and the software renderer): make a GL context, draw, read it back. Prints
    /// the results as JSON.
    Gpu,
    /// Run every video encoder on a moving picture and time it. Prints the results as JSON.
    Encoders(EncodersArgs),
    /// Answer one WebRTC offer (JSON lines on stdin/stdout) with a peer that has no media, and
    /// echo its data channels: the admin's connection test.
    ConnectionTest(ConnectionTestArgs),
}

#[derive(Debug, clap::Args)]
struct EncodersArgs {
    /// The GPU sessions use, as their `--gpu`: a render node is where VA-API encodes.
    #[arg(long, default_value = "auto")]
    gpu: GpuArg,
    /// The longest the whole test may take, in milliseconds.
    #[arg(long, default_value_t = 10_000)]
    budget_ms: u64,
}

#[derive(Debug, clap::Args)]
struct ConnectionTestArgs {
    /// The UDP address to answer on: one of the game ports.
    #[arg(long)]
    udp: SocketAddr,

    /// The IP address clients send to, if not the bound one.
    #[arg(long = "advertise-ip")]
    advertise: Option<IpAddr>,

    /// A STUN server (`host:port`) to ask for the public address, once per server.
    #[arg(long = "stun")]
    stun: Vec<String>,

    /// Seconds before it gives up, connected or not.
    #[arg(long, default_value_t = 30)]
    timeout: u64,
}

/// `--gpu`: `auto`, `software`, `off` (refuse GPU cores) or a render node.
#[derive(Debug, Clone)]
pub struct GpuArg(pub Option<sovemu_retro_host::GpuChoice>);

impl std::str::FromStr for GpuArg {
    type Err = std::convert::Infallible;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(match s.trim() {
            "off" | "none" => None,
            other => Some(other.parse()?),
        }))
    }
}

#[derive(Debug, clap::Args)]
struct SessionArgs {
    /// The libretro core library.
    #[arg(long)]
    core: PathBuf,

    /// The content to load.
    #[arg(long)]
    rom: Option<PathBuf>,

    /// An IPS, UPS or BPS patch applied to `--rom` as it loads (never on disk).
    #[arg(long, requires = "rom")]
    patch: Option<PathBuf>,

    /// The patch's name: the patched game's battery save is `<rom stem> [<name>].srm`.
    #[arg(long, requires = "patch", default_value = "Patched")]
    patch_name: String,

    /// UDP address for this session's WebRTC traffic.
    #[arg(long)]
    udp: SocketAddr,

    /// The IP address clients send media to, if not the bound one.
    #[arg(long = "advertise-ip")]
    advertise: Option<IpAddr>,

    /// A STUN server (`host:port`) to ask for the public address, once per server.
    #[arg(long = "stun")]
    stun: Vec<String>,

    /// The system's BIOS folder.
    #[arg(long)]
    system_dir: PathBuf,

    /// Where in the core's system directory `--system-dir`'s files belong (`dc` for Flycast,
    /// `pcsx2/bios` for PCSX2); empty for the root.
    #[arg(long, default_value = "")]
    bios_path: String,

    /// Directory for battery saves (`<game>.srm`).
    #[arg(long)]
    save_dir: PathBuf,

    /// Directory for this game's save states (`slotN.state`, `slotN.png`).
    #[arg(long)]
    state_dir: Option<PathBuf>,

    /// Video bitrate a stream starts at, kbit/s; it then follows the connection and the client.
    #[arg(long, default_value_t = 5000)]
    video_kbps: u32,

    /// The lowest video bitrate adaptation chooses, kbit/s.
    #[arg(long, default_value_t = 300)]
    video_min_kbps: u32,

    /// The highest video bitrate adaptation chooses, kbit/s (never below the starting rate).
    #[arg(long, default_value_t = 12000)]
    video_max_kbps: u32,

    /// Opus bitrate, kbit/s.
    #[arg(long, default_value_t = 128)]
    audio_kbps: u32,

    /// Set a core option. Repeatable.
    #[arg(long = "var", value_name = "KEY=VALUE", value_parser = parse_key_value)]
    variables: Vec<(String, String)>,

    /// Exit after this many seconds with no client connected (0: never).
    #[arg(long, default_value_t = 300)]
    idle_timeout: u64,

    /// Seconds of rewind history (0 turns rewind off).
    #[arg(long, default_value_t = 60)]
    rewind_seconds: u32,

    /// At most this many MiB of rewind history.
    #[arg(long, default_value_t = 64)]
    rewind_max_mb: u32,

    /// Frames emulated per frame shown while fast-forwarding.
    #[arg(long, default_value_t = 4)]
    fast_forward: u32,

    /// Frames to run ahead of the one shown, hiding the game's own input lag (0 turns it off;
    /// at most 4). Each costs a frame of emulation and a save state, so keep it to 2D systems.
    #[arg(long, default_value_t = 0)]
    run_ahead: u32,

    /// Seconds between `session health` log lines while a client is connected: frame rate,
    /// overrun and late ticks, encode time, bitrate, input (0: none; the exit summary stays).
    #[arg(long, default_value_t = 10)]
    health_interval: u32,

    /// Longest gap between video keyframes, in frames (clients also ask for one after loss).
    #[arg(long, default_value_t = 600)]
    keyframe_interval: u32,
    /// Testing only: throw away this share (percent) of outgoing video packets, as a lossy
    /// link would, to see how the stream recovers.
    #[arg(long, default_value_t = 0.0)]
    drop_video_percent: f64,
    /// Testing only: hold each outgoing video packet this many milliseconds, as a distant
    /// client's link would.
    #[arg(long, default_value_t = 0)]
    delay_video_ms: u32,
    /// Testing only: throw away every outgoing video packet for this many milliseconds once
    /// every 10 seconds, as a Wi-Fi dropout would.
    #[arg(long, default_value_t = 0)]
    drop_video_burst_ms: u32,
    /// Testing only: the link carries at most this much video, kbit/s, as a slow internet
    /// connection would (0: no limit).
    #[arg(long, default_value_t = 0)]
    limit_video_kbps: u32,
    /// Testing only: hold each outgoing video packet a different time, up to this many
    /// milliseconds, as Wi-Fi and busy routes do.
    #[arg(long, default_value_t = 0)]
    jitter_video_ms: u32,
    /// A client silent this many milliseconds and then heard again is sent a keyframe at
    /// once: it was cut off, and would otherwise stall until it asked for one (0: never).
    #[arg(long, default_value_t = 700)]
    dropout_keyframe_ms: u32,
    /// The DSCP value the stream's packets are marked with, for priority on Wi-Fi and through
    /// routers that honour it: 34 (AF41, interactive video) by default, 0 for none.
    #[arg(long, default_value_t = 34, value_parser = clap::value_parser!(u8).range(0..=63))]
    udp_dscp: u8,
    /// The GPU for cores that render with OpenGL: `auto`, `software`, `off` or a render node
    /// (`/dev/dri/renderD128`). A render node also encodes H.264 (VA-API).
    #[arg(long, default_value = "auto")]
    gpu: GpuArg,

    /// The video codec: `auto` (H.264 if the client offers it, else VP8), `h264` or `vp8`.
    #[arg(long, default_value = "auto")]
    video_codec: video::CodecSetting,

    /// The H.264 encoder: `auto` (the GPU's, else x264), `vaapi`, `nvenc` or `software` (x264).
    /// One that does not open, or holds frames back, falls back to x264.
    #[arg(long, default_value = "auto")]
    video_encoder: sovemu_media::H264Preference,

    /// Scale each picture up this many times (1 or 2) by repeating pixels before it is encoded:
    /// at 2 each pixel is a 2×2 block, so 4:2:0 chroma keeps every pixel's colour.
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..=2))]
    video_prescale: u32,

    /// VP8's screen-content mode: 0 off, 1 on, 2 on with more aggressive rate control. H.264
    /// has no such mode.
    #[arg(long, default_value_t = 0, value_parser = clap::value_parser!(u8).range(0..=2))]
    video_screen_content: u8,

    /// Testing only: a folder of `frame_NNNNNN.png` shown instead of the game's picture, the
    /// next one each time the game draws, looping (the encoder bench's captures, #263).
    #[arg(long)]
    replay_frames: Option<PathBuf>,
}

#[derive(Debug, clap::Args)]
struct ServeArgs {
    /// The libretro core library (.so / .dylib / .dll).
    #[arg(long)]
    core: PathBuf,

    /// The content to load. Omit for cores that run
    /// without content.
    #[arg(long)]
    rom: Option<PathBuf>,

    /// Address for the test page and signalling (`POST /api/offer`). Port 0 picks one.
    #[arg(long, default_value = "0.0.0.0:8080", env = "SovEmu__Http__Listen")]
    http: SocketAddr,

    /// The built web client to serve (`SovEmu.Web/dist`).
    #[arg(long, env = "SovEmu__Paths__Web")]
    web_dir: Option<PathBuf>,

    /// UDP address for all WebRTC traffic (one port for everything). Port 0 picks one.
    #[arg(long, default_value = "0.0.0.0:50000")]
    udp: SocketAddr,

    /// The IP address clients should send media to, if not the bound one: in a container, the
    /// host's address as browsers reach it.
    #[arg(long = "advertise-ip", env = "SovEmu__Streaming__AdvertiseAddress")]
    advertise: Option<IpAddr>,

    /// Video bitrate a stream starts at, kbit/s; it then follows the connection and the client.
    #[arg(long, default_value_t = 5000)]
    video_kbps: u32,

    /// The lowest video bitrate adaptation chooses, kbit/s.
    #[arg(long, default_value_t = 300)]
    video_min_kbps: u32,

    /// The highest video bitrate adaptation chooses, kbit/s (never below the starting rate).
    #[arg(long, default_value_t = 12000)]
    video_max_kbps: u32,

    /// Opus bitrate, kbit/s.
    #[arg(long, default_value_t = 128)]
    audio_kbps: u32,

    /// Set a core option, e.g. `--var snes9x_overclock=disabled`. Repeatable.
    #[arg(long = "var", value_name = "KEY=VALUE", value_parser = parse_key_value)]
    variables: Vec<(String, String)>,

    /// The system's BIOS folder.
    #[arg(long, default_value = "system")]
    system_dir: PathBuf,

    /// Where in the core's system directory `--system-dir`'s files belong (`dc` for Flycast,
    /// `pcsx2/bios` for PCSX2); empty for the root.
    #[arg(long, default_value = "")]
    bios_path: String,

    /// Directory the core may save into.
    #[arg(long, default_value = "saves")]
    save_dir: PathBuf,

    /// Seconds of rewind history (0 turns rewind off).
    #[arg(long, default_value_t = 60)]
    rewind_seconds: u32,

    /// At most this many MiB of rewind history.
    #[arg(long, default_value_t = 64)]
    rewind_max_mb: u32,

    /// Frames emulated per frame shown while fast-forwarding.
    #[arg(long, default_value_t = 4)]
    fast_forward: u32,

    /// Frames to run ahead of the one shown, hiding the game's own input lag (0 turns it off;
    /// at most 4). Each costs a frame of emulation and a save state, so keep it to 2D systems.
    #[arg(long, default_value_t = 0)]
    run_ahead: u32,

    /// Seconds between `session health` log lines while a client is connected: frame rate,
    /// overrun and late ticks, encode time, bitrate, input (0: none; the exit summary stays).
    #[arg(long, default_value_t = 10)]
    health_interval: u32,

    /// Longest gap between video keyframes, in frames (clients also ask for one after loss).
    #[arg(long, default_value_t = 600)]
    keyframe_interval: u32,
    /// Testing only: throw away this share (percent) of outgoing video packets, as a lossy
    /// link would, to see how the stream recovers.
    #[arg(long, default_value_t = 0.0)]
    drop_video_percent: f64,
    /// Testing only: hold each outgoing video packet this many milliseconds, as a distant
    /// client's link would.
    #[arg(long, default_value_t = 0)]
    delay_video_ms: u32,
    /// Testing only: throw away every outgoing video packet for this many milliseconds once
    /// every 10 seconds, as a Wi-Fi dropout would.
    #[arg(long, default_value_t = 0)]
    drop_video_burst_ms: u32,
    /// Testing only: the link carries at most this much video, kbit/s, as a slow internet
    /// connection would (0: no limit).
    #[arg(long, default_value_t = 0)]
    limit_video_kbps: u32,
    /// Testing only: hold each outgoing video packet a different time, up to this many
    /// milliseconds, as Wi-Fi and busy routes do.
    #[arg(long, default_value_t = 0)]
    jitter_video_ms: u32,
    /// A client silent this many milliseconds and then heard again is sent a keyframe at
    /// once: it was cut off, and would otherwise stall until it asked for one (0: never).
    #[arg(long, default_value_t = 700)]
    dropout_keyframe_ms: u32,
    /// The DSCP value the stream's packets are marked with, for priority on Wi-Fi and through
    /// routers that honour it: 34 (AF41, interactive video) by default, 0 for none.
    #[arg(long, default_value_t = 34, value_parser = clap::value_parser!(u8).range(0..=63))]
    udp_dscp: u8,
    /// The GPU for cores that render with OpenGL: `auto`, `software`, `off` or a render node
    /// (`/dev/dri/renderD128`). A render node also encodes H.264 (VA-API).
    #[arg(long, default_value = "auto")]
    gpu: GpuArg,

    /// The video codec: `auto` (H.264 if the client offers it, else VP8), `h264` or `vp8`.
    #[arg(long, default_value = "auto")]
    video_codec: video::CodecSetting,

    /// The H.264 encoder: `auto` (the GPU's, else x264), `vaapi`, `nvenc` or `software` (x264).
    /// One that does not open, or holds frames back, falls back to x264.
    #[arg(long, default_value = "auto")]
    video_encoder: sovemu_media::H264Preference,

    /// Scale each picture up this many times (1 or 2) by repeating pixels before it is encoded:
    /// at 2 each pixel is a 2×2 block, so 4:2:0 chroma keeps every pixel's colour.
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..=2))]
    video_prescale: u32,

    /// VP8's screen-content mode: 0 off, 1 on, 2 on with more aggressive rate control. H.264
    /// has no such mode.
    #[arg(long, default_value_t = 0, value_parser = clap::value_parser!(u8).range(0..=2))]
    video_screen_content: u8,

    /// Testing only: a folder of `frame_NNNNNN.png` shown instead of the game's picture, the
    /// next one each time the game draws, looping (the encoder bench's captures, #263).
    #[arg(long)]
    replay_frames: Option<PathBuf>,
}

#[derive(Debug, clap::Args)]
struct ProbeArgs {
    /// Core libraries, or directories of them.
    #[arg(required = true, env = "SovEmu__Paths__Cores", value_delimiter = ':')]
    cores: Vec<PathBuf>,
}

#[derive(Debug, clap::Args)]
struct HeadlessArgs {
    /// The libretro core library (.so / .dylib / .dll).
    #[arg(long)]
    core: PathBuf,

    /// The content to load. Omit for cores that run without content.
    #[arg(long)]
    rom: Option<PathBuf>,

    /// Directory to write frames, audio.wav and summary.json into (created if missing).
    #[arg(long, default_value = "headless-out")]
    out: PathBuf,

    /// Number of frames to run.
    #[arg(long, default_value_t = 600)]
    frames: u64,

    /// Save every Nth frame as a PNG (0 saves none; the last frame is always saved).
    #[arg(long, default_value_t = 60)]
    png_every: u64,

    /// Hold buttons on player 1 for a range of frames, e.g. `120-130:start` or `300:a+b`.
    /// Repeatable.
    #[arg(long = "hold", value_name = "FRAMES:BUTTONS")]
    holds: Vec<InputHold>,

    /// Pace frames at the core's real frame rate instead of running as fast as possible.
    #[arg(long)]
    realtime: bool,

    /// Frames to run ahead of the one shown (as in a session): the PNGs and the timings are
    /// then the run-ahead picture and its cost; the audio stays the real timeline's.
    #[arg(long, default_value_t = 0)]
    run_ahead: u32,

    /// Set a core option, e.g. `--var snes9x_overclock=disabled`. Repeatable.
    #[arg(long = "var", value_name = "KEY=VALUE", value_parser = parse_key_value)]
    variables: Vec<(String, String)>,

    /// The system's BIOS folder.
    #[arg(long, default_value = "system")]
    system_dir: PathBuf,

    /// Where in the core's system directory `--system-dir`'s files belong (`dc` for Flycast,
    /// `pcsx2/bios` for PCSX2); empty for the root.
    #[arg(long, default_value = "")]
    bios_path: String,

    /// Directory the core may save into (defaults to `<out>/saves`).
    #[arg(long)]
    save_dir: Option<PathBuf>,
    /// The GPU for cores that render with OpenGL: `auto`, `software`, `off` or a render node
    /// (`/dev/dri/renderD128`).
    #[arg(long, default_value = "auto")]
    gpu: GpuArg,
}

fn parse_key_value(s: &str) -> Result<(String, String), String> {
    s.split_once('=')
        .map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned()))
        .filter(|(k, _)| !k.is_empty())
        .ok_or_else(|| format!("expected KEY=VALUE, got {s:?}"))
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    init_logs(cli.log_format);
    let span = cli
        .session_id
        .as_deref()
        .map(|id| tracing::info_span!("session", session.id = %id, system = cli.system.as_deref()));
    let _session = span.as_ref().map(tracing::Span::enter);
    let result = match cli.command {
        Command::Session(args) => session::run(args),
        Command::Serve(args) => serve::run(args),
        Command::Headless(args) => headless::run(args),
        Command::Probe(args) => probe::run(args),
        Command::Gpu => gpu::run(),
        Command::Encoders(args) => encoders::run(&args.gpu, args.budget_ms),
        Command::ConnectionTest(args) => connection_test::run(args),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            tracing::error!(error = %format!("{err:#}"), "worker failed");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_is_well_formed() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }

    #[test]
    fn key_value_parsing() {
        assert_eq!(parse_key_value("a=b"), Ok(("a".into(), "b".into())));
        assert_eq!(parse_key_value("a = b=c"), Ok(("a".into(), "b=c".into())));
        assert!(parse_key_value("novalue").is_err());
        assert!(parse_key_value("=x").is_err());
    }
}
