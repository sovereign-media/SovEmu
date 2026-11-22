//! Background jobs: what the server does on its own, on intervals set in the environment, and
//! what an admin starts by hand.
//!
//! - [`Job::Scan`]: rescans the library (at startup, then `SOVEMU_SCAN_INTERVAL_MINUTES`).
//! - [`Job::MirrorSync`]: in `mirror` mode, copies the source into the local library, then
//!   rescans it (at startup, then `SOVEMU_MIRROR_INTERVAL_MINUTES`).
//! - [`Job::Metadata`]: looks up game information for games without it (after each scan with
//!   `SOVEMU_METADATA_AUTO`).
//! - [`Job::Maintenance`] (`SOVEMU_MAINTENANCE_INTERVAL_MINUTES`): expired sign-ins and old job
//!   history deleted, the query planner's statistics refreshed (`PRAGMA optimize`) and the
//!   write-ahead log folded back into the database, so none of them grows without end.
//!
//! Work too frequent to be a job (the resource readings, every few seconds) is a sampler
//! ([`Jobs::sampler`]): stopped on shutdown and kept alive through panics like a job, but with
//! no history.
//!
//! Every run goes through [`Jobs`], which gives each job the same guarantees:
//! - one run of a job at a time: starting one that is running is [`JobError::Running`];
//! - jobs that would get in each other's way wait for each other ([`Job::lane`]: a scan and a
//!   mirror sync both rewrite the games table);
//! - every run is a row in `job_runs` (what started it, how it ended, how long it took, what it
//!   did) and runs in a `job` span, so its log lines carry the job's name;
//! - a run that panics is a failed run, and its schedule carries on;
//! - on shutdown ([`Jobs::shutdown`]) every run is cancelled and recorded as cancelled, and no
//!   new one starts.
//!
//! Schedules come from the environment on every start; none is stored. A scheduled job runs
//! its interval after the previous scheduled run ended (startup runs its own first scan), so a
//! slow run is never followed by a burst of catch-up runs.

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use opentelemetry::KeyValue;
use sqlx::SqlitePool;
use tokio::task::AbortHandle;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use tracing::Instrument;

use crate::error::ApiError;
use crate::{AppState, LibraryMode, admin, telemetry};

/// The jobs there are. Their names are stored in `job_runs`: never rename one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Job {
    Scan,
    MirrorSync,
    Metadata,
    Maintenance,
}

/// Jobs that must not run at the same time share a lane, and wait for each other in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lane {
    /// Rewrites the games table: a scan, and a mirror sync (which ends with one).
    Library,
}

impl Job {
    pub(crate) const ALL: [Job; 4] = [Job::Scan, Job::MirrorSync, Job::Metadata, Job::Maintenance];

    pub(crate) fn name(self) -> &'static str {
        match self {
            Job::Scan => "scan",
            Job::MirrorSync => "mirror_sync",
            Job::Metadata => "metadata",
            Job::Maintenance => "maintenance",
        }
    }

    fn from_name(name: &str) -> Option<Job> {
        Job::ALL.into_iter().find(|j| j.name() == name)
    }

    pub(crate) fn description(self) -> &'static str {
        match self {
            Job::Scan => {
                "Rescans the library: new games added, changed ones updated, missing ones removed."
            }
            Job::MirrorSync => {
                "Copies the library's source into the local mirror, then rescans it."
            }
            Job::Metadata => "Looks up game information and box art for games that have none yet.",
            Job::Maintenance => {
                "Deletes expired sign-ins and old job history, and keeps the database compact and its query plans current."
            }
        }
    }

    fn lane(self) -> Option<Lane> {
        match self {
            Job::Scan | Job::MirrorSync => Some(Lane::Library),
            Job::Metadata | Job::Maintenance => None,
        }
    }

    /// Whether this server has the job at all (a mirror sync only in `mirror` mode).
    pub(crate) fn available(self, state: &AppState) -> bool {
        self != Job::MirrorSync || state.config.library_mode == LibraryMode::Mirror
    }

    /// How often it runs on its own, if it does.
    fn every(self, state: &AppState) -> Option<Duration> {
        let minutes = match self {
            Job::Scan => state.config.scan_interval_minutes,
            Job::MirrorSync => state.config.mirror_interval_minutes,
            // Follows each scan rather than keeping a clock of its own.
            Job::Metadata => 0,
            Job::Maintenance => state.config.maintenance_interval_minutes,
        };
        (minutes > 0 && self.available(state)).then(|| Duration::from_secs(minutes * 60))
    }
}

/// What started a run. Stored in `job_runs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Trigger {
    Startup,
    Schedule,
    Admin,
}

impl Trigger {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Trigger::Startup => "startup",
            Trigger::Schedule => "schedule",
            Trigger::Admin => "admin",
        }
    }

    fn from_str(s: &str) -> Trigger {
        match s {
            "startup" => Trigger::Startup,
            "schedule" => Trigger::Schedule,
            _ => Trigger::Admin,
        }
    }
}

/// How a run ended. Stored in `job_runs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    Running,
    Succeeded,
    Failed,
    /// Stopped by the server shutting down.
    Cancelled,
    /// Found still running at startup: the server stopped without finishing it.
    Interrupted,
}

impl Outcome {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Outcome::Running => "running",
            Outcome::Succeeded => "succeeded",
            Outcome::Failed => "failed",
            Outcome::Cancelled => "cancelled",
            Outcome::Interrupted => "interrupted",
        }
    }

    fn from_str(s: &str) -> Outcome {
        match s {
            "running" => Outcome::Running,
            "succeeded" => Outcome::Succeeded,
            "cancelled" => Outcome::Cancelled,
            "interrupted" => Outcome::Interrupted,
            _ => Outcome::Failed,
        }
    }
}

/// Why a run did not succeed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum JobError {
    /// The job is already running; this run did not start.
    Running,
    /// The job is not available on this server (a mirror sync outside `mirror` mode).
    Unavailable,
    /// The server is shutting down: the run was cancelled, or never started.
    Cancelled,
    /// The run failed (or panicked): why.
    Failed(String),
}

impl From<JobError> for ApiError {
    fn from(e: JobError) -> Self {
        match e {
            JobError::Running => ApiError::conflict("that job is already running"),
            JobError::Unavailable => ApiError::bad_request("that job does not run on this server"),
            JobError::Cancelled => ApiError::new(
                axum::http::StatusCode::SERVICE_UNAVAILABLE,
                "the server is shutting down",
            ),
            JobError::Failed(message) => ApiError::internal(message),
        }
    }
}

/// What a run did, for its `job_runs` row.
pub(crate) trait Summary {
    fn summary(&self) -> Option<String>;
}

impl Summary for () {
    fn summary(&self) -> Option<String> {
        None
    }
}

impl Summary for String {
    fn summary(&self) -> Option<String> {
        Some(self.clone())
    }
}

/// A job's state now: whether it is running, and when it next runs on its own.
#[derive(Debug, Clone)]
pub(crate) struct JobInfo {
    pub(crate) job: Job,
    pub(crate) every: Option<Duration>,
    pub(crate) next_at: Option<SystemTime>,
    pub(crate) running: Option<(Trigger, SystemTime)>,
}

/// One run, as `job_runs` has it. Times are Unix seconds.
#[derive(Debug, Clone)]
pub(crate) struct JobRun {
    pub(crate) id: i64,
    pub(crate) job: Job,
    pub(crate) trigger: Trigger,
    pub(crate) outcome: Outcome,
    pub(crate) started_at: i64,
    pub(crate) finished_at: Option<i64>,
    pub(crate) duration_ms: Option<i64>,
    pub(crate) message: Option<String>,
}

#[derive(Debug, Default)]
struct Slot {
    running: Option<(Trigger, SystemTime)>,
    next_at: Option<SystemTime>,
}

/// Runs the jobs (see the module's notes). Cheap to clone: shared.
#[derive(Clone)]
pub(crate) struct Jobs(Arc<Inner>);

struct Inner {
    db: SqlitePool,
    cancel: CancellationToken,
    tasks: TaskTracker,
    library: tokio::sync::Mutex<()>,
    slots: Mutex<HashMap<Job, Slot>>,
}

impl std::fmt::Debug for Jobs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Jobs").finish_non_exhaustive()
    }
}

/// Clears a job's `running` mark however its run ends.
struct Release(Arc<Inner>, Job);

impl Drop for Release {
    fn drop(&mut self) {
        if let Some(slot) = self.0.slots.lock().unwrap_or_else(|e| e.into_inner()).get_mut(&self.1)
        {
            slot.running = None;
        }
    }
}

/// Aborts a run's task if whatever waits on it is dropped (the server shutting down).
struct AbortOnDrop(AbortHandle);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

impl Jobs {
    /// The job runner, over the database its history goes in. A run left `running` by a
    /// server that stopped without finishing it is marked `interrupted`.
    pub(crate) async fn new(db: SqlitePool) -> sqlx::Result<Self> {
        sqlx::query(
            "UPDATE job_runs SET outcome = 'interrupted', finished_at = datetime('now')
             WHERE outcome = 'running'",
        )
        .execute(&db)
        .await?;
        Ok(Self(Arc::new(Inner {
            db,
            cancel: CancellationToken::new(),
            tasks: TaskTracker::new(),
            library: tokio::sync::Mutex::new(()),
            slots: Mutex::default(),
        })))
    }

    /// Starts a run of `job` in the background and returns at once; the handle gives its
    /// result. [`JobError::Running`] (and nothing started) if it is running already.
    pub(crate) fn start<T, F>(
        &self,
        job: Job,
        trigger: Trigger,
        work: F,
    ) -> Result<tokio::task::JoinHandle<Result<T, JobError>>, JobError>
    where
        T: Summary + Send + 'static,
        F: Future<Output = anyhow::Result<T>> + Send + 'static,
    {
        if self.0.cancel.is_cancelled() {
            return Err(JobError::Cancelled);
        }
        {
            let mut slots = self.0.slots.lock().unwrap_or_else(|e| e.into_inner());
            let slot = slots.entry(job).or_default();
            if slot.running.is_some() {
                return Err(JobError::Running);
            }
            slot.running = Some((trigger, SystemTime::now()));
        }
        let release = Release(self.0.clone(), job);
        let inner = self.0.clone();
        let span = tracing::info_span!("job", job = job.name(), trigger = trigger.as_str());
        Ok(self.0.tasks.spawn(
            async move {
                let _release = release;
                let id = record_start(&inner.db, job, trigger).await;
                let started = Instant::now();
                let result = tokio::select! {
                    r = execute(&inner, job, work) => r,
                    () = inner.cancel.cancelled() => Err(JobError::Cancelled),
                };
                let elapsed = started.elapsed();
                let (outcome, message) = match &result {
                    Ok(v) => (Outcome::Succeeded, v.summary()),
                    Err(JobError::Failed(m)) => (Outcome::Failed, Some(m.clone())),
                    Err(_) => (Outcome::Cancelled, None),
                };
                match outcome {
                    Outcome::Failed => tracing::warn!(
                        elapsed_s = elapsed.as_secs_f64(),
                        error = message.as_deref().unwrap_or_default(),
                        "job failed"
                    ),
                    _ => tracing::info!(
                        outcome = outcome.as_str(),
                        elapsed_s = elapsed.as_secs_f64(),
                        summary = message.as_deref().unwrap_or_default(),
                        "job finished"
                    ),
                }
                telemetry::metrics().job_duration.record(
                    elapsed.as_secs_f64(),
                    &[KeyValue::new("job", job.name()), KeyValue::new("outcome", outcome.as_str())],
                );
                if let Some(id) = id {
                    record_end(&inner.db, id, outcome, elapsed, message.as_deref()).await;
                }
                result
            }
            .instrument(span),
        ))
    }

    /// Runs `job` and waits for it. The run carries on (and is recorded) if the caller stops
    /// waiting: an admin's request that times out does not cut a scan short.
    pub(crate) async fn run<T, F>(&self, job: Job, trigger: Trigger, work: F) -> Result<T, JobError>
    where
        T: Summary + Send + 'static,
        F: Future<Output = anyhow::Result<T>> + Send + 'static,
    {
        match self.start(job, trigger, work)?.await {
            Ok(result) => result,
            Err(_) => Err(JobError::Cancelled),
        }
    }

    /// Runs `job` every `every`, measured from the end of the previous scheduled run, until the
    /// server shuts down. A turn that finds the job already running (an admin started it) is
    /// skipped.
    fn schedule<T, F, Fut>(&self, job: Job, every: Duration, make: F)
    where
        T: Summary + Send + 'static,
        F: Fn() -> Fut + Send + 'static,
        Fut: Future<Output = anyhow::Result<T>> + Send + 'static,
    {
        let jobs = self.clone();
        self.0.tasks.spawn(async move {
            loop {
                jobs.slot(job, |s| s.next_at = Some(SystemTime::now() + every));
                tokio::select! {
                    () = tokio::time::sleep(every) => {}
                    () = jobs.0.cancel.cancelled() => return,
                }
                jobs.slot(job, |s| s.next_at = None);
                match jobs.run(job, Trigger::Schedule, make()).await {
                    Err(JobError::Running) => {
                        tracing::debug!(job = job.name(), "already running; skipping its turn");
                    }
                    Err(JobError::Cancelled) => return,
                    _ => {}
                }
            }
        });
    }

    pub(crate) fn is_running(&self, job: Job) -> bool {
        let slots = self.0.slots.lock().unwrap_or_else(|e| e.into_inner());
        slots.get(&job).is_some_and(|s| s.running.is_some())
    }

    /// Runs `tick` every `every` until the server shuts down: a sampler, for work too frequent
    /// to be a job (a reading every few seconds). It keeps no history; each tick runs in a task
    /// of its own, so a panic is a failed tick, and a failure is logged once, until a tick
    /// succeeds again. Ticks never overlap, and a late one is not made up.
    pub(crate) fn sampler<F, Fut>(&self, name: &'static str, every: Duration, tick: F)
    where
        F: Fn() -> Fut + Send + 'static,
        Fut: Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        let cancel = self.0.cancel.clone();
        let span = tracing::info_span!("sampler", sampler = name);
        self.0.tasks.spawn(
            async move {
                let mut interval = tokio::time::interval(every);
                interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                let mut failing = false;
                loop {
                    tokio::select! {
                        _ = interval.tick() => {}
                        () = cancel.cancelled() => return,
                    }
                    let task = tokio::spawn(tick().in_current_span());
                    let _abort = AbortOnDrop(task.abort_handle());
                    let result = tokio::select! {
                        r = task => r,
                        () = cancel.cancelled() => return,
                    };
                    let error = match result {
                        Ok(Ok(())) => None,
                        Ok(Err(e)) => Some(format!("{e:#}")),
                        Err(e) if e.is_panic() => Some("panicked".to_owned()),
                        Err(_) => return,
                    };
                    match error {
                        Some(e) if !failing => {
                            tracing::warn!(error = %e, "sampling failed; logged once until it recovers");
                            failing = true;
                        }
                        None if failing => {
                            tracing::info!("sampling recovered");
                            failing = false;
                        }
                        _ => {}
                    }
                }
            }
            .instrument(span),
        );
    }

    fn slot(&self, job: Job, f: impl FnOnce(&mut Slot)) {
        f(self.0.slots.lock().unwrap_or_else(|e| e.into_inner()).entry(job).or_default());
    }

    /// Every job this server has, and its state now.
    pub(crate) fn info(&self, state: &AppState) -> Vec<JobInfo> {
        let slots = self.0.slots.lock().unwrap_or_else(|e| e.into_inner());
        Job::ALL
            .into_iter()
            .filter(|j| j.available(state))
            .map(|job| {
                let slot = slots.get(&job);
                JobInfo {
                    job,
                    every: job.every(state),
                    next_at: slot.and_then(|s| s.next_at),
                    running: slot.and_then(|s| s.running),
                }
            })
            .collect()
    }

    /// The latest runs, newest first: of one job, or of all.
    pub(crate) async fn runs(&self, job: Option<Job>, limit: u32) -> sqlx::Result<Vec<JobRun>> {
        let rows: Vec<RunRow> = sqlx::query_as(
            "SELECT id, job, trigger, outcome, unixepoch(started_at), unixepoch(finished_at),
                    duration_ms, message
             FROM job_runs WHERE ?1 IS NULL OR job = ?1 ORDER BY id DESC LIMIT ?2",
        )
        .bind(job.map(Job::name))
        .bind(limit)
        .fetch_all(&self.0.db)
        .await?;
        Ok(rows.into_iter().filter_map(RunRow::into_run).collect())
    }

    /// Each job's latest run.
    pub(crate) async fn last_runs(&self) -> sqlx::Result<HashMap<Job, JobRun>> {
        let rows: Vec<RunRow> = sqlx::query_as(
            "SELECT id, job, trigger, outcome, unixepoch(started_at), unixepoch(finished_at),
                    duration_ms, message
             FROM job_runs WHERE id IN (SELECT MAX(id) FROM job_runs GROUP BY job)",
        )
        .fetch_all(&self.0.db)
        .await?;
        Ok(rows.into_iter().filter_map(RunRow::into_run).map(|r| (r.job, r)).collect())
    }

    /// Cancels every run and schedule, and waits up to `grace` for their history to be written.
    /// Nothing starts afterwards.
    pub(crate) async fn shutdown(&self, grace: Duration) {
        self.0.cancel.cancel();
        self.0.tasks.close();
        if tokio::time::timeout(grace, self.0.tasks.wait()).await.is_err() {
            tracing::warn!("background jobs did not stop in time");
        }
    }
}

/// Runs the work in a task of its own (so a panic is the run's failure, not the server's), in
/// its lane if it has one.
async fn execute<T, F>(inner: &Inner, job: Job, work: F) -> Result<T, JobError>
where
    T: Send + 'static,
    F: Future<Output = anyhow::Result<T>> + Send + 'static,
{
    let _lane = match job.lane() {
        Some(Lane::Library) => Some(inner.library.lock().await),
        None => None,
    };
    let task = tokio::spawn(work.in_current_span());
    let _abort = AbortOnDrop(task.abort_handle());
    match task.await {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(e)) => Err(JobError::Failed(format!("{e:#}"))),
        Err(e) if e.is_panic() => {
            let panic = e.into_panic();
            let what = panic
                .downcast_ref::<&str>()
                .map(|s| (*s).to_owned())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown panic".into());
            Err(JobError::Failed(format!("panicked: {what}")))
        }
        Err(_) => Err(JobError::Cancelled),
    }
}

type RunRow = (i64, String, String, String, i64, Option<i64>, Option<i64>, Option<String>);

trait IntoRun {
    fn into_run(self) -> Option<JobRun>;
}

impl IntoRun for RunRow {
    fn into_run(self) -> Option<JobRun> {
        let (id, job, trigger, outcome, started_at, finished_at, duration_ms, message) = self;
        Some(JobRun {
            id,
            // A job since removed: its old rows are left for maintenance to prune.
            job: Job::from_name(&job)?,
            trigger: Trigger::from_str(&trigger),
            outcome: Outcome::from_str(&outcome),
            started_at,
            finished_at,
            duration_ms,
            message,
        })
    }
}

/// The run's row; none if it could not be written (the run goes ahead regardless).
async fn record_start(db: &SqlitePool, job: Job, trigger: Trigger) -> Option<i64> {
    let id = sqlx::query_scalar("INSERT INTO job_runs (job, trigger) VALUES (?, ?) RETURNING id")
        .bind(job.name())
        .bind(trigger.as_str())
        .fetch_one(db)
        .await;
    id.inspect_err(|e| tracing::warn!(error = %e, "recording a job's start")).ok()
}

async fn record_end(
    db: &SqlitePool,
    id: i64,
    outcome: Outcome,
    elapsed: Duration,
    message: Option<&str>,
) {
    let r = sqlx::query(
        "UPDATE job_runs SET outcome = ?, finished_at = datetime('now'), duration_ms = ?,
           message = ?
         WHERE id = ?",
    )
    .bind(outcome.as_str())
    .bind(i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX))
    .bind(message)
    .bind(id)
    .execute(db)
    .await;
    if let Err(e) = r {
        tracing::warn!(error = %e, "recording a job's end");
    }
}

/// Startup's runs, then every schedule: a scan of what is already in the library (in `mirror`
/// mode a sync, and the scan that ends it, follow), game information for what it found, and
/// each job's interval from then on.
pub(crate) fn spawn(state: &AppState) {
    let jobs = &state.jobs;
    if state.resources.sample_seconds > 0 {
        let s = state.clone();
        let every = Duration::from_secs(u64::from(s.resources.sample_seconds));
        jobs.sampler("resources", every, move || {
            let s = s.clone();
            async move { s.resources.tick(&s.db).await }
        });
    }
    let first = state.clone();
    jobs.0.tasks.spawn(async move {
        let _ = first.jobs.run(Job::Scan, Trigger::Startup, admin::scan(first.clone())).await;
        if first.config.library_mode == LibraryMode::Mirror {
            let _ = first
                .jobs
                .run(Job::MirrorSync, Trigger::Startup, admin::sync_then_scan(first.clone()))
                .await;
        }
        admin::after_scan(&first, Trigger::Startup).await;
    });
    if let Some(every) = Job::Scan.every(state) {
        let state = state.clone();
        jobs.schedule(Job::Scan, every, move || {
            let state = state.clone();
            async move {
                let report = admin::scan(state.clone()).await?;
                admin::after_scan_in_background(&state);
                Ok(report)
            }
        });
    }
    if let Some(every) = Job::MirrorSync.every(state) {
        let state = state.clone();
        jobs.schedule(Job::MirrorSync, every, move || {
            let state = state.clone();
            async move {
                let summary = admin::sync_then_scan(state.clone()).await?;
                admin::after_scan_in_background(&state);
                Ok(summary)
            }
        });
    }
    if let Some(every) = Job::Maintenance.every(state) {
        let state = state.clone();
        jobs.schedule(Job::Maintenance, every, move || maintain(state.clone()));
    }
}

/// Starts `job` as an admin asked: in the background, with its usual work.
pub(crate) fn start_by_admin(state: &AppState, job: Job) -> Result<(), JobError> {
    if !job.available(state) {
        return Err(JobError::Unavailable);
    }
    let jobs = &state.jobs;
    let s = state.clone();
    match job {
        Job::Scan => jobs
            .start(Job::Scan, Trigger::Admin, async move {
                let report = admin::scan(s.clone()).await?;
                admin::after_scan_in_background(&s);
                Ok(report)
            })
            .map(drop),
        Job::MirrorSync => jobs
            .start(Job::MirrorSync, Trigger::Admin, async move {
                let summary = admin::sync_then_scan(s.clone()).await?;
                admin::after_scan_in_background(&s);
                Ok(summary)
            })
            .map(drop),
        Job::Metadata => {
            jobs.start(Job::Metadata, Trigger::Admin, admin::lookup(s, None, false)).map(drop)
        }
        Job::Maintenance => jobs.start(Job::Maintenance, Trigger::Admin, maintain(s)).map(drop),
    }
}

/// One round of upkeep (see the module's notes).
async fn maintain(state: AppState) -> anyhow::Result<String> {
    let samples = crate::resources::prune(&state.db, state.config.resource_history_days).await?;
    let (expired, pruned) =
        maintain_db(&state.db, &state.identity, state.config.job_history_days).await?;
    Ok(format!(
        "{expired} expired sign-ins deleted, {pruned} old job runs and {samples} old resource samples pruned"
    ))
}

/// Expired sign-ins, codes and tokens deleted (`sovemu_identity`), job runs older than
/// `history_days` deleted (0: kept), the query planner's statistics refreshed, the write-ahead
/// log folded back. How many records expired, and how many runs were pruned.
pub(crate) async fn maintain_db(
    db: &SqlitePool,
    identity: &sovemu_identity::Identity,
    history_days: u32,
) -> anyhow::Result<(u64, u64)> {
    let expired = identity.maintain().await?;
    let pruned = if history_days > 0 {
        sqlx::query("DELETE FROM job_runs WHERE started_at < datetime('now', ?)")
            .bind(format!("-{history_days} days"))
            .execute(db)
            .await?
            .rows_affected()
    } else {
        0
    };
    sqlx::query("PRAGMA optimize").execute(db).await?;
    // PASSIVE: never waits on a reader or writer; what it cannot fold back now waits for the
    // next round.
    sqlx::query("PRAGMA wal_checkpoint(PASSIVE)").execute(db).await?;
    Ok((expired, pruned))
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn db(name: &str) -> (SqlitePool, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("sovemu-jobs-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        (crate::db::open(&dir.join("jobs.db")).await.unwrap(), dir)
    }

    async fn outcomes(db: &SqlitePool) -> Vec<(String, String, Option<String>)> {
        sqlx::query_as("SELECT job, outcome, message FROM job_runs ORDER BY id")
            .fetch_all(db)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn a_run_is_recorded_and_a_second_one_is_refused_while_it_runs() {
        let (db, dir) = db("once").await;
        let jobs = Jobs::new(db.clone()).await.unwrap();
        let (go, wait) = tokio::sync::oneshot::channel::<()>();
        let first = jobs
            .start(Job::Scan, Trigger::Admin, async move {
                wait.await.ok();
                Ok("did it".to_owned())
            })
            .unwrap();
        assert_eq!(
            jobs.start(Job::Scan, Trigger::Schedule, async { Ok(()) }).unwrap_err(),
            JobError::Running
        );
        // Another job is not held up by it.
        jobs.run(Job::Maintenance, Trigger::Admin, async { Ok(()) }).await.unwrap();
        go.send(()).unwrap();
        assert_eq!(first.await.unwrap().unwrap(), "did it");
        // Each run writes its row from its own task, so the two rows may land in either order.
        let mut rows = outcomes(&db).await;
        rows.sort();
        assert_eq!(
            rows,
            [
                ("maintenance".into(), "succeeded".into(), None),
                ("scan".into(), "succeeded".into(), Some("did it".into())),
            ]
        );
        // Not running any more, so it can start again.
        jobs.run(Job::Scan, Trigger::Admin, async { Ok(()) }).await.unwrap();
        let last = jobs.last_runs().await.unwrap();
        assert_eq!(last[&Job::Scan].trigger, Trigger::Admin);
        assert_eq!(jobs.runs(Some(Job::Scan), 10).await.unwrap().len(), 2);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn failures_and_panics_are_failed_runs() {
        let (db, dir) = db("fail").await;
        let jobs = Jobs::new(db.clone()).await.unwrap();
        let e = jobs
            .run(Job::Scan, Trigger::Admin, async {
                anyhow::bail!("no library") as anyhow::Result<()>
            })
            .await
            .unwrap_err();
        assert_eq!(e, JobError::Failed("no library".into()));
        let e = jobs
            .run(Job::Scan, Trigger::Admin, async {
                if true {
                    panic!("boom");
                }
                Ok(())
            })
            .await
            .unwrap_err();
        assert_eq!(e, JobError::Failed("panicked: boom".into()));
        let rows = outcomes(&db).await;
        assert_eq!(rows[0].1, "failed");
        assert_eq!(rows[1], ("scan".into(), "failed".into(), Some("panicked: boom".into())));
        // A panic does not leave the job marked as running.
        jobs.run(Job::Scan, Trigger::Admin, async { Ok(()) }).await.unwrap();
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn jobs_in_a_lane_wait_for_each_other() {
        let (db, dir) = db("lane").await;
        let jobs = Jobs::new(db).await.unwrap();
        let order = Arc::new(Mutex::new(Vec::new()));
        let (go, wait) = tokio::sync::oneshot::channel::<()>();
        let (inside, holding) = tokio::sync::oneshot::channel::<()>();
        let o = order.clone();
        let sync = jobs
            .start(Job::MirrorSync, Trigger::Admin, async move {
                inside.send(()).ok();
                wait.await.ok();
                o.lock().unwrap().push("sync");
                Ok(())
            })
            .unwrap();
        // The sync's work runs only once it holds the lane.
        holding.await.unwrap();
        let o = order.clone();
        let scan = jobs
            .start(Job::Scan, Trigger::Admin, async move {
                o.lock().unwrap().push("scan");
                Ok(())
            })
            .unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(order.lock().unwrap().is_empty(), "the scan waits for the sync");
        go.send(()).unwrap();
        sync.await.unwrap().unwrap();
        scan.await.unwrap().unwrap();
        assert_eq!(*order.lock().unwrap(), ["sync", "scan"]);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn shutdown_cancels_runs_and_refuses_new_ones() {
        let (db, dir) = db("shutdown").await;
        let jobs = Jobs::new(db.clone()).await.unwrap();
        let run = jobs
            .start(Job::Scan, Trigger::Admin, async {
                std::future::pending::<()>().await;
                Ok(())
            })
            .unwrap();
        tokio::task::yield_now().await;
        jobs.shutdown(Duration::from_secs(5)).await;
        assert_eq!(run.await.unwrap().unwrap_err(), JobError::Cancelled);
        assert_eq!(outcomes(&db).await[0].1, "cancelled");
        assert_eq!(
            jobs.start(Job::Scan, Trigger::Admin, async { Ok(()) }).unwrap_err(),
            JobError::Cancelled
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn a_run_left_running_is_interrupted_at_the_next_start() {
        let (db, dir) = db("interrupted").await;
        sqlx::query("INSERT INTO job_runs (job, trigger) VALUES ('scan', 'startup')")
            .execute(&db)
            .await
            .unwrap();
        let jobs = Jobs::new(db.clone()).await.unwrap();
        let last = jobs.last_runs().await.unwrap();
        assert_eq!(last[&Job::Scan].outcome, Outcome::Interrupted);
        assert!(last[&Job::Scan].finished_at.is_some());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn maintenance_deletes_only_expired_sign_ins_and_old_runs() {
        let (db, dir) = db("maintain").await;
        let config = sovemu_identity::Config {
            public_urls: vec!["http://localhost:8080".into()],
            token_secret: "test".into(),
            access_token_minutes: 60,
            session_days: 30,
            secure_cookies: false,
            password_sign_in: true,
            max_failures: 5,
            max_failures_per_ip: 20,
            lockout: std::time::Duration::from_secs(300),
            refresh_retry: std::time::Duration::ZERO,
        };
        let identity = sovemu_identity::Identity::new(db.clone(), config).await.unwrap();
        sqlx::query("INSERT INTO users (username, password_hash) VALUES ('a', 'x')")
            .execute(&db)
            .await
            .unwrap();
        for (token, expires) in [("old", "-1 day"), ("live", "+1 day")] {
            sqlx::query(
                "INSERT INTO auth_sessions (token_hash, user_id, expires_at)
                 VALUES (?, 1, datetime('now', ?))",
            )
            .bind(token)
            .bind(expires)
            .execute(&db)
            .await
            .unwrap();
        }
        for started in ["-40 days", "-1 day"] {
            sqlx::query(
                "INSERT INTO job_runs (job, trigger, outcome, started_at)
                 VALUES ('scan', 'schedule', 'succeeded', datetime('now', ?))",
            )
            .bind(started)
            .execute(&db)
            .await
            .unwrap();
        }
        assert_eq!(maintain_db(&db, &identity, 30).await.unwrap(), (1, 1));
        let left: Vec<String> = sqlx::query_scalar("SELECT token_hash FROM auth_sessions")
            .fetch_all(&db)
            .await
            .unwrap();
        assert_eq!(left, ["live"]);
        assert_eq!(maintain_db(&db, &identity, 30).await.unwrap(), (0, 0), "nothing left");
        std::fs::remove_dir_all(&dir).ok();
    }
}
