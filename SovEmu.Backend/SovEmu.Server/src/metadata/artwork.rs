//! Box art and gameplay videos, downloaded by their own background job so a lookup stores game
//! information at once.
//!
//! A lookup records where a game's box art and video are (`game_metadata.box_art_url`,
//! `video_url`, and their sources). When the lookup ends, this job downloads, a few at a time
//! (`SOVEMU_ARTWORK_DOWNLOADS`, within the sources' own thread limits), the box art of every
//! game that will show it (no local thumb) and, with `SOVEMU_METADATA_VIDEOS`, the video of every
//! game with none of its own: those not downloaded yet that have not failed `MAX_ATTEMPTS`
//! times. Box art is asked for at the size it is shown (`SOVEMU_ARTWORK_MAX_PX`), not as a full
//! scan. The queue is the table, so a restart resumes it
//! (the next lookup, even one with nothing to look up, starts the job again). Once a round is
//! done it asks the table again, so box art queued by a lookup that finished meanwhile is
//! fetched too.

use std::sync::Arc;

use serde::Serialize;
use tokio::sync::Semaphore;

use super::Metadata;
use crate::AppState;

/// Downloads of one URL that may fail before the job stops trying it.
pub(crate) const MAX_ATTEMPTS: i64 = 3;

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
    /// Of `downloaded`, the videos.
    pub videos: u64,
}

#[derive(sqlx::FromRow)]
struct Wanted {
    game_id: i64,
    /// `box_art` or `video`.
    kind: String,
    url: String,
    source: Option<String>,
}

/// Starts the job unless one is running: `true` if the caller now owns it and must
/// [`run_claimed`] it.
pub(crate) fn claim(meta: &Metadata) -> bool {
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

/// The box art waiting to be downloaded. A provider's may need its credentials to download
/// ([`Provider::box_art_url`](sovemu_library::info::Provider::box_art_url)), so it waits while
/// the provider that found it is not configured.
async fn wanted(state: &AppState) -> Vec<Wanted> {
    let sources =
        serde_json::to_string(&state.metadata.providers.iter().map(|p| p.id()).collect::<Vec<_>>())
            .unwrap_or_else(|_| "[]".into());
    sqlx::query_as::<_, Wanted>(
        "SELECT m.game_id, 'box_art' AS kind, m.box_art_url AS url, m.box_art_source AS source
         FROM game_metadata m JOIN games g ON g.id = m.game_id
         WHERE m.box_art_url IS NOT NULL AND m.box_art IS NULL AND m.box_art_attempts < ?1
           AND (',' || g.media || ',') NOT LIKE '%,thumb,%'
           AND (m.box_art_source IS NULL
                OR m.box_art_source IN (SELECT value FROM json_each(?2)))
         UNION ALL
         SELECT m.game_id, 'video', m.video_url, m.video_source
         FROM game_metadata m JOIN games g ON g.id = m.game_id
         WHERE ?3 AND m.video_url IS NOT NULL AND m.video IS NULL AND m.video_attempts < ?1
           AND (',' || g.media || ',') NOT LIKE '%,video,%'
           AND (m.video_source IS NULL OR m.video_source IN (SELECT value FROM json_each(?2)))
         ORDER BY kind, game_id",
    )
    .bind(MAX_ATTEMPTS)
    .bind(sources)
    .bind(state.metadata.videos)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default()
}

/// Works through the queue; the caller has [`claim`]ed the job.
pub(crate) async fn run_claimed(state: AppState) {
    let meta = state.metadata.clone();
    // Clears `running` however the downloads end (a panic included), so the next can start.
    let _running = super::scopeguard(meta.clone(), |m| update(m, |s| s.running = false));
    // Downloads count against the sources' accounts too: never more at once than the busiest
    // source allows (ScreenScraper: the account's threads).
    let allowed = meta.providers.iter().map(|p| p.concurrency()).max().unwrap_or(1);
    let slots = Arc::new(Semaphore::new(meta.artwork_downloads.min(allowed).max(1)));
    for _ in 0..MAX_ROUNDS {
        let queue = wanted(&state).await;
        if queue.is_empty() {
            break;
        }
        tracing::info!(count = queue.len(), "box art download started");
        update(&meta, |s| s.total += queue.len() as u64);
        let mut tasks = tokio::task::JoinSet::new();
        for item in queue {
            let permit = slots.clone().acquire_owned().await.expect("never closed");
            let state = state.clone();
            tasks.spawn(async move {
                let ok = fetch(&state, &item).await;
                drop(permit);
                let video = item.kind == "video";
                update(&state.metadata, |s| {
                    s.done += 1;
                    if ok {
                        s.downloaded += 1;
                        s.videos += u64::from(video);
                    } else {
                        s.failed += 1
                    }
                });
            });
        }
        while tasks.join_next().await.is_some() {}
    }
    update(&meta, |s| s.running = false);
    let s = meta.artwork_status();
    if s.total > 0 {
        tracing::info!(
            downloaded = s.downloaded,
            videos = s.videos,
            failed = s.failed,
            "box art and video downloads done"
        );
    }
}

/// Downloads one game's box art or video and records the file, or the failed attempt.
async fn fetch(state: &AppState, item: &Wanted) -> bool {
    let meta = &state.metadata;
    let video = item.kind == "video";
    let max_px = if video { None } else { meta.artwork_max_px };
    let url = match item.source.as_deref().and_then(|id| meta.provider(id)) {
        Some(provider) => provider.media_url(&item.url, max_px),
        None => item.url.clone(),
    };
    let file = if video {
        super::download_video(meta, item.game_id, &url).await
    } else {
        super::download_box_art(meta, item.game_id, &url).await
    };
    let (done, failed) = if video {
        (
            "UPDATE game_metadata SET video = ? WHERE game_id = ?",
            "UPDATE game_metadata SET video_attempts = video_attempts + 1 WHERE game_id = ?",
        )
    } else {
        (
            "UPDATE game_metadata SET box_art = ? WHERE game_id = ?",
            "UPDATE game_metadata SET box_art_attempts = box_art_attempts + 1 WHERE game_id = ?",
        )
    };
    let result = match &file {
        Some(name) => sqlx::query(done).bind(name).bind(item.game_id).execute(&state.db).await,
        None => sqlx::query(failed).bind(item.game_id).execute(&state.db).await,
    };
    if let Err(e) = result {
        tracing::warn!(game_id = item.game_id, kind = %item.kind, error = %e, "cannot record a download");
    }
    file.is_some()
}
