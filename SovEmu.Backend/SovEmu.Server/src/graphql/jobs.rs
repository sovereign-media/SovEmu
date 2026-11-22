//! Background jobs: their schedule, history, and running one now.

use super::*;
use crate::activity::{Entry, Kind};

/// A background job (`jobs.rs`).
#[derive(Enum, Clone, Copy, PartialEq, Eq)]
pub(super) enum JobName {
    /// Rescans the library.
    Scan,
    /// `MIRROR` mode: copies the source into the local mirror, then rescans it.
    MirrorSync,
    /// Looks up game information for games without it.
    Metadata,
    /// Database upkeep: expired sign-ins and old job history deleted.
    Maintenance,
    /// A backup of the database, the signing keys and the saves.
    Backup,
}

impl From<crate::jobs::Job> for JobName {
    fn from(j: crate::jobs::Job) -> Self {
        use crate::jobs::Job;
        match j {
            Job::Scan => Self::Scan,
            Job::MirrorSync => Self::MirrorSync,
            Job::Metadata => Self::Metadata,
            Job::Maintenance => Self::Maintenance,
            Job::Backup => Self::Backup,
        }
    }
}

impl From<JobName> for crate::jobs::Job {
    fn from(j: JobName) -> Self {
        match j {
            JobName::Scan => Self::Scan,
            JobName::MirrorSync => Self::MirrorSync,
            JobName::Metadata => Self::Metadata,
            JobName::Maintenance => Self::Maintenance,
            JobName::Backup => Self::Backup,
        }
    }
}

/// What started a job's run.
#[derive(Enum, Clone, Copy, PartialEq, Eq)]
pub(super) enum JobTrigger {
    Startup,
    /// Its interval (`SOVEMU_*_INTERVAL_MINUTES`).
    Schedule,
    /// An admin.
    Admin,
}

impl From<crate::jobs::Trigger> for JobTrigger {
    fn from(t: crate::jobs::Trigger) -> Self {
        use crate::jobs::Trigger;
        match t {
            Trigger::Startup => Self::Startup,
            Trigger::Schedule => Self::Schedule,
            Trigger::Admin => Self::Admin,
        }
    }
}

/// How a job's run ended.
#[derive(Enum, Clone, Copy, PartialEq, Eq)]
pub(super) enum JobOutcome {
    Running,
    Succeeded,
    Failed,
    /// Stopped by the server shutting down.
    Cancelled,
    /// The server stopped without finishing it.
    Interrupted,
}

impl From<crate::jobs::Outcome> for JobOutcome {
    fn from(o: crate::jobs::Outcome) -> Self {
        use crate::jobs::Outcome;
        match o {
            Outcome::Running => Self::Running,
            Outcome::Succeeded => Self::Succeeded,
            Outcome::Failed => Self::Failed,
            Outcome::Cancelled => Self::Cancelled,
            Outcome::Interrupted => Self::Interrupted,
        }
    }
}

/// One run of a background job.
#[derive(SimpleObject)]
pub(super) struct JobRun {
    pub(super) id: ID,
    pub(super) job: JobName,
    pub(super) trigger: JobTrigger,
    pub(super) outcome: JobOutcome,
    /// Unix seconds.
    pub(super) started_at: f64,
    /// Unix seconds; none while it runs.
    pub(super) finished_at: Option<f64>,
    pub(super) seconds: Option<f64>,
    /// What it did ("1,204 games: 3 added, 0 removed"), or why it failed.
    pub(super) message: Option<String>,
}

impl From<crate::jobs::JobRun> for JobRun {
    fn from(r: crate::jobs::JobRun) -> Self {
        Self {
            id: ID(r.id.to_string()),
            job: r.job.into(),
            trigger: r.trigger.into(),
            outcome: r.outcome.into(),
            started_at: r.started_at as f64,
            finished_at: r.finished_at.map(|t| t as f64),
            seconds: r.duration_ms.map(|ms| ms as f64 / 1000.0),
            message: r.message,
        }
    }
}

/// A background job this server has, and its state now.
#[derive(SimpleObject)]
pub(super) struct JobState {
    pub(super) name: JobName,
    /// Its name for people ("Library scan").
    pub(super) label: String,
    pub(super) description: String,
    /// How often it runs on its own, in minutes; none if it does not (it follows another job,
    /// or its interval is 0).
    pub(super) interval_minutes: Option<f64>,
    /// When it next runs on its own (Unix seconds); none while it runs or if it does not.
    pub(super) next_run_at: Option<f64>,
    pub(super) running: bool,
    /// What started the run under way.
    pub(super) running_trigger: Option<JobTrigger>,
    /// When the run under way started (Unix seconds).
    pub(super) running_since: Option<f64>,
    /// Its latest run (the one under way, if it is running).
    pub(super) last_run: Option<JobRun>,
}

/// Every job this server has, as [`JobState`]s.
pub(super) async fn job_states(state: &AppState) -> Result<Vec<JobState>> {
    // Which are running first, then their runs: a run's end is recorded before its job stops
    // running, so read in this order a job never shows as stopped while its last run still
    // reads as running (read the other way round, a run ending in between did).
    let info = state.jobs.info(state);
    let mut last = state.jobs.last_runs().await.map_err(|e| gql(e.into()))?;
    Ok(info
        .into_iter()
        .map(|i| JobState {
            name: i.job.into(),
            label: i.job.label().to_owned(),
            description: i.job.description().to_owned(),
            interval_minutes: i.every.map(|d| d.as_secs_f64() / 60.0),
            next_run_at: i.next_at.map(unix),
            running: i.running.is_some(),
            running_trigger: i.running.map(|(t, _)| t.into()),
            running_since: i.running.map(|(_, at)| unix(at)),
            last_run: last.remove(&i.job).map(Into::into),
        })
        .collect())
}

#[derive(Default)]
pub(super) struct JobsQuery;

#[Object]
impl JobsQuery {
    /// Admins: the background jobs (scans, syncs, lookups, upkeep), and how each last ran.
    async fn jobs(&self, ctx: &Context<'_>) -> Result<Vec<JobState>> {
        admin(ctx).await?;
        job_states(state(ctx)).await
    }

    /// Admins: the latest runs of background jobs, newest first: of one job, or of all.
    async fn job_runs(
        &self,
        ctx: &Context<'_>,
        job: Option<JobName>,
        #[graphql(default = 50, validator(minimum = 1, maximum = 500))] limit: i32,
    ) -> Result<Vec<JobRun>> {
        admin(ctx).await?;
        let runs = state(ctx)
            .jobs
            .runs(job.map(Into::into), limit as u32)
            .await
            .map_err(|e| gql(e.into()))?;
        Ok(runs.into_iter().map(Into::into).collect())
    }
}

#[derive(Default)]
pub(super) struct JobsMutation;

#[Object]
impl JobsMutation {
    /// Admins: starts a background job now, as its schedule would (`METADATA`: games without
    /// game information). `CONFLICT` if it is running; `BAD_REQUEST` if this server does not
    /// have it (`MIRROR_SYNC` outside `MIRROR` mode).
    async fn run_job(&self, ctx: &Context<'_>, job: JobName) -> Result<Vec<JobState>> {
        let me = admin(ctx).await?;
        let state = state(ctx);
        let job: crate::jobs::Job = job.into();
        crate::jobs::start_by_admin(state, job).map_err(|e| gql(e.into()))?;
        record(ctx, &me, Entry::new(Kind::JobRun).about(job.label()));
        job_states(state).await
    }
}
