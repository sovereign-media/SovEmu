//! `sovemu-worker`: the per-session process that hosts one libretro core.
//!
//! Phase 0 modes: `headless` runs a core with no client attached and writes what it produced to
//! disk (PNG frames, a WAV file and a timing summary); `probe` loads cores and reports what they
//! say about themselves. Later phases add the session mode the server spawns, which streams over
//! WebRTC instead.

mod headless;
mod pacing;
mod probe;
mod schedule;

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
    /// Run a core with no client and dump its video, audio and timings to a directory.
    Headless(HeadlessArgs),
    /// Load each core, print its name, version and extensions as JSON, and fail if any core
    /// cannot be loaded.
    Probe(ProbeArgs),
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
        Command::Headless(args) => headless::run(args),
        Command::Probe(args) => probe::run(args),
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
