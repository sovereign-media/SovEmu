//! Pictures (box art, screenshots, logos) and gameplay videos, downloaded by their own
//! background job so a lookup stores game information at once.
//!
//! A lookup records where each is (`game_metadata.box_art_url`, `screenshot_url`,
//! `marquee_url`, `video_url`, and their sources). When the lookup ends, this job downloads, a
//! few at a time (`SovEmu__Artwork__MaxConcurrentDownloads`, within the sources' own thread limits), every
//! picture and, with `SovEmu__Metadata__Videos`, every video not downloaded yet that has not
//! failed `MAX_ATTEMPTS` times. Pictures are asked for at the size they are shown
//! (`SovEmu__Artwork__MaxPixels`), not as a full scan, and kept as lossy WebP. The queue is the
//! table, so a restart resumes it (the next lookup, even one with nothing to look up, starts
//! the job again). Once a round is done it asks the table again, so what a lookup that finished
//! meanwhile queued is fetched too.

use std::sync::Arc;

use serde::Serialize;
use tokio::sync::Semaphore;

use super::{Metadata, Picture};
use crate::AppState;

/// Downloads of one URL that may fail before the job stops trying it.
pub(crate) const MAX_ATTEMPTS: i64 = 3;

/// Rounds of the queue one job works through (each round retries what failed in the last).
const MAX_ROUNDS: usize = MAX_ATTEMPTS as usize + 2;

/// Progress of the current (or last) download job.
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
    /// `box_art`, `screenshot`, `marquee` or `video`.
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

/// The pictures and videos waiting to be downloaded. A provider's may need its credentials to
/// download ([`Provider::media_url`](sovemu_library::info::Provider::media_url)), so each waits
/// while the provider that found it is not configured.
async fn wanted(state: &AppState) -> Vec<Wanted> {
    let sources =
        serde_json::to_string(&state.metadata.providers.iter().map(|p| p.id()).collect::<Vec<_>>())
            .unwrap_or_else(|_| "[]".into());
    sqlx::query_as::<_, Wanted>(
        "SELECT game_id, 'box_art' AS kind, box_art_url AS url, box_art_source AS source
         FROM game_metadata
         WHERE box_art_url IS NOT NULL AND box_art IS NULL AND box_art_attempts < ?1
           AND (box_art_source IS NULL OR box_art_source IN (SELECT value FROM json_each(?2)))
         UNION ALL
         SELECT game_id, 'screenshot', screenshot_url, screenshot_source FROM game_metadata
         WHERE screenshot_url IS NOT NULL AND screenshot IS NULL AND screenshot_attempts < ?1
           AND (screenshot_source IS NULL
                OR screenshot_source IN (SELECT value FROM json_each(?2)))
         UNION ALL
         SELECT game_id, 'marquee', marquee_url, marquee_source FROM game_metadata
         WHERE marquee_url IS NOT NULL AND marquee IS NULL AND marquee_attempts < ?1
           AND (marquee_source IS NULL OR marquee_source IN (SELECT value FROM json_each(?2)))
         UNION ALL
         SELECT game_id, 'video', video_url, video_source FROM game_metadata
         WHERE ?3 AND video_url IS NOT NULL AND video IS NULL AND video_attempts < ?1
           AND (video_source IS NULL OR video_source IN (SELECT value FROM json_each(?2)))
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
        tracing::info!(count = queue.len(), "artwork download started");
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
            "artwork and video downloads done"
        );
    }
}

/// The picture a queued `kind` is, or none for a video.
fn picture(kind: &str) -> Option<Picture> {
    match kind {
        "box_art" => Some(Picture::BoxArt),
        "screenshot" => Some(Picture::Screenshot),
        "marquee" => Some(Picture::Marquee),
        _ => None,
    }
}

/// The statements that record a downloaded file and a failed attempt for a queued `kind`.
fn records(kind: &str) -> (&'static str, &'static str) {
    match kind {
        "box_art" => (
            "UPDATE game_metadata SET box_art = ? WHERE game_id = ?",
            "UPDATE game_metadata SET box_art_attempts = box_art_attempts + 1 WHERE game_id = ?",
        ),
        "screenshot" => (
            "UPDATE game_metadata SET screenshot = ? WHERE game_id = ?",
            "UPDATE game_metadata SET screenshot_attempts = screenshot_attempts + 1
             WHERE game_id = ?",
        ),
        "marquee" => (
            "UPDATE game_metadata SET marquee = ? WHERE game_id = ?",
            "UPDATE game_metadata SET marquee_attempts = marquee_attempts + 1 WHERE game_id = ?",
        ),
        _ => (
            "UPDATE game_metadata SET video = ? WHERE game_id = ?",
            "UPDATE game_metadata SET video_attempts = video_attempts + 1 WHERE game_id = ?",
        ),
    }
}

/// Downloads one game's picture or video and records the file, or the failed attempt.
async fn fetch(state: &AppState, item: &Wanted) -> bool {
    let meta = &state.metadata;
    let picture = picture(&item.kind);
    let max_px = if picture.is_some() { meta.artwork_max_px } else { None };
    let url = match item.source.as_deref().and_then(|id| meta.provider(id)) {
        Some(provider) => provider.media_url(&item.url, max_px),
        None => item.url.clone(),
    };
    let file = match picture {
        Some(picture) => super::download_picture(meta, item.game_id, picture, &url).await,
        None => super::download_video(meta, item.game_id, &url).await,
    };
    let (done, failed) = records(&item.kind);
    let result = match &file {
        Some(name) => sqlx::query(done).bind(name).bind(item.game_id).execute(&state.db).await,
        None => sqlx::query(failed).bind(item.game_id).execute(&state.db).await,
    };
    if let Err(e) = result {
        tracing::warn!(game_id = item.game_id, kind = %item.kind, error = %e, "cannot record a download");
    }
    file.is_some()
}
