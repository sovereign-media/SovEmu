//! Game sessions: one `sovemu-worker session` process per game, started, spoken to over JSON
//! lines on its stdin and stdout (`sovemu_protocol::ipc`), and stopped so its saves are written.
//!
//! - [`workers`]: the running sessions, their UDP ports and the worker processes, with the
//!   allow-listed environment they get (no secrets).
//! - [`starts`]: the progress of games being started, for the clients' loading screens.
//! - [`gpu`]: where 3D systems render, as `sovemu-worker gpu` reports.
//!
//! The server never runs a core: this crate starts the processes that do. It knows nothing about
//! HTTP or the database; it reports sessions starting and ending to an [`Observer`] (the server
//! records metrics).

pub mod gpu;
pub mod starts;
pub mod workers;

use std::path::PathBuf;
use std::time::Duration;

/// Told when sessions start and end. Every method does nothing by default.
pub trait Observer: Send + Sync + std::fmt::Debug {
    fn session_started(&self, _system: &str) {}
    fn session_ended(&self, _system: &str, _ran: Duration) {}
}

/// Nobody listening (tests).
impl Observer for () {}

/// Why a session could not be started or asked something.
#[derive(Debug)]
pub enum Error {
    /// Every session port is in use (how many there are).
    NoPort(usize),
    /// The worker binary could not be started.
    Spawn(PathBuf, std::io::Error),
    /// The worker said no, or failed: its message.
    Worker(String),
    /// The worker exited while loading the game.
    Exited,
    /// The session ended before answering.
    Ended,
    /// The worker's stdin is closed.
    Closed,
    /// Waited too long: for what.
    Timeout(&'static str),
    /// A reply that does not answer the request.
    Unexpected(String),
    Io(std::io::Error),
    Json(serde_json::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoPort(n) => write!(f, "all {n} session ports are in use"),
            Self::Spawn(bin, e) => write!(f, "starting {}: {e}", bin.display()),
            Self::Worker(message) => f.write_str(message),
            Self::Exited => f.write_str("the worker exited while loading the game"),
            Self::Ended => f.write_str("the session ended"),
            Self::Closed => f.write_str("worker stdin closed"),
            Self::Timeout(what) => f.write_str(what),
            Self::Unexpected(reply) => write!(f, "unexpected reply {reply}"),
            Self::Io(e) => e.fmt(f),
            Self::Json(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Spawn(_, e) | Self::Io(e) => Some(e),
            Self::Json(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}
