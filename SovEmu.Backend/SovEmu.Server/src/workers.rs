//! Game sessions: one `sovemu-worker session` process each.
//!
//! Every session gets its own UDP port from a fixed range (publish that range from the
//! container) and its own worker, so a crashing core takes down only its session. The server
//! talks to a worker in JSON lines ([`sovemu_protocol::ipc`]), matching replies to requests by
//! id. A user has at most one session: starting a game ends the previous one.

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
use sovemu_protocol::ipc::{Reply, Request};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::oneshot;

use crate::starts::{Phase, Starts};

/// How long a request to a worker may take.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone)]
pub struct WorkerConfig {
    pub worker_bin: PathBuf,
    pub ports: (u16, u16),
    pub bind_ip: IpAddr,
    pub advertise_ip: Option<IpAddr>,
    pub system_dir: PathBuf,
    pub idle_timeout_secs: u64,
    pub video_kbps: u32,
    pub audio_kbps: u32,
    /// How long a worker may take to load its core and game. Generous by default: a disc image
    /// on a remote share takes tens of seconds to open.
    pub start_timeout: Duration,
    pub rewind_seconds: u32,
    pub rewind_max_mb: u32,
    pub fast_forward: u32,
    pub keyframe_interval: u32,
    /// `--gpu` for workers: a render node, `software` or `off` (see [`crate::gpu`]).
    pub gpu: String,
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
    child: tokio::sync::Mutex<Child>,
    pending: Mutex<HashMap<u64, oneshot::Sender<Reply>>>,
    next_id: AtomicU64,
    connected: AtomicBool,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session").field("id", &self.id).field("port", &self.port).finish()
    }
}

impl Session {
    pub fn info(&self) -> SessionInfo {
        SessionInfo { connected: self.connected.load(Ordering::Relaxed), ..self.info.clone() }
    }

    /// Sends a request and waits for its reply.
    pub async fn request(&self, make: impl FnOnce(u64) -> Request) -> Result<Reply> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let request = make(id);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap_or_else(|e| e.into_inner()).insert(id, tx);
        let mut line = serde_json::to_string(&request)?;
        line.push('\n');
        {
            let mut stdin = self.stdin.lock().await;
            stdin.write_all(line.as_bytes()).await.context("worker stdin closed")?;
            stdin.flush().await?;
        }
        match tokio::time::timeout(REQUEST_TIMEOUT, rx).await {
            Ok(Ok(Reply::Error { message, .. })) => bail!("{message}"),
            Ok(Ok(reply)) => Ok(reply),
            Ok(Err(_)) => bail!("the session ended"),
            Err(_) => bail!("the session did not answer in time"),
        }
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
            inner: Mutex::new(Inner { sessions: HashMap::new(), free_ports }),
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

    /// Starts a session, ending the user's current one first.
    pub async fn start(self: &Arc<Self>, mut launch: Launch) -> Result<Arc<Session>> {
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
        match self.spawn(&launch, port, after_exit).await {
            Ok(session) => {
                self.lock().sessions.insert(session.id.clone(), session.clone());
                Ok(session)
            }
            Err(e) => {
                self.lock().free_ports.insert(port);
                Err(e)
            }
        }
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
        // The BIOS folder may sit on a read-only library that has none: most cores need no BIOS,
        // so a missing one is not a reason to refuse the game.
        if let Err(e) = std::fs::create_dir_all(&self.config.system_dir) {
            tracing::debug!("system dir {}: {e}", self.config.system_dir.display());
        }
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
            .arg(&self.config.system_dir)
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
            .arg(&self.config.gpu);
        for (key, value) in &launch.variables {
            command.arg("--var").arg(format!("{key}={value}"));
        }
        if let Some(ip) = self.config.advertise_ip {
            command.arg("--advertise-ip").arg(ip.to_string());
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .with_context(|| format!("starting {}", self.config.worker_bin.display()))?;
        let stdin = child.stdin.take().expect("piped");
        let mut lines = BufReader::new(child.stdout.take().expect("piped")).lines();

        let ready = tokio::time::timeout(self.config.start_timeout, async {
            while let Some(line) = lines.next_line().await? {
                match serde_json::from_str::<Reply>(&line) {
                    Ok(Reply::Ready { core, width, height, fps, .. }) => {
                        return Ok((core, width, height, fps));
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
        .await
        .map_err(|_| anyhow!("the worker took too long to load the game"))??;

        let (core, width, height, fps) = ready;
        let id = random_id();
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
            },
            stdin: tokio::sync::Mutex::new(stdin),
            child: tokio::sync::Mutex::new(child),
            pending: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            connected: AtomicBool::new(false),
        });
        tracing::info!("session {id}: {} ({}) on UDP {port}", launch.title, launch.system);

        let workers = self.clone();
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
                    Reply::Ready { .. } | Reply::Progress { .. } => {}
                }
            }
            // The worker exited: idle timeout, shutdown, or a crash.
            let status = reader.child.lock().await.wait().await;
            tracing::info!("session {} ended ({status:?})", reader.id);
            workers.forget(&reader.id, reader.port);
            if let Some(after) = after_exit {
                after();
            }
        });
        Ok(session)
    }

    fn forget(&self, id: &str, port: u16) {
        let mut inner = self.lock();
        if inner.sessions.remove(id).is_some() || !inner.free_ports.contains(&port) {
            inner.free_ports.insert(port);
        }
    }

    /// Ends a session: asks the worker to flush and exit, then kills it if it does not.
    pub async fn stop(&self, id: &str) {
        let Some(session) = self.lock().sessions.remove(id) else { return };
        let asked = tokio::time::timeout(
            Duration::from_secs(3),
            session.request(|id| Request::Shutdown { id }),
        )
        .await;
        if !matches!(asked, Ok(Ok(_))) {
            tracing::warn!("session {id} did not shut down cleanly; killing it");
        }
        let mut child = session.child.lock().await;
        if tokio::time::timeout(Duration::from_secs(2), child.wait()).await.is_err() {
            let _ = child.kill().await;
        }
        drop(child);
        self.lock().free_ports.insert(session.port);
    }

    /// Ends every session (server shutdown).
    pub async fn stop_all(&self) {
        let ids: Vec<String> = self.lock().sessions.keys().cloned().collect();
        for id in ids {
            self.stop(&id).await;
        }
    }
}

fn random_id() -> String {
    let mut bytes = [0u8; 12];
    getrandom::fill(&mut bytes).expect("the OS random source works");
    hex::encode(bytes)
}
