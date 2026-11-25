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
    // And backgrounds and system photos.
    state.reload_appearance();
    let started = std::time::Instant::now();
    let r = library::scan(&state.db, &state.config.roms_dir).await?;
    state.game_counts.refresh(&state.db).await?;
    // Games came or went: the declared collections hold what the library now has.
    let declared = crate::collections::declare(&state.db, &state.config.collections).await?;
    if !declared.missing.is_empty() {
        tracing::warn!(
            games = %declared.missing.join(", "),
            "declared collections name games the library does not have"
        );
    }
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
    let s = state.clone();
    let _ = state.jobs.run(Job::Reconcile, trigger, async move { reconcile(&s).await }).await;
    if state.config.metadata_auto {
        let _ = state
            .jobs
            .run(
                Job::Metadata,
                trigger,
                lookup(state.clone(), metadata::Scope::automatic(&state.config)),
            )
            .await;
    } else {
        download_missing(state);
    }
}

/// [`after_scan`] without waiting: a scheduled scan's run ends when the scan does.
pub(crate) fn after_scan_in_background(state: &AppState) {
    let s = state.clone();
    let _ = state.jobs.start(Job::Reconcile, Trigger::Schedule, async move {
        let report = reconcile(&s).await?;
        fetch_missing(&s, Trigger::Schedule);
        Ok(report)
    });
}

/// Reconciles game information and artwork with the library ([`Job::Reconcile`]'s work).
pub(crate) async fn reconcile(state: &AppState) -> anyhow::Result<String> {
    Ok(metadata::reconcile::reconcile(state).await?.to_string())
}

/// What follows a reconcile, without waiting: the lookup of games without information, which
/// ends by downloading the artwork that is missing (with `SovEmu__Metadata__AutoLookup`, or when an admin
/// asked); else only the downloads.
pub(crate) fn fetch_missing(state: &AppState, trigger: Trigger) {
    if state.config.metadata_auto || trigger == Trigger::Admin {
        let _ = state.jobs.start(
            Job::Metadata,
            trigger,
            lookup(state.clone(), metadata::Scope::automatic(&state.config)),
        );
    } else {
        download_missing(state);
    }
}

/// Starts the artwork downloads unless they are running.
fn download_missing(state: &AppState) {
    if metadata::artwork::claim(&state.metadata) {
        tokio::spawn(metadata::artwork::run_claimed(state.clone()));
    }
}

/// Looks up game information ([`Job::Metadata`]'s work) for the games `scope` covers.
pub(crate) async fn lookup(state: AppState, scope: metadata::Scope) -> anyhow::Result<String> {
    metadata::run_job(state.clone(), scope).await;
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
pub(crate) fn metadata_fetch(state: &AppState, scope: metadata::Scope) -> ApiResult<JobStatus> {
    if let Some(unknown) = scope.providers.iter().find(|id| state.metadata.provider(id).is_none()) {
        let known: Vec<&str> = state.metadata.providers.iter().map(|p| p.id()).collect();
        return Err(ApiError::bad_request(format!(
            "{unknown} is not a configured source (configured: {})",
            known.join(", ")
        )));
    }
    state.jobs.start(Job::Metadata, Trigger::Admin, lookup(state.clone(), scope)).map_err(|e| {
        match e {
            JobError::Running => ApiError::conflict("a metadata job is already running"),
            e => e.into(),
        }
    })?;
    Ok(state.metadata.status())
}

/// The decoration sets found, and the set each system's games are framed with.
pub(crate) struct DecorationsReport {
    /// Each set, by name, and what it holds.
    pub(crate) sets: Vec<(String, decorations::SetSummary)>,
    /// `SovEmu__Decorations__Set`: the set for systems without one of their own.
    pub(crate) default: String,
    /// The folders sets are read from, first wins.
    pub(crate) folders: Vec<String>,
    pub(crate) systems: Vec<SystemDecoration>,
}

pub(crate) struct SystemDecoration {
    pub(crate) system: crate::api::SystemSummary,
    pub(crate) set: String,
    /// Chosen for this system (`SovEmu__Systems__<id>__Decoration`), not the default.
    pub(crate) per_system: bool,
    /// What its games are framed with when they have no image of their own: the system's
    /// image, else the set's default. None for `none`, or a set that isn't there.
    pub(crate) preview: Option<decorations::Decoration>,
}

/// Every system with its decoration set and a preview, and every set found.
pub(crate) async fn decorations_report(state: &AppState) -> ApiResult<DecorationsReport> {
    let found = state.decorations();
    let systems = crate::api::systems_with_games(state).await?;
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

/// Makes a backup ([`Job::Backup`]'s work) in the backup folder, keeping the newest.
pub(crate) async fn back_up(state: AppState) -> anyhow::Result<crate::backup::Made> {
    let dir = state.config.backup_dir();
    let made =
        crate::backup::create(&state.db, &state.config.paths(), &dir, state.config.backup_keep)
            .await?;
    tracing::info!(
        backup = %made.file.name,
        bytes = made.file.bytes,
        pruned = made.pruned,
        "backup made"
    );
    Ok(made)
}

/// A backup, for an admin only (`/media/backups/<name>`, the signed URL `backups` hands out):
/// it holds every account's password hash and the signing keys.
pub(crate) async fn backup_file(
    axum::extract::State(state): axum::extract::State<AppState>,
    crate::assets::SignedUser(user_id): crate::assets::SignedUser,
    axum::extract::Path(name): axum::extract::Path<String>,
    request: axum::extract::Request,
) -> ApiResult<axum::response::Response> {
    use axum::http::{HeaderValue, header};
    use axum::response::IntoResponse;
    use tower::ServiceExt;
    let admin = state.identity.user(user_id).await.map_err(ApiError::internal)?;
    if !admin.is_some_and(|u| u.role == sovemu_identity::Role::Admin) {
        return Err(ApiError::forbidden());
    }
    let file = crate::backup::find(&state.config.backup_dir(), &name)
        .ok_or_else(|| ApiError::not_found("backup"))?;
    let mut response = tower_http::services::ServeFile::new(file)
        .oneshot(request)
        .await
        .map_err(ApiError::internal)?
        .into_response();
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    if let Ok(v) = HeaderValue::from_str(&format!("attachment; filename=\"{name}\"")) {
        headers.insert(header::CONTENT_DISPOSITION, v);
    }
    Ok(response)
}
