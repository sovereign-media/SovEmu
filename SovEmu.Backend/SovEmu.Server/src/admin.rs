//! Library and metadata administration: status, mirror sync, metadata jobs.

use axum::http::StatusCode;
use serde::Serialize;

use crate::error::{ApiError, ApiResult};
use crate::metadata::{self, JobStatus};
use crate::mirror::{self, SyncStatus};
use crate::{AppState, LibraryMode, library};

pub async fn scan_and_log(state: &AppState) {
    // Decoration sets may have been added beside the library: list them again too.
    state.reload_decorations();
    let started = std::time::Instant::now();
    match library::scan(&state.db, &state.config.roms_dir).await {
        Ok(r) => tracing::info!(
            library = %state.config.roms_dir.display(),
            games = r.found,
            added = r.added,
            removed = r.removed,
            elapsed_s = started.elapsed().as_secs_f64(),
            "library scanned"
        ),
        Err(e) => tracing::error!(error = %e, "library scan failed"),
    }
}

/// After a scan: look up game information for games that have none yet, if configured to.
pub async fn after_scan(state: &AppState) {
    if state.config.metadata_auto {
        metadata::run_job(state.clone(), None, false).await;
    }
}

/// Mirrors the source into the local library, then rescans it. No-op outside `mirror` mode or
/// while a sync is running.
pub async fn sync_then_scan(state: AppState) {
    let Some(source) = state.config.source_dir.clone() else { return };
    {
        let mut s = state.mirror.lock().unwrap_or_else(|e| e.into_inner());
        if s.running {
            return;
        }
        s.running = true;
    }
    let (dest, only, status) =
        (state.config.roms_dir.clone(), state.config.mirror_systems.clone(), state.mirror.clone());
    let threads = state.config.copy_threads;
    let _ =
        tokio::task::spawn_blocking(move || mirror::sync(&source, &dest, &only, threads, &status))
            .await;
    scan_and_log(&state).await;
}

#[derive(Debug, Serialize)]
pub struct LibraryStatus {
    pub(crate) mode: LibraryMode,
    pub(crate) games: i64,
    /// `cache` mode: bytes on local disk.
    pub(crate) cache_bytes: Option<u64>,
    /// `mirror` mode: the current or last sync.
    pub(crate) mirror: Option<SyncStatus>,
}

pub async fn library_status(state: &AppState) -> ApiResult<LibraryStatus> {
    let games: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM games").fetch_one(&state.db).await?;
    let cache_bytes = (state.config.library_mode == LibraryMode::Cache).then(|| {
        let dir = state.config.data_dir.join("cache");
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
    });
    let mirror = (state.config.library_mode == LibraryMode::Mirror)
        .then(|| state.mirror.lock().unwrap_or_else(|e| e.into_inner()).clone());
    Ok(LibraryStatus { mode: state.config.library_mode, games, cache_bytes, mirror })
}

/// Starts a mirror sync (then a rescan) in the background.
/// Starts a mirror sync in the background.
pub fn library_sync(state: &AppState) -> ApiResult<()> {
    if state.config.library_mode != LibraryMode::Mirror {
        return Err(ApiError::bad_request("the library is not in mirror mode"));
    }
    tokio::spawn(sync_then_scan(state.clone()));
    Ok(())
}

pub fn metadata_status(state: &AppState) -> JobStatus {
    state.metadata.status()
}

/// Starts a metadata job in the background.
/// Starts looking up game information in the background (`system`: one system only; `force`:
/// look up games that already have it too).
pub fn metadata_fetch(
    state: &AppState,
    system: Option<String>,
    force: bool,
) -> ApiResult<JobStatus> {
    if state.metadata.status().running {
        return Err(ApiError::new(StatusCode::CONFLICT, "a metadata job is already running"));
    }
    tokio::spawn(metadata::run_job(state.clone(), system, force));
    Ok(state.metadata.status())
}
