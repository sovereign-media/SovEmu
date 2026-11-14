//! Game information from metadata services, fetched by a background job and kept in
//! `game_metadata`.
//!
//! Sources, tried in order and merged (an earlier source's fields win):
//! - [`fbneo`]: FinalBurn Neo's own DATs, for arcade and Neo Geo sets. No account needed; turns
//!   `mslug` into *Metal Slug - Super Vehicle-001* with its year and maker.
//! - [`screenscraper`]: ScreenScraper, the service retro frontends (Recalbox, Batocera, RetroBat,
//!   ES-DE) scrape from. Matches by MD5 + size + file name. Needs developer credentials; a user
//!   account raises the quota.
//! - [`thegamesdb`]: TheGamesDB, by name and platform. Needs an API key.

pub mod fbneo;
pub mod screenscraper;
pub mod thegamesdb;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use md5::{Digest, Md5};
use serde::Serialize;

use crate::AppState;

/// What a source knows about a game. Every field is optional; sources fill each other's gaps.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GameInfo {
    pub title: Option<String>,
    pub description: Option<String>,
    pub release_date: Option<String>,
    pub developer: Option<String>,
    pub publisher: Option<String>,
    pub genre: Option<String>,
    pub players: Option<String>,
    /// 0 to 1.
    pub rating: Option<f64>,
    /// Front box art.
    pub box_art_url: Option<String>,
}

impl GameInfo {
    /// Fills this one's missing fields from `other`.
    pub fn merge(&mut self, other: GameInfo) {
        macro_rules! fill {
            ($($f:ident),*) => { $( if self.$f.is_none() { self.$f = other.$f; } )* };
        }
        fill!(
            title,
            description,
            release_date,
            developer,
            publisher,
            genre,
            players,
            rating,
            box_art_url
        );
    }

    /// Whether the fields a game page shows are all present.
    pub fn complete(&self) -> bool {
        self.description.is_some() && self.release_date.is_some() && self.box_art_url.is_some()
    }
}

/// What a lookup knows about the file.
#[derive(Debug, Clone)]
pub struct LookupInput {
    /// SovEmu system id (`nes`, `psx`, …).
    pub system: String,
    /// The title parsed from the file name.
    pub title: String,
    /// The file name, extension included.
    pub file_name: String,
    pub size: u64,
    /// Lower-case hex MD5 of the file, when it was small enough to hash.
    pub md5: Option<String>,
}

#[derive(Debug)]
pub enum MetaError {
    /// Credentials rejected: stop using this source.
    Unauthorized(String),
    /// Out of requests for now: stop using this source for this job.
    Quota(String),
    /// Anything else; the game is retried next time.
    Other(String),
}

impl std::fmt::Display for MetaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unauthorized(m) => write!(f, "credentials rejected: {m}"),
            Self::Quota(m) => write!(f, "quota exhausted: {m}"),
            Self::Other(m) => f.write_str(m),
        }
    }
}

impl From<reqwest::Error> for MetaError {
    fn from(e: reqwest::Error) -> Self {
        Self::Other(e.to_string())
    }
}

/// Classifies an HTTP status from a metadata service.
pub fn status_error(service: &str, status: reqwest::StatusCode, body: &str) -> MetaError {
    let detail = format!("{service} HTTP {status}: {}", body.chars().take(160).collect::<String>());
    match status.as_u16() {
        401 | 403 => MetaError::Unauthorized(detail),
        // ScreenScraper uses 429-431 for its per-minute and daily quotas.
        429..=431 => MetaError::Quota(detail),
        _ => MetaError::Other(detail),
    }
}

/// Files hashed for MD5 matching at most this big (disc images are matched by name and size).
const MAX_HASH_BYTES: u64 = 64 * 1024 * 1024;

pub fn md5_file(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    if meta.len() > MAX_HASH_BYTES {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    Some(hex::encode(Md5::digest(&bytes)))
}

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
}

/// The configured sources and the job's shared state.
pub struct Metadata {
    pub fbneo: fbneo::FbNeoDats,
    pub screenscraper: Option<screenscraper::ScreenScraper>,
    pub thegamesdb: Option<thegamesdb::TheGamesDb>,
    pub media_dir: PathBuf,
    pub status: Mutex<JobStatus>,
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
        screenscraper: Option<screenscraper::ScreenScraper>,
        thegamesdb: Option<thegamesdb::TheGamesDb>,
        screenscraper_delay: Duration,
    ) -> Arc<Self> {
        let mut sources = vec!["fbneo"];
        if screenscraper.is_some() {
            sources.push("screenscraper");
        }
        if thegamesdb.is_some() {
            sources.push("thegamesdb");
        }
        Arc::new(Self {
            fbneo: fbneo::FbNeoDats::new(dir.join("fbneo")),
            screenscraper,
            thegamesdb,
            media_dir: dir.join("media"),
            status: Mutex::new(JobStatus { sources, ..JobStatus::default() }),
            http: http_client(),
            screenscraper_delay,
        })
    }

    pub fn status(&self) -> JobStatus {
        self.status.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn update(&self, f: impl FnOnce(&mut JobStatus)) {
        f(&mut self.status.lock().unwrap_or_else(|e| e.into_inner()));
    }
}

/// The HTTP client every source uses: a timeout, and a product name in the user agent.
pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent(concat!("SovEmu/", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("a TLS backend is compiled in")
}

#[derive(sqlx::FromRow)]
struct Pending {
    id: i64,
    system: String,
    path: String,
    title: String,
    size: i64,
}

/// Looks up every game without metadata (all of a system's, or every game with `force`), one
/// at a time, and stores what it finds. Returns at once if a job is already running.
pub async fn run_job(state: AppState, system: Option<String>, force: bool) {
    let meta = state.metadata.clone();
    {
        let mut s = meta.status.lock().unwrap_or_else(|e| e.into_inner());
        if s.running {
            return;
        }
        let sources = std::mem::take(&mut s.sources);
        *s = JobStatus { running: true, sources, ..JobStatus::default() };
    }
    let games = sqlx::query_as::<_, Pending>(
        "SELECT g.id, g.system, g.path, g.title, g.size FROM games g
         LEFT JOIN game_metadata m ON m.game_id = g.id
         WHERE (?1 IS NULL OR g.system = ?1) AND (?2 OR m.game_id IS NULL)
         ORDER BY g.system, g.title",
    )
    .bind(&system)
    .bind(force)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();
    meta.update(|s| s.total = games.len() as u64);
    tracing::info!("metadata: looking up {} game(s)", games.len());

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
        let mut failed = false;
        if let Some(found) = meta.fbneo.lookup(&meta.http, &input).await {
            info.merge(found);
            sources.push("fbneo");
        }
        if use_ss && !info.complete() {
            let ss = meta.screenscraper.as_ref().expect("checked");
            match ss.lookup(&input).await {
                Ok(Some(found)) => {
                    info.merge(found);
                    sources.push("screenscraper");
                }
                Ok(None) => {}
                Err(e @ (MetaError::Unauthorized(_) | MetaError::Quota(_))) => {
                    tracing::warn!("metadata: ScreenScraper stopped: {e}");
                    meta.update(|s| s.notes.push(format!("ScreenScraper: {e}")));
                    use_ss = false;
                    failed = true;
                }
                Err(e) => {
                    tracing::debug!("metadata: ScreenScraper {}: {e}", game.title);
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
                    info.merge(found);
                    sources.push("thegamesdb");
                }
                Ok(None) => {}
                Err(e @ (MetaError::Unauthorized(_) | MetaError::Quota(_))) => {
                    tracing::warn!("metadata: TheGamesDB stopped: {e}");
                    meta.update(|s| s.notes.push(format!("TheGamesDB: {e}")));
                    use_tgdb = false;
                    failed = true;
                }
                Err(e) => {
                    tracing::debug!("metadata: TheGamesDB {}: {e}", game.title);
                    failed = true;
                }
            }
        }

        let found = !sources.is_empty();
        if found || !failed {
            let box_art = match &info.box_art_url {
                Some(url) => download_box_art(&meta, game.id, url).await,
                None => None,
            };
            let _ = store(&state, game.id, &info, &sources, box_art.as_deref()).await;
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
    meta.update(|s| s.running = false);
    let s = meta.status();
    tracing::info!(
        "metadata: done ({} found, {} not found, {} failed)",
        s.found,
        s.not_found,
        s.failed
    );
}

async fn store(
    state: &AppState,
    game_id: i64,
    info: &GameInfo,
    sources: &[&str],
    box_art: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO game_metadata (game_id, status, sources, title, description, release_date,
           developer, publisher, genre, players, rating, box_art, fetched_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, datetime('now'))
         ON CONFLICT(game_id) DO UPDATE SET status = excluded.status, sources = excluded.sources,
           title = excluded.title, description = excluded.description,
           release_date = excluded.release_date, developer = excluded.developer,
           publisher = excluded.publisher, genre = excluded.genre, players = excluded.players,
           rating = excluded.rating, box_art = COALESCE(excluded.box_art, game_metadata.box_art),
           fetched_at = excluded.fetched_at",
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
    .bind(box_art)
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

/// Lower-case alphanumerics only, for comparing titles across sources.
pub fn normalize(title: &str) -> String {
    title.to_lowercase().replace(['&'], "and").chars().filter(|c| c.is_alphanumeric()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_keeps_earlier_fields() {
        let mut a = GameInfo { title: Some("Metal Slug".into()), ..GameInfo::default() };
        a.merge(GameInfo {
            title: Some("METAL SLUG".into()),
            description: Some("Run and gun.".into()),
            ..GameInfo::default()
        });
        assert_eq!(a.title.as_deref(), Some("Metal Slug"));
        assert_eq!(a.description.as_deref(), Some("Run and gun."));
        assert!(!a.complete());
    }

    #[test]
    fn normalizes_titles() {
        assert_eq!(
            normalize("The Legend of Zelda: A Link to the Past"),
            "thelegendofzeldaalinktothepast"
        );
        assert_eq!(normalize("Sonic & Knuckles"), "sonicandknuckles");
    }

    #[test]
    fn classifies_service_errors() {
        use reqwest::StatusCode;
        assert!(matches!(
            status_error("x", StatusCode::UNAUTHORIZED, ""),
            MetaError::Unauthorized(_)
        ));
        assert!(matches!(
            status_error("x", StatusCode::from_u16(430).unwrap(), ""),
            MetaError::Quota(_)
        ));
        assert!(matches!(status_error("x", StatusCode::BAD_GATEWAY, ""), MetaError::Other(_)));
    }

    #[test]
    fn hashes_small_files() {
        let p = std::env::temp_dir().join(format!("sovemu-md5-{}", std::process::id()));
        std::fs::write(&p, b"abc").unwrap();
        assert_eq!(md5_file(&p).as_deref(), Some("900150983cd24fb0d6963f7d28e17f72"));
        std::fs::remove_file(p).ok();
    }
}
