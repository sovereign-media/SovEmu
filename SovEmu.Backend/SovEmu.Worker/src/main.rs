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

mod gpu;
mod headless;
mod pacing;
mod persist;
mod probe;
mod rewind;
mod schedule;
mod serve;
mod session;
mod stream;
mod sysdir;
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

    /// UDP address for this session's WebRTC traffic.
    #[arg(long)]
    udp: SocketAddr,

    /// The IP address clients send media to, if not the bound one.
    #[arg(long = "advertise-ip")]
    advertise: Option<IpAddr>,

    /// BIOS and system files directory.
    #[arg(long)]
    system_dir: PathBuf,

    /// Directory for battery saves (`<game>.srm`).
    #[arg(long)]
    save_dir: PathBuf,

    /// Directory for this game's save states (`slotN.state`, `slotN.png`).
    #[arg(long)]
    state_dir: Option<PathBuf>,

    /// VP8 target bitrate, kbit/s.
    #[arg(long, default_value_t = 2500)]
    video_kbps: u32,

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

    /// Longest gap between video keyframes, in frames (clients also ask for one after loss).
    #[arg(long, default_value_t = 600)]
    keyframe_interval: u32,
    /// The GPU for cores that render with OpenGL: `auto`, `software`, `off` or a render node
    /// (`/dev/dri/renderD128`).
    #[arg(long, default_value = "auto")]
    gpu: GpuArg,
}

#[derive(Debug, clap::Args)]
struct ServeArgs {
    /// The libretro core library (.so / .dylib / .dll).
    #[arg(long, env = "SOVEMU_CORE")]
    core: PathBuf,

    /// The content to load (or `SOVEMU_ROM`; empty means none). Omit for cores that run
    /// without content.
    #[arg(long)]
    rom: Option<PathBuf>,

    /// Address for the test page and signalling (`POST /api/offer`). Port 0 picks one.
    #[arg(long, default_value = "0.0.0.0:8080", env = "SOVEMU_HTTP")]
    http: SocketAddr,

    /// The built web client to serve (`SovEmu.Web/dist`).
    #[arg(long, env = "SOVEMU_WEB_DIR")]
    web_dir: Option<PathBuf>,

    /// UDP address for all WebRTC traffic (one port for everything). Port 0 picks one.
    #[arg(long, default_value = "0.0.0.0:50000", env = "SOVEMU_UDP")]
    udp: SocketAddr,

    /// The IP address clients should send media to, if not the bound one: in a container, the
    /// host's address as browsers reach it.
    #[arg(long = "advertise-ip", env = "SOVEMU_ADVERTISE_IP")]
    advertise: Option<IpAddr>,

    /// VP8 target bitrate, kbit/s.
    #[arg(long, default_value_t = 2500)]
    video_kbps: u32,

    /// Opus bitrate, kbit/s.
    #[arg(long, default_value_t = 128)]
    audio_kbps: u32,

    /// Set a core option, e.g. `--var snes9x_overclock=disabled`. Repeatable.
    #[arg(long = "var", value_name = "KEY=VALUE", value_parser = parse_key_value)]
    variables: Vec<(String, String)>,

    /// BIOS and system files directory.
    #[arg(long, default_value = "system")]
    system_dir: PathBuf,

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

    /// Longest gap between video keyframes, in frames (clients also ask for one after loss).
    #[arg(long, default_value_t = 600)]
    keyframe_interval: u32,
    /// The GPU for cores that render with OpenGL: `auto`, `software`, `off` or a render node
    /// (`/dev/dri/renderD128`).
    #[arg(long, default_value = "auto")]
    gpu: GpuArg,
}

#[derive(Debug, clap::Args)]
struct ProbeArgs {
    /// Core libraries, or directories of them.
    #[arg(required = true, env = "SOVEMU_CORES_DIR", value_delimiter = ':')]
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

    /// Set a core option, e.g. `--var snes9x_overclock=disabled`. Repeatable.
    #[arg(long = "var", value_name = "KEY=VALUE", value_parser = parse_key_value)]
    variables: Vec<(String, String)>,

    /// BIOS and system files directory.
    #[arg(long, default_value = "system")]
    system_dir: PathBuf,

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
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();
    let result = match cli.command {
        Command::Session(args) => session::run(args),
        Command::Serve(mut args) => {
            // Read by hand so an empty value (compose's `${SOVEMU_ROM:-}`) means "no content".
            if args.rom.is_none() {
                args.rom =
                    std::env::var_os("SOVEMU_ROM").filter(|v| !v.is_empty()).map(PathBuf::from);
            }
            serve::run(args)
        }
        Command::Headless(args) => headless::run(args),
        Command::Probe(args) => probe::run(args),
        Command::Gpu => gpu::run(),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            tracing::error!("{err:#}");
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
