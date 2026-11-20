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

use anyhow::{Context, Result, anyhow, bail};
use serde::Serialize;
use sovemu_protocol::SessionDescription;
use sovemu_protocol::ipc::{CoreCategory, CoreOption};
use sovemu_protocol::ipc::{Reply, Request};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{oneshot, watch};
use tracing::Instrument;

use crate::starts::{Phase, Starts};

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
    /// BIOS files, a folder per system (see [`crate::bios`]).
    pub bios_dir: PathBuf,
    pub idle_timeout_secs: u64,
    pub video_kbps: u32,
    pub audio_kbps: u32,
    /// How long a worker may take to load its core and game. Generous by default: a disc image
    /// on a remote share takes tens of seconds to open.
    pub start_timeout: Duration,
    /// How long a worker may take to write its saves and exit before it is killed.
    pub stop_timeout: Duration,
    pub rewind_seconds: u32,
    pub rewind_max_mb: u32,
    pub fast_forward: u32,
    pub keyframe_interval: u32,
    /// `--gpu` for workers: a render node, `software` or `off` (see [`crate::gpu`]).
    pub gpu: String,
    /// How workers write their log lines (as the server does); each also gets its session id.
    pub log_format: crate::telemetry::LogFormat,
}

/// The only environment a worker inherits. Cores are third-party code: they get no credentials
/// (the NAS password, API keys) and nothing else of the server's environment.
pub const WORKER_ENV: &[&str] = &[
    "PATH",
    "HOME",
    "TMPDIR",
    "LANG",
    "RUST_LOG",
    "RUST_BACKTRACE",
    // Mesa's own knobs (driver choice, llvmpipe threads): tuning, never secrets.
    "MESA_LOADER_DRIVER_OVERRIDE",
    "GALLIUM_DRIVER",
    "LP_NUM_THREADS",
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
    /// The `session` span: its id, game, system and user, on every line logged about it.
    span: tracing::Span,
}

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
    /// Starts the reaper for `child`, which returns `port` and runs `after_exit` once it exits.
    fn reap(
        mut child: Child,
        id: String,
        port: u16,
        workers: Arc<Workers>,
        after_exit: Option<Box<dyn FnOnce() + Send + Sync>>,
        span: tracing::Span,
        system: String,
    ) -> Self {
        let pid = child.id();
        let started = Instant::now();
        let (kill, mut killed) = oneshot::channel::<()>();
        let (exit_tx, exited) = watch::channel(false);
        tokio::spawn(
            async move {
                let status = tokio::select! {
                    status = child.wait() => status,
                    Ok(()) = &mut killed => {
                        let _ = child.start_kill();
                        child.wait().await
                    }
                };
                let ran = started.elapsed();
                tracing::info!(?status, duration_s = ran.as_secs(), "session ended");
                crate::telemetry::metrics()
                    .session_duration
                    .record(ran.as_secs_f64(), &[opentelemetry::KeyValue::new("system", system)]);
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
                stdin.write_all(line.as_bytes()).await.context("worker stdin closed")?;
                stdin.flush().await?;
            }
            match tokio::time::timeout(limit, rx).await {
                Ok(Ok(Reply::Error { message, .. })) => bail!("{message}"),
                Ok(Ok(reply)) => Ok(reply),
                Ok(Err(_)) => bail!("the session ended"),
                Err(_) => bail!("the session did not answer in time"),
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
            other => bail!("unexpected reply {other:?}"),
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
        let port = self
            .lock()
            .free_ports
            .pop_first()
            .ok_or_else(|| anyhow!("all {} session ports are in use", self.port_count()))?;
        let after_exit = launch.after_exit.take();
        // From here the port goes back to the pool when the worker exits (`Process::reap`).
        let session = self.spawn(&launch, port, after_exit).await?;
        self.lock().sessions.insert(session.id.clone(), session.clone());
        Ok(session)
    }

    fn port_count(&self) -> usize {
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
            tracing::debug!("BIOS folder {}: {e}", bios.display());
        }
        let id = random_id();
        let span = tracing::info_span!(
            "session",
            session.id = %id,
            game.id = launch.game_id,
            system = %launch.system,
            user.id = launch.user_id,
        );
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
            .arg("--audio-kbps")
            .arg(self.config.audio_kbps.to_string())
            .arg("--core")
            .arg(&launch.core)
            .arg("--rom")
            .arg(&launch.rom)
            .arg("--udp")
            .arg(format!("{}:{port}", self.config.bind_ip))
            .arg("--system-dir")
            .arg(&bios)
            .arg("--bios-path")
            .arg(crate::bios::core_path(&launch.system))
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
            .arg("--session-id")
            .arg(&id)
            .arg("--log-format")
            .arg(self.config.log_format.as_arg());
        if let Some((patch, name)) = &launch.patch {
            command.arg("--patch").arg(patch).arg("--patch-name").arg(name);
        }
        for (key, value) in &launch.variables {
            command.arg("--var").arg(format!("{key}={value}"));
        }
        if let Some(ip) = self.config.advertise_ip {
            command.arg("--advertise-ip").arg(ip.to_string());
        }
        let spawned = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .with_context(|| format!("starting {}", self.config.worker_bin.display()));
        let mut child = match spawned {
            Ok(child) => child,
            Err(e) => {
                self.lock().free_ports.insert(port);
                return Err(e);
            }
        };
        let stdin = child.stdin.take().expect("piped");
        let mut lines = BufReader::new(child.stdout.take().expect("piped")).lines();
        let process = Process::reap(
            child,
            id.clone(),
            port,
            self.clone(),
            after_exit,
            span.clone(),
            launch.system.clone(),
        );

        let mut declared = None;
        let ready = tokio::time::timeout(self.config.start_timeout, async {
            while let Some(line) = lines.next_line().await? {
                match serde_json::from_str::<Reply>(&line) {
                    Ok(Reply::Failed { message }) => bail!("{message}"),
                    Ok(Reply::Ready { core, width, height, fps, discs, .. }) => {
                        return Ok((core, width, height, fps, discs));
                    }
                    Ok(Reply::Options { options, categories }) => {
                        declared = Some((options, categories));
                    }
                    Ok(Reply::Progress { phase, read, total }) => {
                        if let Some(id) = &launch.start_id {
                            self.starts.update(id, phase.into(), read, total, Instant::now());
                        }
                    }
                    Ok(other) => tracing::debug!("worker said {other:?} before ready"),
                    Err(_) => tracing::debug!("worker: {line}"),
                }
            }
            bail!("the worker exited while loading the game")
        })
        .await;
        let ready = match ready {
            Ok(Ok(ready)) => ready,
            Ok(Err(e)) => return Err(e),
            Err(_) => {
                process.kill();
                bail!("the worker took too long to load the game");
            }
        };

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
            span: span.clone(),
        });
        tracing::info!(parent: &span, game = %launch.title, udp.port = port, "session started");
        crate::telemetry::metrics()
            .sessions_started
            .add(1, &[opentelemetry::KeyValue::new("system", launch.system.clone())]);

        let reader = session.clone();
        tokio::spawn(async move {
            while let Ok(Some(line)) = lines.next_line().await {
                let Ok(reply) = serde_json::from_str::<Reply>(&line) else { continue };
                match reply {
                    Reply::ClientConnected => reader.connected.store(true, Ordering::Relaxed),
                    Reply::ClientDisconnected => reader.connected.store(false, Ordering::Relaxed),
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

    fn workers(dir: &Path, then: &str, stop_timeout: Duration) -> Arc<Workers> {
        let bin = dir.join("worker.sh");
        std::fs::write(&bin, WORKER.replace("THEN", then)).unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        Workers::new(WorkerConfig {
            worker_bin: bin,
            ports: (40000, 40001),
            bind_ip: "127.0.0.1".parse().unwrap(),
            advertise_ip: None,
            bios_dir: dir.join("bios"),
            idle_timeout_secs: 0,
            video_kbps: 1000,
            audio_kbps: 64,
            start_timeout: Duration::from_secs(10),
            stop_timeout,
            rewind_seconds: 0,
            rewind_max_mb: 0,
            fast_forward: 1,
            keyframe_interval: 60,
            gpu: "off".into(),
            log_format: crate::telemetry::LogFormat::Text,
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
        std::fs::write(&slow, "#!/bin/sh\nsleep 0.3\nexec \"$0.real\" \"$@\"\n").unwrap();
        std::fs::set_permissions(&slow, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::rename(dir.join("worker.sh"), dir.join("slow.sh.real")).unwrap();
        let workers = Workers::new(WorkerConfig { worker_bin: slow, ..workers.config.clone() });
        let start = workers.start(launch(&dir, 1, 1));
        assert!(tokio::time::timeout(Duration::from_millis(50), start).await.is_err());
        assert_eq!(free_ports(&workers), 1, "the abandoned worker still has its port");
        // It finishes loading, finds its stdin closed, and exits on its own.
        settle(&workers, 2).await;
    }
}
