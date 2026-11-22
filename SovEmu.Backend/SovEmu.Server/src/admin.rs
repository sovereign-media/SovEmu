//! Library and metadata administration: status, mirror sync, metadata jobs.

use serde::Serialize;

use crate::error::{ApiError, ApiResult};
use crate::jobs::{self, Job, JobError, Summary, Trigger};
use crate::library::{self, ScanReport};
use crate::metadata::{self, JobStatus};
use crate::{AppState, LibraryMode};
use sovemu_library::decorations;
use sovemu_library::mirror::{self, SyncStatus};

/// Rescans the library ([`Job::Scan`](crate::jobs::Job::Scan)'s work, and the end of a mirror
/// sync's).
pub(crate) async fn scan(state: AppState) -> anyhow::Result<ScanReport> {
    // Decoration sets may have been added beside the library: list them again too.
    state.reload_decorations();
    let started = std::time::Instant::now();
    let r = library::scan(&state.db, &state.config.roms_dir).await?;
    tracing::info!(
        library = %state.config.roms_dir.display(),
        games = r.found,
        added = r.added,
        removed = r.removed,
        elapsed_s = started.elapsed().as_secs_f64(),
        "library scanned"
    );
    Ok(r)
}

impl Summary for ScanReport {
    fn summary(&self) -> Option<String> {
        Some(format!("{} games: {} added, {} removed", self.found, self.added, self.removed))
    }
}

/// After a scan: look up game information for games that have none yet, if configured to, and
/// wait for it.
pub(crate) async fn after_scan(state: &AppState, trigger: Trigger) {
    if state.config.metadata_auto {
        let _ = state.jobs.run(Job::Metadata, trigger, lookup(state.clone(), None, false)).await;
    }
}

/// [`after_scan`] without waiting: a scheduled scan's run ends when the scan does.
pub(crate) fn after_scan_in_background(state: &AppState) {
    if state.config.metadata_auto {
        let _ =
            state.jobs.start(Job::Metadata, Trigger::Schedule, lookup(state.clone(), None, false));
    }
}

/// Looks up game information ([`Job::Metadata`]'s work): games without it, or with `force`
/// every game; one system, or all.
pub(crate) async fn lookup(
    state: AppState,
    system: Option<String>,
    force: bool,
) -> anyhow::Result<String> {
    metadata::run_job(state.clone(), system, force).await;
    let s = state.metadata.status();
    let mut summary =
        format!("{} found, {} not found, {} to retry", s.found, s.not_found, s.failed);
    for note in &s.notes {
        summary.push_str("; ");
        summary.push_str(note);
    }
    Ok(summary)
}

/// Mirrors the source into the local library, then rescans it ([`Job::MirrorSync`]'s work).
/// Fails if the source cannot be reached; files that failed to copy are counted.
pub(crate) async fn sync_then_scan(state: AppState) -> anyhow::Result<String> {
    let Some(source) = state.config.source_dir.clone() else {
        anyhow::bail!("the library is not in mirror mode");
    };
    let (dest, only, status) =
        (state.config.roms_dir.clone(), state.config.mirror_systems.clone(), state.mirror.clone());
    let threads = state.config.copy_threads;
    // A file is copied to `<name>.part` and renamed into place, so a sync cut short (the
    // server stopping) leaves no half-copied game behind.
    tokio::task::spawn_blocking(move || mirror::sync(&source, &dest, &only, threads, &status))
        .await?;
    let s = state.mirror.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if let Some(error) = s.error {
        anyhow::bail!(error);
    }
    let report = scan(state).await?;
    Ok(format!(
        "{} files copied, {} removed, {} failed; {}",
        s.files_done,
        s.removed,
        s.failed,
        report.summary().unwrap_or_default()
    ))
}

#[derive(Debug, Serialize)]
pub(crate) struct LibraryStatus {
    pub(crate) mode: LibraryMode,
    pub(crate) games: i64,
    /// `cache` mode: bytes on local disk.
    pub(crate) cache_bytes: Option<u64>,
    /// `mirror` mode: the current or last sync.
    pub(crate) mirror: Option<SyncStatus>,
}

pub(crate) async fn library_status(state: &AppState) -> ApiResult<LibraryStatus> {
    let games: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM games").fetch_one(&state.db).await?;
    let cache_bytes = if state.config.library_mode == LibraryMode::Cache {
        // A walk of the cache: off the async threads.
        let dir = state.config.paths().cache_games();
        tokio::task::spawn_blocking(move || {
            let mut total = 0u64;
            let mut stack = vec![dir];
            while let Some(d) = stack.pop() {
                for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
                    match e.metadata() {
                        Ok(m) if m.is_dir() => stack.push(e.path()),
                        Ok(m) => total += m.len(),
                        Err(_) => {}
                    }
                }
            }
            total
        })
        .await
        .ok()
    } else {
        None
    };
    let mirror = (state.config.library_mode == LibraryMode::Mirror)
        .then(|| state.mirror.lock().unwrap_or_else(|e| e.into_inner()).clone());
    Ok(LibraryStatus { mode: state.config.library_mode, games, cache_bytes, mirror })
}

/// Starts a mirror sync (then a rescan) in the background.
pub(crate) fn library_sync(state: &AppState) -> ApiResult<()> {
    if state.config.library_mode != LibraryMode::Mirror {
        return Err(ApiError::bad_request("the library is not in mirror mode"));
    }
    Ok(jobs::start_by_admin(state, Job::MirrorSync)?)
}

pub(crate) fn metadata_status(state: &AppState) -> JobStatus {
    state.metadata.status()
}

/// Starts looking up game information in the background (`system`: one system only; `force`:
/// look up games that already have it too).
pub(crate) fn metadata_fetch(
    state: &AppState,
    system: Option<String>,
    force: bool,
) -> ApiResult<JobStatus> {
    state.jobs.start(Job::Metadata, Trigger::Admin, lookup(state.clone(), system, force)).map_err(
        |e| match e {
            JobError::Running => ApiError::conflict("a metadata job is already running"),
            e => e.into(),
        },
    )?;
    Ok(state.metadata.status())
}

/// The decoration sets found, and the set each system's games are framed with.
pub(crate) struct DecorationsReport {
    /// Each set, by name, and what it holds.
    pub(crate) sets: Vec<(String, decorations::SetSummary)>,
    /// `SOVEMU_DECORATION`: the set for systems without one of their own.
    pub(crate) default: String,
    /// The folders sets are read from, first wins.
    pub(crate) folders: Vec<String>,
    pub(crate) systems: Vec<SystemDecoration>,
}

pub(crate) struct SystemDecoration {
    pub(crate) system: crate::api::SystemSummary,
    pub(crate) set: String,
    /// Chosen for this system (`SOVEMU_SYSTEMS__<id>__DECORATION`), not the default.
    pub(crate) per_system: bool,
    /// What its games are framed with when they have no image of their own: the system's
    /// image, else the set's default. None for `none`, or a set that isn't there.
    pub(crate) preview: Option<decorations::Decoration>,
}

/// Every system with its decoration set and a preview, and every set found.
pub(crate) async fn decorations_report(state: &AppState) -> ApiResult<DecorationsReport> {
    let found = state.decorations();
    let systems = crate::api::list_systems(state).await?;
    let config = &state.config;
    Ok(DecorationsReport {
        sets: found
            .sets()
            .into_iter()
            .filter_map(|s| Some((s.clone(), found.summary(&s)?)))
            .collect(),
        default: config.decoration.clone(),
        folders: config.decoration_dirs().iter().map(|d| d.display().to_string()).collect(),
        systems: systems
            .into_iter()
            .map(|system| {
                let id = system.system.id;
                let set = config.decoration_for(id).to_owned();
                let per_system = config.systems.get(id).is_some_and(|s| s.decoration.is_some());
                let preview =
                    (set != decorations::NONE).then(|| found.for_game(&set, id, "")).flatten();
                SystemDecoration { system, set, per_system, preview }
            })
            .collect(),
    })
}
