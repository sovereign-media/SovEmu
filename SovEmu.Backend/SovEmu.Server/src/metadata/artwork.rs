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
//!
//! Every failure says why ([`DownloadError`]): the job counts each kind's downloads and failures
//! and each distinct reason (the Library page's progress), logs a reason the first time it
//! comes in a job, and keeps the last one per game and kind (`<kind>_error`), so Needs
//! attention lists the downloads that gave up, and why, after a restart too.

use std::sync::Arc;

use serde::Serialize;
use tokio::sync::Semaphore;

use super::{DownloadError, Metadata, Picture};
use crate::AppState;

/// Downloads of one URL that may fail before the job stops trying it.
pub(crate) const MAX_ATTEMPTS: i64 = 3;

/// Rounds of the queue one job works through (each round retries what failed in the last).
const MAX_ROUNDS: usize = MAX_ATTEMPTS as usize + 2;

/// A failure reason's count is kept for at most this many distinct reasons a job.
const MAX_REASONS: usize = 20;

/// What a download is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Kind {
    BoxArt,
    Screenshot,
    Marquee,
    Video,
}

impl Kind {
    pub const ALL: [Self; 4] = [Self::BoxArt, Self::Screenshot, Self::Marquee, Self::Video];

    /// Its name in the queue, and its columns' prefix (`box_art`, `box_art_url` …).
    pub fn column(self) -> &'static str {
        match self {
            Self::BoxArt => "box_art",
            Self::Screenshot => "screenshot",
            Self::Marquee => "marquee",
            Self::Video => "video",
        }
    }

    fn parse(column: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.column() == column)
    }

    /// The picture it is, or none for a video.
    fn picture(self) -> Option<Picture> {
        match self {
            Self::BoxArt => Some(Picture::BoxArt),
            Self::Screenshot => Some(Picture::Screenshot),
            Self::Marquee => Some(Picture::Marquee),
            Self::Video => None,
        }
    }

    /// The statements that record a downloaded file, and a failed attempt and its reason.
    fn records(self) -> (&'static str, &'static str) {
        match self {
            Self::BoxArt => (
                "UPDATE game_metadata SET box_art = ?, box_art_error = NULL WHERE game_id = ?",
                "UPDATE game_metadata SET box_art_attempts = box_art_attempts + 1,
                   box_art_error = ? WHERE game_id = ?",
            ),
            Self::Screenshot => (
                "UPDATE game_metadata SET screenshot = ?, screenshot_error = NULL
                 WHERE game_id = ?",
                "UPDATE game_metadata SET screenshot_attempts = screenshot_attempts + 1,
                   screenshot_error = ? WHERE game_id = ?",
            ),
            Self::Marquee => (
                "UPDATE game_metadata SET marquee = ?, marquee_error = NULL WHERE game_id = ?",
                "UPDATE game_metadata SET marquee_attempts = marquee_attempts + 1,
                   marquee_error = ? WHERE game_id = ?",
            ),
            Self::Video => (
                "UPDATE game_metadata SET video = ?, video_error = NULL WHERE game_id = ?",
                "UPDATE game_metadata SET video_attempts = video_attempts + 1,
                   video_error = ? WHERE game_id = ?",
            ),
        }
    }
}

/// One kind's downloads in the current (or last) job.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct KindProgress {
    pub kind: Kind,
    /// Queued since the job started (a retry counts again).
    pub total: u64,
    pub downloaded: u64,
    pub failed: u64,
}

/// A reason downloads failed, and how often, in the current (or last) job.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Failure {
    pub kind: Kind,
    pub reason: String,
    pub count: u64,
}

/// Progress of the current (or last) download job.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ArtworkStatus {
    pub running: bool,
    /// Downloads queued since the job started.
    pub total: u64,
    pub done: u64,
    pub downloaded: u64,
    pub failed: u64,
    /// Each kind's share, in [`Kind::ALL`]'s order.
    pub kinds: Vec<KindProgress>,
    /// Why downloads failed, in the order the reasons first came (at most [`MAX_REASONS`]).
    pub failures: Vec<Failure>,
}

impl Default for ArtworkStatus {
    fn default() -> Self {
        Self {
            running: false,
            total: 0,
            done: 0,
            downloaded: 0,
            failed: 0,
            kinds: Kind::ALL
                .into_iter()
                .map(|kind| KindProgress { kind, total: 0, downloaded: 0, failed: 0 })
                .collect(),
            failures: Vec::new(),
        }
    }
}

impl ArtworkStatus {
    fn kind(&mut self, kind: Kind) -> &mut KindProgress {
        let i = Kind::ALL.iter().position(|k| *k == kind).expect("every kind is listed");
        &mut self.kinds[i]
    }

    /// Counts one finished download; true if its failure's reason is new in this job.
    fn record(&mut self, kind: Kind, outcome: Result<(), &DownloadError>) -> bool {
        self.done += 1;
        let Err(e) = outcome else {
            self.downloaded += 1;
            self.kind(kind).downloaded += 1;
            return false;
        };
        self.failed += 1;
        self.kind(kind).failed += 1;
        let reason = e.to_string();
        if let Some(f) = self.failures.iter_mut().find(|f| f.kind == kind && f.reason == reason) {
            f.count += 1;
            return false;
        }
        if self.failures.len() < MAX_REASONS {
            self.failures.push(Failure { kind, reason, count: 1 });
        }
        true
    }
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

fn update<T>(meta: &Metadata, f: impl FnOnce(&mut ArtworkStatus) -> T) -> T {
    f(&mut meta.artwork.lock().unwrap_or_else(|e| e.into_inner()))
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
        let queue: Vec<(Kind, Wanted)> = wanted(&state)
            .await
            .into_iter()
            .filter_map(|w| Some((Kind::parse(&w.kind)?, w)))
            .collect();
        if queue.is_empty() {
            break;
        }
        tracing::info!(count = queue.len(), "artwork download started");
        update(&meta, |s| {
            s.total += queue.len() as u64;
            for (kind, _) in &queue {
                s.kind(*kind).total += 1;
            }
        });
        let mut tasks = tokio::task::JoinSet::new();
        for (kind, item) in queue {
            let permit = slots.clone().acquire_owned().await.expect("never closed");
            let state = state.clone();
            tasks.spawn(async move {
                let outcome = fetch(&state, kind, &item).await;
                drop(permit);
                let new = update(&state.metadata, |s| s.record(kind, outcome.as_ref().map(|_| ())));
                if let Err(e) = outcome {
                    // Once per reason a job: a quota or a refusal fails every download alike.
                    if new {
                        tracing::warn!(
                            game_id = item.game_id,
                            kind = kind.column(),
                            reason = %e,
                            "artwork download failed"
                        );
                    } else {
                        tracing::debug!(game_id = item.game_id, kind = kind.column(), reason = %e, "artwork download failed");
                    }
                }
            });
        }
        while tasks.join_next().await.is_some() {}
    }
    update(&meta, |s| s.running = false);
    let s = meta.artwork_status();
    if s.total > 0 {
        let failed = |kind| s.kinds.iter().find(|k| k.kind == kind).map_or(0, |k| k.failed);
        let videos = s.kinds.iter().find(|k| k.kind == Kind::Video).map_or(0, |k| k.downloaded);
        tracing::info!(
            downloaded = s.downloaded,
            videos,
            failed = s.failed,
            box_art_failed = failed(Kind::BoxArt),
            screenshots_failed = failed(Kind::Screenshot),
            logos_failed = failed(Kind::Marquee),
            videos_failed = failed(Kind::Video),
            "artwork and video downloads done"
        );
    }
}

/// Downloads one game's picture or video and records the file, or the failed attempt and why.
async fn fetch(state: &AppState, kind: Kind, item: &Wanted) -> Result<String, DownloadError> {
    let meta = &state.metadata;
    let picture = kind.picture();
    let max_px = if picture.is_some() { meta.artwork_max_px } else { None };
    let url = match item.source.as_deref().and_then(|id| meta.provider(id)) {
        Some(provider) => provider.media_url(&item.url, max_px),
        None => item.url.clone(),
    };
    let file = match picture {
        Some(picture) => super::download_picture(meta, item.game_id, picture, &url).await,
        None => super::download_video(meta, item.game_id, &url).await,
    };
    let (done, failed) = kind.records();
    let result = match &file {
        Ok(name) => sqlx::query(done).bind(name).bind(item.game_id).execute(&state.db).await,
        Err(e) => {
            sqlx::query(failed).bind(e.to_string()).bind(item.game_id).execute(&state.db).await
        }
    };
    if let Err(e) = result {
        tracing::warn!(game_id = item.game_id, kind = kind.column(), error = %e, "cannot record a download");
    }
    file
}
