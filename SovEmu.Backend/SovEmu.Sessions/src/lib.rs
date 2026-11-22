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
    fn session_ended(&self, _ended: &Ended<'_>) {}
}

/// Several observers told in turn, as one (the server's metrics and its failures list).
#[derive(Debug, Default)]
pub struct Observers(pub Vec<std::sync::Arc<dyn Observer>>);

impl Observer for Observers {
    fn session_started(&self, system: &str) {
        self.0.iter().for_each(|o| o.session_started(system));
    }

    fn session_ended(&self, ended: &Ended<'_>) {
        self.0.iter().for_each(|o| o.session_ended(ended));
    }
}

/// A worker that has exited: what it was playing, how far it got, why it ended, and what it
/// said last.
#[derive(Debug, Clone, Copy)]
pub struct Ended<'a> {
    pub session_id: &'a str,
    pub user_id: i64,
    pub game_id: i64,
    pub title: &'a str,
    pub system: &'a str,
    /// The core's file name without its extension (`snes9x_libretro`).
    pub core: &'a str,
    pub reason: EndReason,
    /// It had loaded its game; false for a worker that ended while loading.
    pub loaded: bool,
    /// Why its start failed (the worker's message, or the server's timeout), if it did.
    pub start_error: Option<&'a str>,
    pub code: Option<i32>,
    pub signal: Option<i32>,
    /// Its last lines on stderr (its log, and its core's), oldest first.
    pub last_lines: &'a [String],
    pub ran: Duration,
}

impl Ended<'_> {
    /// The session went wrong: its start failed, or it crashed, failed or had to be killed.
    pub fn failed(&self) -> bool {
        self.start_error.is_some()
            || matches!(self.reason, EndReason::Error | EndReason::Crashed | EndReason::Killed)
    }
}

/// Why a worker exited: what it said as it went, or what the server saw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndReason {
    /// Asked to (a player quit, the server stopping).
    Stopped,
    /// No client for its idle timeout.
    Idle,
    /// Its stdin closed under it.
    ControllerGone,
    /// It failed and said so (`exiting` with `error`, or a non-zero exit, a failed load included).
    Error,
    /// Killed by a signal nobody here sent: a crash, most often in the core.
    Crashed,
    /// Killed by the server: it did not stop when asked, or its start timed out.
    Killed,
    /// Exited cleanly without saying why.
    Exited,
}

impl EndReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stopped => "stopped",
            Self::Idle => "idle",
            Self::ControllerGone => "controller_gone",
            Self::Error => "error",
            Self::Crashed => "crashed",
            Self::Killed => "killed",
            Self::Exited => "exited",
        }
    }
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

impl Error {
    /// The start failed before any worker ran (no port free, the binary would not start), so
    /// no worker will report it.
    pub fn before_worker(&self) -> bool {
        matches!(self, Self::NoPort(_) | Self::Spawn(..))
    }
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
