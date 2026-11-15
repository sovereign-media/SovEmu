//! Box art, downloaded by its own background job so a lookup stores game information at once.
//!
//! A lookup records where a game's box art is (`game_metadata.box_art_url` and its source). When
//! the lookup ends, this job downloads, a few at a time (`SOVEMU_ARTWORK_DOWNLOADS`), the box art
//! of every game that will show it: one with no local thumb, whose box art is not downloaded
//! yet and has not failed `MAX_ATTEMPTS` times. The queue is the table, so a restart resumes it
//! (the next lookup, even one with nothing to look up, starts the job again). Once a round is
//! done it asks the table again, so box art queued by a lookup that finished meanwhile is
//! fetched too.

use std::sync::Arc;

use serde::Serialize;
use tokio::sync::Semaphore;

use super::Metadata;
use crate::AppState;

/// Downloads of one URL that may fail before the job stops trying it.
pub const MAX_ATTEMPTS: i64 = 3;

/// Rounds of the queue one job works through (each round retries what failed in the last).
const MAX_ROUNDS: usize = MAX_ATTEMPTS as usize + 2;

/// Progress of the current (or last) box art job.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct ArtworkStatus {
    pub running: bool,
    /// Downloads queued since the job started.
    pub total: u64,
    pub done: u64,
    pub downloaded: u64,
    pub failed: u64,
}

#[derive(sqlx::FromRow)]
struct Wanted {
    game_id: i64,
    box_art_url: String,
    box_art_source: Option<String>,
}

/// Starts the job unless one is running: `true` if the caller now owns it and must
/// [`run_claimed`] it.
pub fn claim(meta: &Metadata) -> bool {
    let mut s = meta.artwork.lock().unwrap_or_else(|e| e.into_inner());
    if s.running {
        return false;
    }
    *s = ArtworkStatus { running: true, ..ArtworkStatus::default() };
    true
}

fn update(meta: &Metadata, f: impl FnOnce(&mut ArtworkStatus)) {
    f(&mut meta.artwork.lock().unwrap_or_else(|e| e.into_inner()));
}

/// The box art waiting to be downloaded. ScreenScraper's needs its credentials, so it waits
/// while ScreenScraper is not configured.
async fn wanted(state: &AppState) -> Vec<Wanted> {
    sqlx::query_as::<_, Wanted>(
        "SELECT m.game_id, m.box_art_url, m.box_art_source FROM game_metadata m
         JOIN games g ON g.id = m.game_id
         WHERE m.box_art_url IS NOT NULL AND m.box_art IS NULL AND m.box_art_attempts < ?1
           AND (',' || g.media || ',') NOT LIKE '%,thumb,%'
           AND (?2 OR COALESCE(m.box_art_source, '') != 'screenscraper')
         ORDER BY g.system, g.title",
    )
    .bind(MAX_ATTEMPTS)
    .bind(state.metadata.screenscraper.is_some())
    .fetch_all(&state.db)
    .await
    .unwrap_or_default()
}

/// Works through the queue; the caller has [`claim`]ed the job.
pub async fn run_claimed(state: AppState) {
    let meta = state.metadata.clone();
    let slots = Arc::new(Semaphore::new(meta.artwork_downloads.max(1)));
    for _ in 0..MAX_ROUNDS {
        let queue = wanted(&state).await;
        if queue.is_empty() {
            break;
        }
        tracing::info!("metadata: downloading box art for {} game(s)", queue.len());
        update(&meta, |s| s.total += queue.len() as u64);
        let mut tasks = tokio::task::JoinSet::new();
        for item in queue {
            let permit = slots.clone().acquire_owned().await.expect("never closed");
            let state = state.clone();
            tasks.spawn(async move {
                let ok = fetch(&state, &item).await;
                drop(permit);
                update(&state.metadata, |s| {
                    s.done += 1;
                    if ok { s.downloaded += 1 } else { s.failed += 1 }
                });
            });
        }
        while tasks.join_next().await.is_some() {}
    }
    update(&meta, |s| s.running = false);
    let s = meta.artwork_status();
    if s.total > 0 {
        tracing::info!("metadata: box art done ({} downloaded, {} failed)", s.downloaded, s.failed);
    }
}

/// Downloads one game's box art and records the file, or the failed attempt.
async fn fetch(state: &AppState, item: &Wanted) -> bool {
    let meta = &state.metadata;
    let url = match (item.box_art_source.as_deref(), &meta.screenscraper) {
        (Some("screenscraper"), Some(ss)) => ss.authorize(&item.box_art_url),
        _ => item.box_art_url.clone(),
    };
    let file = super::download_box_art(meta, item.game_id, &url).await;
    let result = match &file {
        Some(name) => {
            sqlx::query("UPDATE game_metadata SET box_art = ? WHERE game_id = ?")
                .bind(name)
                .bind(item.game_id)
                .execute(&state.db)
                .await
        }
        None => sqlx::query(
            "UPDATE game_metadata SET box_art_attempts = box_art_attempts + 1 WHERE game_id = ?",
        )
        .bind(item.game_id)
        .execute(&state.db)
        .await,
    };
    if let Err(e) = result {
        tracing::warn!("metadata: cannot record box art for game {}: {e}", item.game_id);
    }
    file.is_some()
}
