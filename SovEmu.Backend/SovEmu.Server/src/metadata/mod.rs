//! Game information from metadata services, fetched by a background job and kept in
//! `game_metadata`.
//!
//! Sources, tried in order and merged (an earlier source's fields win):
//! - [`fbneo`]: FinalBurn Neo's own DATs, for arcade and Neo Geo sets. No account needed; turns
//!   `mslug` into *Metal Slug - Super Vehicle-001* with its year and maker.
//! - [`launchbox`]: the LaunchBox Games Database, from the dump it publishes (downloaded and
//!   refreshed by the server). By name and platform. No account needed: the default source,
//!   and the one with the most community ratings.
//! - [`screenscraper`]: ScreenScraper, the service retro frontends (Recalbox, Batocera, RetroBat,
//!   ES-DE) scrape from. Matches by MD5 + size + file name. Needs developer credentials; a user
//!   account raises the quota.
//! - [`thegamesdb`]: TheGamesDB, by name and platform. Needs an API key.
//!
//! A lookup stores what it finds at once, box art included as a URL; [`artwork`] downloads the
//! box art afterwards, in the background, for the games that will show it.
//!
//! ScreenScraper and TheGamesDB are only asked about games the sources before them left
//! incomplete, so their quotas go on what LaunchBox does not know.

pub(crate) mod artwork;

use sovemu_library::info::{
    GameInfo, LookupInput, MetaError, fbneo, http_client, launchbox, md5_file, screenscraper,
    thegamesdb,
};

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;

use crate::AppState;

/// Progress of the current (or last) metadata job.
#[derive(Debug, Clone, Default, Serialize)]
pub struct JobStatus {
    pub running: bool,
    pub total: u64,
    pub done: u64,
    pub found: u64,
    pub not_found: u64,
    pub failed: u64,
    /// Which sources are configured.
    pub sources: Vec<&'static str>,
    /// Why a source stopped early (bad credentials, quota), if one did.
    pub notes: Vec<String>,
    /// The box art downloads that follow a lookup.
    pub artwork: artwork::ArtworkStatus,
}

/// The configured sources and the job's shared state.
pub struct Metadata {
    pub fbneo: fbneo::FbNeoDats,
    pub launchbox: Option<launchbox::LaunchBox>,
    pub screenscraper: Option<screenscraper::ScreenScraper>,
    pub thegamesdb: Option<thegamesdb::TheGamesDb>,
    pub media_dir: PathBuf,
    pub status: Mutex<JobStatus>,
    pub artwork: Mutex<artwork::ArtworkStatus>,
    /// Box art downloads at once.
    pub artwork_downloads: usize,
    http: reqwest::Client,
    screenscraper_delay: Duration,
}

impl std::fmt::Debug for Metadata {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Metadata").finish_non_exhaustive()
    }
}

impl Metadata {
    pub fn new(
        dir: PathBuf,
        launchbox: Option<launchbox::LaunchBox>,
        screenscraper: Option<screenscraper::ScreenScraper>,
        thegamesdb: Option<thegamesdb::TheGamesDb>,
        screenscraper_delay: Duration,
        artwork_downloads: usize,
    ) -> Arc<Self> {
        let mut sources = vec!["fbneo"];
        if launchbox.is_some() {
            sources.push("launchbox");
        }
        if screenscraper.is_some() {
            sources.push("screenscraper");
        }
        if thegamesdb.is_some() {
            sources.push("thegamesdb");
        }
        Arc::new(Self {
            fbneo: fbneo::FbNeoDats::new(dir.join("fbneo")),
            launchbox,
            screenscraper,
            thegamesdb,
            media_dir: dir.join("media"),
            status: Mutex::new(JobStatus { sources, ..JobStatus::default() }),
            artwork: Mutex::new(artwork::ArtworkStatus::default()),
            artwork_downloads,
            http: http_client(),
            screenscraper_delay,
        })
    }

    pub fn status(&self) -> JobStatus {
        let mut status = self.status.lock().unwrap_or_else(|e| e.into_inner()).clone();
        status.artwork = self.artwork_status();
        status
    }

    pub fn artwork_status(&self) -> artwork::ArtworkStatus {
        self.artwork.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn update(&self, f: impl FnOnce(&mut JobStatus)) {
        f(&mut self.status.lock().unwrap_or_else(|e| e.into_inner()));
    }
}

#[derive(sqlx::FromRow)]
struct Pending {
    id: i64,
    system: String,
    path: String,
    title: String,
    size: i64,
    /// `not_found`, or null for a game never looked up.
    status: Option<String>,
    /// The services asked last time.
    tried: Option<String>,
}

/// Whether a game should be looked up: never looked up, or not found by the services asked
/// then while one it was not asked is now configured.
fn needs_lookup(status: Option<&str>, tried: Option<&str>, available: &[&str]) -> bool {
    match status {
        None => true,
        Some("not_found") => {
            let tried: Vec<&str> = tried.unwrap_or("").split(',').collect();
            available.iter().any(|s| !tried.contains(s))
        }
        Some(_) => false,
    }
}

/// Looks up every game without metadata, and every game no service found while a service it was
/// not asked is now configured (all of a system's, or every game with `force`), one at a time,
/// and stores what it finds. Returns at once if a job is already running.
pub(crate) async fn run_job(state: AppState, system: Option<String>, force: bool) {
    let meta = state.metadata.clone();
    {
        let mut s = meta.status.lock().unwrap_or_else(|e| e.into_inner());
        if s.running {
            return;
        }
        let sources = std::mem::take(&mut s.sources);
        *s = JobStatus { running: true, sources, ..JobStatus::default() };
    }
    let mut available = vec!["fbneo"];
    if meta.launchbox.is_some() {
        available.push("launchbox");
    }
    if meta.screenscraper.is_some() {
        available.push("screenscraper");
    }
    if meta.thegamesdb.is_some() {
        available.push("thegamesdb");
    }
    let games: Vec<Pending> = sqlx::query_as::<_, Pending>(
        "SELECT g.id, g.system, g.path, g.title, g.size, m.status, m.tried FROM games g
         LEFT JOIN game_metadata m ON m.game_id = g.id
         WHERE (?1 IS NULL OR g.system = ?1)
           AND (?2 OR m.game_id IS NULL OR m.status = 'not_found')
         ORDER BY g.system, g.title",
    )
    .bind(&system)
    .bind(force)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default()
    .into_iter()
    .filter(|g| force || needs_lookup(g.status.as_deref(), g.tried.as_deref(), &available))
    .collect();
    meta.update(|s| s.total = games.len() as u64);
    tracing::info!(count = games.len(), "metadata lookup started");

    // Download, refresh or read the LaunchBox dump once for the whole job (not at all when
    // there is nothing to look up, as after most scans).
    let launchbox = match &meta.launchbox {
        Some(_) if games.is_empty() => None,
        Some(lb) => match lb.prepare().await {
            Ok(index) => Some(index),
            Err(e) => {
                tracing::warn!(error = %e, "metadata: LaunchBox unavailable");
                meta.update(|s| s.notes.push(format!("LaunchBox: {e}")));
                None
            }
        },
        None => None,
    };
    let (mut use_ss, mut use_tgdb) = (meta.screenscraper.is_some(), meta.thegamesdb.is_some());
    for game in games {
        let path = state.config.roms_dir.join(&game.path);
        let input = LookupInput {
            system: game.system.clone(),
            title: game.title.clone(),
            file_name: Path::new(&game.path)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            size: game.size as u64,
            md5: if use_ss {
                tokio::task::spawn_blocking(move || md5_file(&path)).await.ok().flatten()
            } else {
                None
            },
        };

        let mut info = GameInfo::default();
        let mut sources = Vec::new();
        // The services that answered, found or not: a game none found is asked again only by
        // a service not among them.
        let mut tried = vec!["fbneo"];
        let mut failed = false;
        // The source of the box art URL, which the box art job needs (for credentials).
        let mut art = None;
        if let Some(found) = meta.fbneo.lookup(&meta.http, &input).await {
            merge_from(&mut info, &mut art, found, "fbneo");
            sources.push("fbneo");
        }
        if let Some(index) = &launchbox {
            tried.push("launchbox");
            // Arcade files are set names (`mslug`): match the title FBNeo gave them.
            let title = info.title.as_deref().unwrap_or(&input.title);
            if let Some(found) = index.lookup(&input.system, title) {
                merge_from(&mut info, &mut art, found, "launchbox");
                sources.push("launchbox");
            }
        }
        if use_ss && !info.complete() {
            let ss = meta.screenscraper.as_ref().expect("checked");
            match ss.lookup(&input).await {
                Ok(Some(found)) => {
                    merge_from(&mut info, &mut art, found, "screenscraper");
                    sources.push("screenscraper");
                    tried.push("screenscraper");
                }
                Ok(None) => tried.push("screenscraper"),
                Err(e @ (MetaError::Unauthorized(_) | MetaError::Quota(_))) => {
                    tracing::warn!(error = %e, "metadata: ScreenScraper stopped");
                    meta.update(|s| s.notes.push(format!("ScreenScraper: {e}")));
                    use_ss = false;
                    failed = true;
                }
                Err(e) => {
                    tracing::debug!(game = %game.title, error = %e, "metadata: ScreenScraper lookup failed");
                    failed = true;
                }
            }
            // Be gentle: free ScreenScraper accounts get one request at a time.
            tokio::time::sleep(meta.screenscraper_delay).await;
        }
        if use_tgdb && !info.complete() {
            let tgdb = meta.thegamesdb.as_ref().expect("checked");
            match tgdb.lookup(&input).await {
                Ok(Some(found)) => {
                    merge_from(&mut info, &mut art, found, "thegamesdb");
                    sources.push("thegamesdb");
                    tried.push("thegamesdb");
                }
                Ok(None) => tried.push("thegamesdb"),
                Err(e @ (MetaError::Unauthorized(_) | MetaError::Quota(_))) => {
                    tracing::warn!(error = %e, "metadata: TheGamesDB stopped");
                    meta.update(|s| s.notes.push(format!("TheGamesDB: {e}")));
                    use_tgdb = false;
                    failed = true;
                }
                Err(e) => {
                    tracing::debug!(game = %game.title, error = %e, "metadata: TheGamesDB lookup failed");
                    failed = true;
                }
            }
        }

        let found = !sources.is_empty();
        if found || !failed {
            let _ = store(&state, game.id, &info, art, &sources, &tried).await;
        }
        meta.update(|s| {
            s.done += 1;
            if found {
                s.found += 1;
            } else if failed {
                s.failed += 1;
            } else {
                s.not_found += 1;
            }
        });
    }
    // Game information renames games: which share a title may have changed.
    if let Err(e) = crate::releases::refresh(&state.db).await {
        tracing::warn!(error = %e, "metadata: updating release labels");
    }
    // Claimed before this job reads as finished, so no one sees both idle in between.
    let artwork = artwork::claim(&meta);
    meta.update(|s| s.running = false);
    let s = meta.status();
    tracing::info!(
        found = s.found,
        not_found = s.not_found,
        failed = s.failed,
        "metadata lookup done"
    );
    // Box art in the background: the scan loop waits for this job, not for the downloads.
    if artwork {
        tokio::spawn(artwork::run_claimed(state));
    }
}

/// Merges `found` into `info`, noting `source` in `art` if it is where the box art URL came
/// from (an earlier source's URL wins, as every field does).
fn merge_from(
    info: &mut GameInfo,
    art: &mut Option<&'static str>,
    found: GameInfo,
    source: &'static str,
) {
    if info.box_art_url.is_none() && found.box_art_url.is_some() {
        *art = Some(source);
    }
    info.merge(found);
}

async fn store(
    state: &AppState,
    game_id: i64,
    info: &GameInfo,
    box_art_source: Option<&str>,
    sources: &[&str],
    tried: &[&str],
) -> Result<(), sqlx::Error> {
    // Downloaded box art is kept; a new URL is tried afresh (box_art_attempts back to 0).
    sqlx::query(
        "INSERT INTO game_metadata (game_id, status, sources, title, description, release_date,
           developer, publisher, genre, players, rating, box_art_url, box_art_source, tried,
           fetched_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, datetime('now'))
         ON CONFLICT(game_id) DO UPDATE SET status = excluded.status, sources = excluded.sources,
           title = excluded.title, description = excluded.description,
           release_date = excluded.release_date, developer = excluded.developer,
           publisher = excluded.publisher, genre = excluded.genre, players = excluded.players,
           rating = excluded.rating,
           box_art_attempts = CASE WHEN excluded.box_art_url IS game_metadata.box_art_url
             THEN game_metadata.box_art_attempts ELSE 0 END,
           box_art_url = excluded.box_art_url, box_art_source = excluded.box_art_source,
           tried = excluded.tried, fetched_at = excluded.fetched_at",
    )
    .bind(game_id)
    .bind(if sources.is_empty() { "not_found" } else { "found" })
    .bind(sources.join(","))
    .bind(&info.title)
    .bind(&info.description)
    .bind(&info.release_date)
    .bind(&info.developer)
    .bind(&info.publisher)
    .bind(&info.genre)
    .bind(&info.players)
    .bind(info.rating)
    .bind(&info.box_art_url)
    .bind(box_art_source)
    .bind(tried.join(","))
    .execute(&state.db)
    .await?;
    Ok(())
}

/// Downloads box art into the media directory; returns its file name.
async fn download_box_art(meta: &Metadata, game_id: i64, url: &str) -> Option<String> {
    let response = meta.http.get(url).send().await.ok()?.error_for_status().ok()?;
    let ext =
        match response.headers().get(reqwest::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()) {
            Some(t) if t.contains("jpeg") => "jpg",
            Some(t) if t.contains("webp") => "webp",
            _ => "png",
        };
    let bytes = response.bytes().await.ok()?;
    let name = format!("{game_id}-box.{ext}");
    tokio::fs::create_dir_all(&meta.media_dir).await.ok()?;
    tokio::fs::write(meta.media_dir.join(&name), &bytes).await.ok()?;
    Some(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asks_again_only_when_a_new_service_could_answer() {
        let all = ["fbneo", "screenscraper", "thegamesdb"];
        assert!(needs_lookup(None, None, &["fbneo"]), "never looked up");
        assert!(!needs_lookup(Some("found"), Some("fbneo"), &all), "found stays found");
        assert!(!needs_lookup(Some("not_found"), Some("fbneo"), &["fbneo"]), "nothing new");
        assert!(needs_lookup(Some("not_found"), Some("fbneo"), &all), "credentials added");
        assert!(needs_lookup(Some("not_found"), Some(""), &["fbneo"]), "before this was kept");
        assert!(!needs_lookup(Some("not_found"), Some("fbneo,screenscraper,thegamesdb"), &all));
    }
}
