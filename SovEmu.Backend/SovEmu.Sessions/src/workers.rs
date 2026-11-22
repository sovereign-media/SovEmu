//! Game sessions: one `sovemu-worker session` process each.
//!
//! Every session gets its own UDP port from a fixed range (publish that range from the
//! container) and its own worker, so a crashing core takes down only its session. The server
//! talks to a worker in JSON lines ([`sovemu_protocol::ipc`]), matching replies to requests by
//! id. A user has at most one session: starting a game ends the previous one, and a user's
//! starts run one at a time, so two quick starts never load two games side by side.
//!
//! Each worker has a reaper task that owns its process: it waits for the exit, returns the
//! port to the pool and runs the session's `after_exit`. Nothing else waits on the process, so
//! a worker that will not die (blocked in the kernel on a network share, where even SIGKILL
//! waits) is left to the reaper instead of blocking the next start.

use std::collections::{BTreeSet, HashMap};
use std::net::IpAddr;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use sovemu_protocol::ipc::SessionHealth;
use sovemu_protocol::ipc::{CoreCategory, CoreOption};
use sovemu_protocol::ipc::{ExitReason, Reply, Request};
use sovemu_protocol::{ClientStats, Pipeline, SessionDescription};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{oneshot, watch};
use tracing::Instrument;

use crate::starts::{Phase, Starts};
use crate::{EndReason, Ended, Error, Observer};

type Result<T> = std::result::Result<T, Error>;

/// How long a request to a worker may take.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// How long a killed worker gets to go before it is reported stuck and left to its reaper.
const KILL_WAIT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone)]
pub struct WorkerConfig {
    pub worker_bin: PathBuf,
    pub ports: (u16, u16),
    pub bind_ip: IpAddr,
    pub advertise_ip: Option<IpAddr>,
    /// BIOS files, a folder per system (see [`sovemu_library::bios`]).
    pub bios_dir: PathBuf,
    pub idle_timeout_secs: u64,
    /// The video bitrate a stream starts at, and the floor and ceiling it adapts between.
    pub video_kbps: u32,
    pub video_min_kbps: u32,
    pub video_max_kbps: u32,
    pub audio_kbps: u32,
    /// `--video-codec` and `--video-encoder` for workers (`SOVEMU_VIDEO_CODEC`,
    /// `SOVEMU_VIDEO_ENCODER`).
    pub video_codec: String,
    pub video_encoder: String,
    /// How long a worker may take to load its core and game. Generous by default: a disc image
    /// on a remote share takes tens of seconds to open.
    pub start_timeout: Duration,
    /// How long a worker may take to write its saves and exit before it is killed.
    pub stop_timeout: Duration,
    pub rewind_seconds: u32,
    pub rewind_max_mb: u32,
    pub fast_forward: u32,
    pub keyframe_interval: u32,
    /// Seconds between a worker's `session health` log lines (0: none).
    pub health_interval_secs: u32,
    /// `--gpu` for workers: a render node, `software` or `off` (see [`crate::gpu`]).
    pub gpu: String,
    /// How workers write their log lines (`--log-format`: `text` or `json`, as the server does);
    /// each also gets its session id.
    pub log_format: &'static str,
    /// Told when sessions start and end (the server records metrics).
    pub observer: Arc<dyn Observer>,
}

/// The only environment a worker inherits. Cores are third-party code: they get no credentials
/// (the NAS password, API keys) and nothing else of the server's environment.
pub(crate) const WORKER_ENV: &[&str] = &[
    "PATH",
    "HOME",
    "TMPDIR",
    "LANG",
    // The time zone, so a worker's log lines carry the same local times as the server's.
    "TZ",
    "RUST_LOG",
    "RUST_BACKTRACE",
    // Mesa's own knobs (driver choice, llvmpipe threads): tuning, never secrets.
    "MESA_LOADER_DRIVER_OVERRIDE",
    "GALLIUM_DRIVER",
    "LP_NUM_THREADS",
    // Where an instrumented build writes its coverage (cargo-llvm-cov sets it for test runs):
    // without it a worker started by a test writes `default_*.profraw` into the working folder,
    // and its coverage is lost.
    "LLVM_PROFILE_FILE",
    "LIBGL_ALWAYS_SOFTWARE",
];

/// The allow-listed part of this process's environment, for a worker.
pub fn worker_env() -> Vec<(String, String)> {
    WORKER_ENV.iter().filter_map(|k| std::env::var(k).ok().map(|v| ((*k).to_owned(), v))).collect()
}

/// What a session is playing, as the API reports it.
#[derive(Debug, Clone, Serialize)]
pub struct SessionInfo {
    pub id: String,
    pub game_id: i64,
    pub system: String,
    pub title: String,
    pub core: String,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub connected: bool,
    /// How many discs the game has (1 for most), and which is in the tray (1-based).
    pub discs: u32,
    pub disc: u32,
}

/// What to start.
impl std::fmt::Debug for Launch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Launch")
            .field("game_id", &self.game_id)
            .field("rom", &self.rom)
            .finish_non_exhaustive()
    }
}

pub struct Launch {
    pub user_id: i64,
    pub game_id: i64,
    pub system: String,
    pub title: String,
    pub core: PathBuf,
    pub rom: PathBuf,
    pub save_dir: PathBuf,
    pub state_dir: PathBuf,
    /// A soft patch to apply as the game loads, and its name (`patches.rs`).
    pub patch: Option<(PathBuf, String)>,
    /// Core options (`key`, `value`).
    pub variables: Vec<(String, String)>,
    /// Frames the picture runs ahead of the game, hiding its own input lag (0: off).
    pub run_ahead: u32,
    /// Run once the worker has exited (for example, to cache a large disc image only after the
    /// session that streamed it, so the copy does not compete with the game for the network).
    pub after_exit: Option<Box<dyn FnOnce() + Send + Sync>>,
    /// The client's id for following this start's progress.
    pub start_id: Option<String>,
}

pub struct Session {
    pub id: String,
    pub user_id: i64,
    port: u16,
    info: SessionInfo,
    stdin: tokio::sync::Mutex<ChildStdin>,
    process: Process,
    pending: Mutex<HashMap<u64, oneshot::Sender<Reply>>>,
    next_id: AtomicU64,
    connected: AtomicBool,
    /// The disc in the tray (1-based), after any change.
    disc: std::sync::atomic::AtomicU32,
    /// The options the core declared with its game loaded (the worker's `options` reply).
    declared: Mutex<Option<Declared>>,
    /// When it started, as a Unix time.
    started_at: f64,
    /// What the worker and the client last reported about the stream.
    live: Mutex<Live>,
    /// The `session` span: its id, game, system and user, on every line logged about it.
    span: tracing::Span,
}

/// What a session's worker and client last reported about its stream.
#[derive(Debug, Clone, Default)]
pub struct Live {
    /// How the worker makes the stream (renderer, codec, encoder and its fallbacks, audio).
    pub pipeline: Option<Pipeline>,
    /// The worker's last `session health` interval.
    pub health: Option<SessionHealth>,
    /// What the client's stats panel showed, and when (only while the panel is open).
    pub client_stats: Option<(ClientStats, Instant)>,
    /// The client playing it (`web`, `android 1.4.0 (…)`), as it named itself.
    pub client: Option<String>,
}

/// Client stats older than this are dropped: the panel was closed.
const CLIENT_STATS_FRESH: Duration = Duration::from_secs(5);

/// A core's options and categories, as a session reported them.
pub type Declared = (Vec<CoreOption>, Vec<CoreCategory>);

impl Session {
    /// The options the core declared once its game loaded, taken (once) to be remembered.
    pub fn take_declared(&self) -> Option<Declared> {
        self.declared.lock().unwrap_or_else(|e| e.into_inner()).take()
    }
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session").field("id", &self.id).field("port", &self.port).finish()
    }
}

/// What the reaper needs to say why a worker ended.
struct Exit {
    user_id: i64,
    game_id: i64,
    title: String,
    system: String,
    core: String,
    told: Arc<Mutex<Told>>,
    /// Closes once the start has settled (ready, failed or abandoned), so [`Told`] says how it
    /// went: the worker's exit and the start's outcome reach the server in either order.
    settled: oneshot::Receiver<()>,
    last_words: LastWords,
}

/// What the server learnt from a worker while it ran, for its reaper.
#[derive(Debug, Default)]
struct Told {
    /// It said it was ready: an end before that is an end while starting.
    loaded: bool,
    /// Why its start failed, if it did.
    start_error: Option<String>,
    /// What it said as it went ([`Reply::Exiting`]), if it did.
    exiting: Option<ExitReason>,
}

impl Told {
    fn change(told: &Mutex<Self>, change: impl FnOnce(&mut Self)) {
        change(&mut told.lock().unwrap_or_else(|e| e.into_inner()));
    }
}

/// Why a worker ended: the server's own kill first, then what the worker said, then how the
/// process ended.
fn end_reason(
    we_killed: bool,
    reported: Option<ExitReason>,
    signal: Option<i32>,
    code: Option<i32>,
) -> EndReason {
    if we_killed {
        return EndReason::Killed;
    }
    match reported {
        Some(ExitReason::Stopped) => EndReason::Stopped,
        Some(ExitReason::Idle) => EndReason::Idle,
        Some(ExitReason::ControllerGone) => EndReason::ControllerGone,
        Some(ExitReason::Error) => EndReason::Error,
        None if signal.is_some() => EndReason::Crashed,
        None if code.is_some_and(|c| c != 0) => EndReason::Error,
        None => EndReason::Exited,
    }
}

#[cfg(unix)]
fn signal_of(status: &std::process::ExitStatus) -> Option<i32> {
    std::os::unix::process::ExitStatusExt::signal(status)
}

#[cfg(not(unix))]
fn signal_of(_: &std::process::ExitStatus) -> Option<i32> {
    None
}

/// `snes9x_libretro` for `/opt/sovemu/cores/snes9x_libretro.so`: the metric's core label.
fn core_name(core: &std::path::Path) -> String {
    core.file_stem().map_or_else(String::new, |s| s.to_string_lossy().into_owned())
}

/// The worker's stderr (its log), passed through to the server's own and its last lines kept,
/// so a crash's log line carries what the worker (and its core) said just before.
struct LastWords {
    lines: Arc<Mutex<std::collections::VecDeque<String>>>,
    reader: tokio::task::JoinHandle<()>,
}

/// Lines kept for a crash's log line.
const LAST_WORDS: usize = 20;

impl LastWords {
    fn follow(stderr: tokio::process::ChildStderr) -> Self {
        let lines = Arc::new(Mutex::new(std::collections::VecDeque::with_capacity(LAST_WORDS)));
        let kept = lines.clone();
        let reader = tokio::spawn(async move {
            let mut stderr = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = stderr.next_line().await {
                // Still in the container's log, as when the worker wrote to it directly.
                eprintln!("{line}");
                let mut kept = kept.lock().unwrap_or_else(|e| e.into_inner());
                if kept.len() == LAST_WORDS {
                    kept.pop_front();
                }
                kept.push_back(line);
            }
        });
        Self { lines, reader }
    }

    /// The last lines, once the worker's stderr has closed (or after a second, if a child of
    /// its keeps it open).
    async fn finish(self) -> Vec<String> {
        let _ = tokio::time::timeout(Duration::from_secs(1), self.reader).await;
        self.lines.lock().unwrap_or_else(|e| e.into_inner()).iter().cloned().collect()
    }
}

/// A handle on a worker process, whose reaper task owns the [`Child`].
struct Process {
    pid: Option<u32>,
    /// Tells the reaper to kill the worker. Dropping it does not: a worker whose start was
    /// abandoned finishes loading, sees its stdin closed and exits cleanly.
    kill: Mutex<Option<oneshot::Sender<()>>>,
    /// True once the worker has exited.
    exited: watch::Receiver<bool>,
}

impl Process {
    /// Starts the reaper for `child`, which returns `port` and runs `after_exit` once it exits,
    /// and logs why it did.
    fn reap(
        mut child: Child,
        id: String,
        port: u16,
        workers: Arc<Workers>,
        after_exit: Option<Box<dyn FnOnce() + Send + Sync>>,
        span: tracing::Span,
        exit: Exit,
    ) -> Self {
        let pid = child.id();
        let started = Instant::now();
        let (kill, mut killed) = oneshot::channel::<()>();
        let (exit_tx, exited) = watch::channel(false);
        tokio::spawn(
            async move {
                let mut we_killed = false;
                let status = tokio::select! {
                    status = child.wait() => status,
                    Ok(()) = &mut killed => {
                        we_killed = true;
                        let _ = child.start_kill();
                        child.wait().await
                    }
                };
                let ran = started.elapsed();
                // Its last lines (on stderr, after its exit) come in a moment after the exit.
                let last_words = exit.last_words.finish().await;
                let _ = exit.settled.await;
                let told =
                    std::mem::take(&mut *exit.told.lock().unwrap_or_else(|e| e.into_inner()));
                let signal = status.as_ref().ok().and_then(signal_of);
                let code = status.as_ref().ok().and_then(std::process::ExitStatus::code);
                let reason = end_reason(we_killed, told.exiting, signal, code);
                if matches!(reason, EndReason::Crashed | EndReason::Killed) {
                    tracing::warn!(
                        reason = reason.as_str(),
                        signal,
                        code,
                        core = %exit.core,
                        duration_s = ran.as_secs(),
                        last_lines = %last_words.join("\n"),
                        "session ended"
                    );
                } else {
                    tracing::info!(
                        reason = reason.as_str(),
                        code,
                        core = %exit.core,
                        duration_s = ran.as_secs(),
                        "session ended"
                    );
                }
                workers.config.observer.session_ended(&Ended {
                    user_id: exit.user_id,
                    game_id: exit.game_id,
                    title: &exit.title,
                    system: &exit.system,
                    core: &exit.core,
                    reason,
                    ran,
                    loaded: told.loaded,
                    start_error: told.start_error.as_deref(),
                    code,
                    signal,
                    last_lines: &last_words,
                });
                let _ = exit_tx.send(true);
                workers.forget(&id, port);
                if let Some(after) = after_exit {
                    after();
                }
            }
            .instrument(span),
        );
        Self { pid, kill: Mutex::new(Some(kill)), exited }
    }

    fn kill(&self) {
        if let Some(kill) = self.kill.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = kill.send(());
        }
    }

    /// Waits until the worker has exited, or `deadline`; true if it exited.
    async fn exited_by(&self, deadline: tokio::time::Instant) -> bool {
        let mut exited = self.exited.clone();
        matches!(tokio::time::timeout_at(deadline, exited.wait_for(|e| *e)).await, Ok(Ok(_)))
    }
}

impl Session {
    pub fn info(&self) -> SessionInfo {
        SessionInfo {
            connected: self.connected.load(Ordering::Relaxed),
            disc: self.disc.load(Ordering::Relaxed),
            ..self.info.clone()
        }
    }

    /// When the session started, as a Unix time.
    pub fn started_at(&self) -> f64 {
        self.started_at
    }

    /// What the worker and client last reported, without client stats gone stale.
    pub fn live(&self) -> Live {
        let mut live = self.live.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if live.client_stats.as_ref().is_some_and(|(_, at)| at.elapsed() > CLIENT_STATS_FRESH) {
            live.client_stats = None;
        }
        live
    }

    /// Records the client that sent the current offer.
    pub fn set_client(&self, client: Option<String>) {
        self.with_live(|live| live.client = client);
    }

    fn with_live(&self, change: impl FnOnce(&mut Live)) {
        change(&mut self.live.lock().unwrap_or_else(|e| e.into_inner()));
    }

    /// Records a disc change the worker made.
    pub fn set_current_disc(&self, disc: u32) {
        self.disc.store(disc, Ordering::Relaxed);
    }

    /// Sends a request and waits for its reply.
    pub async fn request(&self, make: impl FnOnce(u64) -> Request) -> Result<Reply> {
        self.request_within(REQUEST_TIMEOUT, make).await
    }

    /// Sends a request and waits up to `limit` for its reply.
    async fn request_within(
        &self,
        limit: Duration,
        make: impl FnOnce(u64) -> Request,
    ) -> Result<Reply> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let request = make(id);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap_or_else(|e| e.into_inner()).insert(id, tx);
        let mut line = serde_json::to_string(&request)?;
        line.push('\n');
        let result = async {
            {
                let mut stdin = self.stdin.lock().await;
                stdin.write_all(line.as_bytes()).await.map_err(|_| Error::Closed)?;
                stdin.flush().await.map_err(|_| Error::Closed)?;
            }
            match tokio::time::timeout(limit, rx).await {
                Ok(Ok(Reply::Error { message, .. })) => Err(Error::Worker(message)),
                Ok(Ok(reply)) => Ok(reply),
                Ok(Err(_)) => Err(Error::Ended),
                Err(_) => Err(Error::Timeout("the session did not answer in time")),
            }
        }
        .await;
        if result.is_err() {
            self.pending.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
        }
        result
    }

    pub async fn offer(&self, offer: SessionDescription) -> Result<SessionDescription> {
        match self.request(|id| Request::Offer { id, offer }).await? {
            Reply::Answer { answer, .. } => Ok(answer),
            other => Err(Error::Unexpected(format!("{other:?}"))),
        }
    }
}

#[derive(Debug, Default)]
struct Inner {
    sessions: HashMap<String, Arc<Session>>,
    free_ports: BTreeSet<u16>,
    /// One lock per user, held for the whole of a start (stopping the old session included).
    starting: HashMap<i64, Arc<tokio::sync::Mutex<()>>>,
}

#[derive(Debug)]
pub struct Workers {
    config: WorkerConfig,
    inner: Mutex<Inner>,
    /// Starts' progress, for clients' loading bars.
    pub starts: Starts,
}

impl Workers {
    pub fn new(config: WorkerConfig) -> Arc<Self> {
        let free_ports = (config.ports.0..=config.ports.1).collect();
        Arc::new(Self {
            config,
            inner: Mutex::new(Inner { free_ports, ..Inner::default() }),
            starts: Starts::default(),
        })
    }

    /// The worker binary, for one-off jobs (probing a core's settings).
    pub fn worker_bin(&self) -> PathBuf {
        self.config.worker_bin.clone()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn get(&self, id: &str) -> Option<Arc<Session>> {
        self.lock().sessions.get(id).cloned()
    }

    pub fn for_user(&self, user_id: i64) -> Option<Arc<Session>> {
        self.lock().sessions.values().find(|s| s.user_id == user_id).cloned()
    }

    /// Every running session, oldest first.
    pub fn all(&self) -> Vec<Arc<Session>> {
        let mut all: Vec<_> = self.lock().sessions.values().cloned().collect();
        all.sort_by(|a, b| a.started_at.total_cmp(&b.started_at));
        all
    }

    pub fn count(&self) -> usize {
        self.lock().sessions.len()
    }

    /// Starts a session, ending the user's current one first. A user's starts take turns: a
    /// second one waits for the first to finish loading, then ends its session.
    pub async fn start(self: &Arc<Self>, mut launch: Launch) -> Result<Arc<Session>> {
        let turn = self.lock().starting.entry(launch.user_id).or_default().clone();
        let _turn = turn.lock().await;
        let phase = |phase| {
            if let Some(id) = &launch.start_id {
                self.starts.update(id, phase, None, None, Instant::now());
            }
        };
        if let Some(existing) = self.for_user(launch.user_id) {
            phase(Phase::Stopping);
            self.stop(&existing.id).await;
        }
        phase(Phase::Launching);
        let port =
            self.lock().free_ports.pop_first().ok_or_else(|| Error::NoPort(self.port_count()))?;
        let after_exit = launch.after_exit.take();
        // From here the port goes back to the pool when the worker exits (`Process::reap`).
        let session = self.spawn(&launch, port, after_exit).await?;
        self.lock().sessions.insert(session.id.clone(), session.clone());
        Ok(session)
    }

    /// Ports held: by a session, or by a worker that has not exited yet.
    pub fn ports_in_use(&self) -> usize {
        self.port_count() - self.lock().free_ports.len()
    }

    /// The UDP ports there are, one per concurrent session.
    pub fn port_count(&self) -> usize {
        (self.config.ports.1 - self.config.ports.0) as usize + 1
    }

    async fn spawn(
        self: &Arc<Self>,
        launch: &Launch,
        port: u16,
        after_exit: Option<Box<dyn FnOnce() + Send + Sync>>,
    ) -> Result<Arc<Session>> {
        std::fs::create_dir_all(&launch.save_dir)?;
        // Most systems need no BIOS, so a missing folder is no reason to refuse the game.
        let bios = self.config.bios_dir.join(&launch.system);
        if let Err(e) = std::fs::create_dir_all(&bios) {
            tracing::debug!(path = %bios.display(), error = %e, "cannot create the BIOS folder");
        }
        let id = random_id();
        let span = tracing::info_span!(
            "session",
            session.id = %id,
            game.id = launch.game_id,
            system = %launch.system,
            user.id = launch.user_id,
        );
        // Everything that decides how the game runs, so a report about one game can be read
        // against what it was started with.
        tracing::info!(
            parent: &span,
            game = %launch.title,
            core = %launch.core.display(),
            rom = %launch.rom.display(),
            patch = launch.patch.as_ref().map(|(_, name)| name.as_str()),
            options = launch.variables.len(),
            run_ahead = launch.run_ahead,
            udp.port = port,
            "session starting"
        );
        let started = Instant::now();
        let mut command = Command::new(&self.config.worker_bin);
        command.env_clear();
        for var in WORKER_ENV {
            if let Some(value) = std::env::var_os(var) {
                command.env(var, value);
            }
        }
        command
            .arg("session")
            .arg("--video-kbps")
            .arg(self.config.video_kbps.to_string())
            .arg("--video-min-kbps")
            .arg(self.config.video_min_kbps.to_string())
            .arg("--video-max-kbps")
            .arg(self.config.video_max_kbps.to_string())
            .arg("--audio-kbps")
            .arg(self.config.audio_kbps.to_string())
            .arg("--video-codec")
            .arg(&self.config.video_codec)
            .arg("--video-encoder")
            .arg(&self.config.video_encoder)
            .arg("--core")
            .arg(&launch.core)
            .arg("--rom")
            .arg(&launch.rom)
            .arg("--udp")
            .arg(format!("{}:{port}", self.config.bind_ip))
            .arg("--system-dir")
            .arg(&bios)
            .arg("--bios-path")
            .arg(sovemu_library::bios::core_path(&launch.system))
            .arg("--save-dir")
            .arg(&launch.save_dir)
            .arg("--state-dir")
            .arg(&launch.state_dir)
            .arg("--idle-timeout")
            .arg(self.config.idle_timeout_secs.to_string())
            .arg("--rewind-seconds")
            .arg(self.config.rewind_seconds.to_string())
            .arg("--rewind-max-mb")
            .arg(self.config.rewind_max_mb.to_string())
            .arg("--fast-forward")
            .arg(self.config.fast_forward.to_string())
            .arg("--keyframe-interval")
            .arg(self.config.keyframe_interval.to_string())
            .arg("--gpu")
            .arg(&self.config.gpu)
            .arg("--health-interval")
            .arg(self.config.health_interval_secs.to_string())
            .arg("--session-id")
            .arg(&id)
            .arg("--system")
            .arg(&launch.system)
            .arg("--log-format")
            .arg(self.config.log_format);
        if let Some((patch, name)) = &launch.patch {
            command.arg("--patch").arg(patch).arg("--patch-name").arg(name);
        }
        for (key, value) in &launch.variables {
            command.arg("--var").arg(format!("{key}={value}"));
        }
        if launch.run_ahead > 0 {
            command.arg("--run-ahead").arg(launch.run_ahead.to_string());
        }
        if let Some(ip) = self.config.advertise_ip {
            command.arg("--advertise-ip").arg(ip.to_string());
        }
        let spawned = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| Error::Spawn(self.config.worker_bin.clone(), e));
        let mut child = match spawned {
            Ok(child) => child,
            Err(e) => {
                self.lock().free_ports.insert(port);
                return Err(e);
            }
        };
        let stdin = child.stdin.take().expect("piped");
        let mut lines = BufReader::new(child.stdout.take().expect("piped")).lines();
        let told = Arc::new(Mutex::new(Told::default()));
        // Held until this start returns, however it does.
        let (_settling, settled) = oneshot::channel::<()>();
        let last_words = LastWords::follow(child.stderr.take().expect("piped"));
        let process = Process::reap(
            child,
            id.clone(),
            port,
            self.clone(),
            after_exit,
            span.clone(),
            Exit {
                user_id: launch.user_id,
                game_id: launch.game_id,
                title: launch.title.clone(),
                system: launch.system.clone(),
                core: core_name(&launch.core),
                told: told.clone(),
                settled,
                last_words,
            },
        );

        let mut declared = None;
        // When the core had loaded and the game began to: splits the load time in two.
        let mut content_from = None;
        let ready = tokio::time::timeout(self.config.start_timeout, async {
            while let Some(line) = lines.next_line().await? {
                match serde_json::from_str::<Reply>(&line) {
                    Ok(Reply::Failed { message }) => return Err(Error::Worker(message)),
                    Ok(Reply::Ready { core, width, height, fps, discs, .. }) => {
                        return Ok((core, width, height, fps, discs));
                    }
                    Ok(Reply::Options { options, categories }) => {
                        declared = Some((options, categories));
                    }
                    Ok(Reply::Progress { phase, read, total }) => {
                        if phase == sovemu_protocol::ipc::LoadPhase::Content {
                            content_from.get_or_insert_with(Instant::now);
                        }
                        if let Some(id) = &launch.start_id {
                            self.starts.update(id, phase.into(), read, total, Instant::now());
                        }
                    }
                    Ok(other) => tracing::debug!(reply = ?other, "worker replied before ready"),
                    Err(_) => {
                        tracing::debug!(line = %line, "worker wrote a line that is not a reply")
                    }
                }
            }
            Err(Error::Exited)
        })
        .await;
        let ready = match ready
            .unwrap_or(Err(Error::Timeout("the worker took too long to load the game")))
        {
            Ok(ready) => {
                Told::change(&told, |t| t.loaded = true);
                ready
            }
            Err(e) => {
                Told::change(&told, |t| t.start_error = Some(e.to_string()));
                if matches!(e, Error::Timeout(_)) {
                    process.kill();
                }
                tracing::warn!(
                    parent: &span,
                    error = %e,
                    load_ms = started.elapsed().as_millis() as u64,
                    "session failed to start"
                );
                return Err(e);
            }
        };
        let ready_at = Instant::now();
        let core_ms = content_from.unwrap_or(ready_at).duration_since(started).as_millis() as u64;
        let content_ms = content_from.map(|from| ready_at.duration_since(from).as_millis() as u64);

        let (core, width, height, fps, discs) = ready;
        let session = Arc::new(Session {
            id: id.clone(),
            user_id: launch.user_id,
            port,
            info: SessionInfo {
                id: id.clone(),
                game_id: launch.game_id,
                system: launch.system.clone(),
                title: launch.title.clone(),
                core,
                width,
                height,
                fps,
                connected: false,
                discs: discs.unwrap_or(1),
                disc: 1,
            },
            stdin: tokio::sync::Mutex::new(stdin),
            process,
            pending: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            connected: AtomicBool::new(false),
            disc: std::sync::atomic::AtomicU32::new(1),
            declared: Mutex::new(declared),
            started_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0.0, |d| d.as_secs_f64()),
            live: Mutex::new(Live::default()),
            span: span.clone(),
        });
        tracing::info!(
            parent: &span,
            game = %launch.title,
            udp.port = port,
            core.name = %session.info.core,
            width = session.info.width,
            height = session.info.height,
            fps = session.info.fps,
            run_ahead = launch.run_ahead,
            load_ms = ready_at.duration_since(started).as_millis() as u64,
            core_ms,
            content_ms,
            "session started"
        );
        self.config.observer.session_started(&launch.system);

        let reader = session.clone();
        tokio::spawn(async move {
            while let Ok(Some(line)) = lines.next_line().await {
                let Ok(reply) = serde_json::from_str::<Reply>(&line) else { continue };
                match reply {
                    Reply::Exiting { reason } => Told::change(&told, |t| t.exiting = Some(reason)),
                    Reply::ClientConnected => reader.connected.store(true, Ordering::Relaxed),
                    Reply::ClientDisconnected => {
                        reader.connected.store(false, Ordering::Relaxed);
                        reader.with_live(|live| live.client_stats = None);
                    }
                    Reply::Pipeline { pipeline } => {
                        reader.with_live(|live| live.pipeline = Some(pipeline));
                    }
                    Reply::Health { health } => reader.with_live(|live| live.health = Some(health)),
                    Reply::ClientStats { stats } => {
                        reader.with_live(|live| live.client_stats = Some((stats, Instant::now())));
                    }
                    Reply::Answer { id, .. } | Reply::Done { id } | Reply::Error { id, .. } => {
                        let waiter =
                            reader.pending.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
                        if let Some(tx) = waiter {
                            let _ = tx.send(reply);
                        }
                    }
                    Reply::Ready { .. }
                    | Reply::Progress { .. }
                    | Reply::Options { .. }
                    | Reply::Failed { .. } => {}
                }
            }
            // The worker is going (idle timeout, shutdown, a crash): nothing more will answer.
            reader.pending.lock().unwrap_or_else(|e| e.into_inner()).clear();
        });
        Ok(session)
    }

    /// The worker of session `id` has exited: its port is free again.
    fn forget(&self, id: &str, port: u16) {
        let mut inner = self.lock();
        inner.sessions.remove(id);
        inner.free_ports.insert(port);
    }

    /// Ends a session: asks the worker to write its saves and exit, and kills it only if it has
    /// not gone within the stop timeout. Returns once it has exited or is known to be stuck (a
    /// stuck worker keeps its port until it finally exits).
    pub async fn stop(&self, id: &str) {
        let Some(session) = self.lock().sessions.remove(id) else { return };
        let limit = self.config.stop_timeout;
        let deadline = tokio::time::Instant::now() + limit;
        if let Err(e) = session.request_within(limit, |id| Request::Shutdown { id }).await {
            tracing::warn!(parent: &session.span, error = %format!("{e:#}"), "session did not acknowledge shutdown");
        }
        if session.process.exited_by(deadline).await {
            return;
        }
        tracing::warn!(parent: &session.span, ?limit, "session still running after shutdown; killing it");
        session.process.kill();
        if !session.process.exited_by(tokio::time::Instant::now() + KILL_WAIT).await {
            tracing::error!(
                parent: &session.span,
                pid = ?session.process.pid,
                "session did not die when killed: it is blocked in the kernel, most likely on \
                 the network share; leaving it"
            );
        }
    }

    /// Ends every session at once (server shutdown).
    pub async fn stop_all(self: &Arc<Self>) {
        let ids: Vec<String> = self.lock().sessions.keys().cloned().collect();
        let mut stops = tokio::task::JoinSet::new();
        for id in ids {
            let workers = self.clone();
            stops.spawn(async move { workers.stop(&id).await });
        }
        stops.join_all().await;
    }
}

fn random_id() -> String {
    let mut bytes = [0u8; 12];
    getrandom::fill(&mut bytes).expect("the OS random source works");
    hex::encode(bytes)
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    use super::*;

    /// A stand-in worker: says it is ready, then answers every request with `done`. After a
    /// shutdown it runs `then` (`exit 0`, or a sleep for a worker slow to go).
    const WORKER: &str = r#"#!/bin/sh
echo "$@" > "$(dirname "$0")/args"
echo '{"type":"ready","udp_port":1,"core":"fake","width":320,"height":240,"fps":60.0}'
while read -r line; do
  id=$(echo "$line" | sed 's/.*"id":\([0-9]*\).*/\1/')
  echo "{\"type\":\"done\",\"id\":$id}"
  case "$line" in *shutdown*) THEN ;; esac
done
"#;

    fn dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("sovemu-workers-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Writes an executable script. Tests run in parallel and spawn processes, and a child forked
    /// while this process holds the script open for writing keeps that handle until it execs;
    /// running the script meanwhile fails with ETXTBSY ("Text file busy"). So `cp` writes it: the
    /// handle only ever exists in cp's process, which no test's child can inherit.
    fn write_script(path: &Path, contents: &str) {
        let source = path.with_extension("src");
        std::fs::write(&source, contents).unwrap();
        let copied = std::process::Command::new("cp").arg(&source).arg(path).status().unwrap();
        assert!(copied.success(), "cp {} {}", source.display(), path.display());
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn workers(dir: &Path, then: &str, stop_timeout: Duration) -> Arc<Workers> {
        workers_with(dir, &WORKER.replace("THEN", then), stop_timeout, Arc::new(()))
    }

    /// Whether it had loaded, its exit code, its last lines, and why its start failed.
    type Details = (bool, Option<i32>, Vec<String>, Option<String>);

    /// Remembers why each session ended, and its core.
    #[derive(Debug, Default)]
    struct Recorder(Mutex<Vec<(EndReason, String)>>, Mutex<Vec<Details>>);

    impl Observer for Recorder {
        fn session_ended(&self, ended: &Ended<'_>) {
            self.0.lock().unwrap().push((ended.reason, ended.core.to_owned()));
            self.1.lock().unwrap().push((
                ended.loaded,
                ended.code,
                ended.last_lines.to_vec(),
                ended.start_error.map(str::to_owned),
            ));
        }
    }

    impl Recorder {
        /// The reasons recorded, once `n` sessions have ended.
        async fn ended(&self, n: usize) -> Vec<EndReason> {
            for _ in 0..200 {
                let ended = self.0.lock().unwrap().clone();
                if ended.len() >= n {
                    return ended.into_iter().map(|(reason, _)| reason).collect();
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            panic!("{n} session(s) did not end: {:?}", self.0.lock().unwrap());
        }
    }

    fn workers_with(
        dir: &Path,
        script: &str,
        stop_timeout: Duration,
        observer: Arc<dyn Observer>,
    ) -> Arc<Workers> {
        let bin = dir.join("worker.sh");
        write_script(&bin, script);
        Workers::new(WorkerConfig {
            worker_bin: bin,
            ports: (40000, 40001),
            bind_ip: "127.0.0.1".parse().unwrap(),
            advertise_ip: None,
            bios_dir: dir.join("bios"),
            idle_timeout_secs: 0,
            video_kbps: 1000,
            video_min_kbps: 300,
            video_max_kbps: 2000,
            audio_kbps: 64,
            video_codec: "auto".into(),
            video_encoder: "software".into(),
            start_timeout: Duration::from_secs(10),
            stop_timeout,
            rewind_seconds: 0,
            rewind_max_mb: 0,
            fast_forward: 1,
            keyframe_interval: 60,
            health_interval_secs: 10,
            gpu: "off".into(),
            log_format: "text",
            observer,
        })
    }

    fn launch(dir: &Path, user_id: i64, game_id: i64) -> Launch {
        Launch {
            user_id,
            game_id,
            system: "nes".into(),
            title: format!("game {game_id}"),
            core: dir.join("core.so"),
            rom: dir.join("game.nes"),
            save_dir: dir.join("saves"),
            state_dir: dir.join("states"),
            patch: None,
            variables: Vec::new(),
            run_ahead: 0,
            after_exit: None,
            start_id: None,
        }
    }

    fn free_ports(workers: &Workers) -> usize {
        workers.lock().free_ports.len()
    }

    /// The reaper returns a port just after the exit; give it a moment.
    async fn settle(workers: &Workers, ports: usize) {
        for _ in 0..100 {
            if free_ports(workers) == ports {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(free_ports(workers), ports, "ports back in the pool");
    }

    #[tokio::test]
    async fn a_new_start_ends_the_previous_session_and_reuses_its_port() {
        let dir = dir("restart");
        let workers = workers(&dir, "exit 0", Duration::from_secs(5));
        for game in 1..=5 {
            let session = workers.start(launch(&dir, 1, game)).await.unwrap();
            assert_eq!(session.info().game_id, game);
            assert_eq!(workers.count(), 1, "one session per user");
        }
        workers.stop_all().await;
        settle(&workers, 2).await;
        assert_eq!(workers.count(), 0);
    }

    #[tokio::test]
    async fn a_worker_logs_with_its_session_id_and_the_servers_format() {
        let dir = dir("args");
        let workers = workers(&dir, "exit 0", Duration::from_secs(5));
        let session = workers.start(launch(&dir, 1, 1)).await.unwrap();
        let args = std::fs::read_to_string(dir.join("args")).unwrap();
        assert!(args.contains(&format!("--session-id {}", session.id)), "{args}");
        assert!(args.contains("--log-format text"), "{args}");
        workers.stop_all().await;
    }

    #[tokio::test]
    async fn run_ahead_reaches_the_worker_only_when_on() {
        let dir = dir("runahead");
        let workers = workers(&dir, "exit 0", Duration::from_secs(5));
        workers.start(Launch { run_ahead: 2, ..launch(&dir, 1, 1) }).await.unwrap();
        let args = std::fs::read_to_string(dir.join("args")).unwrap();
        assert!(args.contains("--run-ahead 2"), "{args}");
        workers.start(launch(&dir, 1, 2)).await.unwrap();
        let args = std::fs::read_to_string(dir.join("args")).unwrap();
        assert!(!args.contains("--run-ahead"), "off is the worker's default: {args}");
        workers.stop_all().await;
        settle(&workers, 2).await;
    }

    #[test]
    fn why_a_worker_ended() {
        use EndReason as R;
        assert_eq!(end_reason(true, Some(ExitReason::Stopped), None, Some(0)), R::Killed);
        assert_eq!(end_reason(false, Some(ExitReason::Idle), None, Some(0)), R::Idle);
        assert_eq!(end_reason(false, Some(ExitReason::Error), None, Some(1)), R::Error);
        assert_eq!(end_reason(false, None, Some(11), None), R::Crashed, "a segfault");
        assert_eq!(end_reason(false, None, None, Some(1)), R::Error, "a failed load");
        assert_eq!(end_reason(false, None, None, Some(0)), R::Exited);
    }

    #[tokio::test]
    async fn a_worker_that_says_why_it_stops_is_recorded_so() {
        let dir = dir("exiting");
        let recorder = Arc::new(Recorder::default());
        let then = r#"echo '{"type":"exiting","reason":"stopped"}'; exit 0"#;
        let workers = workers_with(
            &dir,
            &WORKER.replace("THEN", then),
            Duration::from_secs(5),
            recorder.clone(),
        );
        workers.start(launch(&dir, 1, 1)).await.unwrap();
        workers.stop_all().await;
        assert_eq!(recorder.ended(1).await, [EndReason::Stopped]);
        assert_eq!(recorder.0.lock().unwrap()[0].1, "core", "the core's file name");
    }

    #[tokio::test]
    async fn a_worker_killed_by_a_signal_is_a_crash() {
        let dir = dir("crash");
        let recorder = Arc::new(Recorder::default());
        let crash = r#"#!/bin/sh
echo '{"type":"ready","udp_port":1,"core":"fake","width":320,"height":240,"fps":60.0}'
echo "the core's last words" >&2
kill -SEGV $$
"#;
        let workers = workers_with(&dir, crash, Duration::from_secs(5), recorder.clone());
        workers.start(launch(&dir, 1, 1)).await.unwrap();
        assert_eq!(recorder.ended(1).await, [EndReason::Crashed]);
        let (loaded, _, lines, start_error) = recorder.1.lock().unwrap()[0].clone();
        assert!(loaded, "it had loaded: a crash while playing");
        assert_eq!(start_error, None);
        assert_eq!(lines, ["the core's last words"], "what to tell the admin");
        settle(&workers, 2).await;
    }

    #[tokio::test]
    async fn a_game_that_fails_to_load_says_so_with_its_last_words() {
        let dir = dir("load-fails");
        let recorder = Arc::new(Recorder::default());
        let fails = r#"#!/bin/sh
echo "missing BIOS: scph5501.bin" >&2
echo '{"type":"failed","message":"the core could not load the game"}'
exit 2
"#;
        let workers = workers_with(&dir, fails, Duration::from_secs(5), recorder.clone());
        assert!(workers.start(launch(&dir, 1, 1)).await.is_err());
        assert_eq!(recorder.ended(1).await, [EndReason::Error]);
        let (loaded, code, lines, start_error) = recorder.1.lock().unwrap()[0].clone();
        assert_eq!((loaded, code), (false, Some(2)), "ended while starting");
        assert_eq!(lines, ["missing BIOS: scph5501.bin"]);
        assert_eq!(start_error.as_deref(), Some("the core could not load the game"));
        settle(&workers, 2).await;
    }

    #[tokio::test]
    async fn a_worker_that_exits_quietly_while_loading_is_a_failed_start() {
        let dir = dir("load-quiet");
        let recorder = Arc::new(Recorder::default());
        let workers =
            workers_with(&dir, "#!/bin/sh\nexit 0\n", Duration::from_secs(5), recorder.clone());
        assert!(workers.start(launch(&dir, 1, 1)).await.is_err());
        assert_eq!(recorder.ended(1).await, [EndReason::Exited], "a clean exit, but no game");
        let (loaded, _, _, start_error) = recorder.1.lock().unwrap()[0].clone();
        assert!(!loaded);
        assert_eq!(start_error.as_deref(), Some("the worker exited while loading the game"));
        settle(&workers, 2).await;
    }

    #[tokio::test]
    async fn a_worker_binary_that_will_not_start_fails_before_any_worker() {
        let dir = dir("no-binary");
        let workers = workers(&dir, "exit 0", Duration::from_secs(5));
        let workers = Workers::new(WorkerConfig {
            worker_bin: dir.join("missing"),
            ..workers.config.clone()
        });
        let e = workers.start(launch(&dir, 1, 1)).await.unwrap_err();
        assert!(e.before_worker(), "{e}");
        settle(&workers, 2).await;
    }

    #[test]
    fn a_failed_start_or_a_crash_is_a_failure_and_quitting_is_not() {
        let ended = |reason, start_error: Option<&'static str>| Ended {
            user_id: 1,
            game_id: 1,
            title: "",
            system: "nes",
            core: "core",
            reason,
            ran: Duration::ZERO,
            loaded: start_error.is_none(),
            code: None,
            signal: None,
            last_lines: &[],
            start_error,
        };
        for reason in [EndReason::Stopped, EndReason::Idle, EndReason::ControllerGone] {
            assert!(!ended(reason, None).failed(), "{reason:?}");
        }
        for reason in [EndReason::Error, EndReason::Crashed, EndReason::Killed] {
            assert!(ended(reason, None).failed(), "{reason:?}");
        }
        assert!(ended(EndReason::Exited, Some("exited while loading")).failed());
    }

    #[tokio::test]
    async fn a_worker_the_server_kills_is_recorded_as_killed() {
        let dir = dir("killed");
        let recorder = Arc::new(Recorder::default());
        let script = WORKER.replace("THEN", "exec sleep 600");
        let workers = workers_with(&dir, &script, Duration::from_millis(300), recorder.clone());
        workers.start(launch(&dir, 1, 1)).await.unwrap();
        workers.stop_all().await;
        assert_eq!(recorder.ended(1).await, [EndReason::Killed]);
    }

    #[tokio::test]
    async fn a_worker_slow_to_exit_is_killed_after_the_stop_timeout() {
        let dir = dir("slow");
        let workers = workers(&dir, "exec sleep 600", Duration::from_millis(500));
        let first = workers.start(launch(&dir, 1, 1)).await.unwrap();
        let began = Instant::now();
        let second = workers.start(launch(&dir, 1, 2)).await.unwrap();
        let took = began.elapsed();
        assert!(took >= Duration::from_millis(500), "it had its stop timeout: {took:?}");
        assert!(took < Duration::from_secs(5), "then was killed: {took:?}");
        assert!(*first.process.exited.borrow(), "the first worker is gone");
        assert_eq!(workers.get(&second.id).unwrap().info().game_id, 2);
        workers.stop_all().await;
        settle(&workers, 2).await;
    }

    #[tokio::test]
    async fn a_users_starts_take_turns() {
        let dir = dir("turns");
        let workers = workers(&dir, "exit 0", Duration::from_secs(5));
        let (a, b) =
            tokio::join!(workers.start(launch(&dir, 1, 1)), workers.start(launch(&dir, 1, 2)));
        let (a, b) = (a.unwrap(), b.unwrap());
        assert_eq!(workers.count(), 1, "the second start ended the first: never two at once");
        let live = workers.for_user(1).unwrap();
        assert!(live.id == a.id || live.id == b.id);
        workers.stop_all().await;
        settle(&workers, 2).await;
    }

    #[tokio::test]
    async fn an_abandoned_start_does_not_leak_its_port() {
        let dir = dir("abandoned");
        let workers = workers(&dir, "exit 0", Duration::from_secs(5));
        // Cancel a start while it waits for the worker (as when the browser leaves the page).
        let slow = dir.join("slow.sh");
        write_script(&slow, "#!/bin/sh\nsleep 0.3\nexec \"$0.real\" \"$@\"\n");
        std::fs::rename(dir.join("worker.sh"), dir.join("slow.sh.real")).unwrap();
        let workers = Workers::new(WorkerConfig { worker_bin: slow, ..workers.config.clone() });
        let start = workers.start(launch(&dir, 1, 1));
        assert!(tokio::time::timeout(Duration::from_millis(50), start).await.is_err());
        assert_eq!(free_ports(&workers), 1, "the abandoned worker still has its port");
        assert_eq!((workers.ports_in_use(), workers.count()), (1, 0), "held without a session");
        // It finishes loading, finds its stdin closed, and exits on its own.
        settle(&workers, 2).await;
        assert_eq!((workers.ports_in_use(), workers.port_count()), (0, 2));
    }
}
