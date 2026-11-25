//! Game information from metadata services, fetched by a background job and kept in
//! `game_metadata`.
//!
//! The sources are [`Provider`]s, asked in the order `SOVEMU_METADATA_SOURCES` gives
//! (`fbneo,screenscraper` by default): FinalBurn Neo's own DATs name arcade and Neo Geo sets,
//! ScreenScraper matches every other game by MD5, size and file name. An earlier source's field
//! wins, and a game is asked about until every field a game page shows is filled
//! ([`GameInfo::complete`]). The job names no provider: a source can be swapped for another in
//! `sovemu-library` without touching it.
//!
//! A lookup stores what it finds at once, box art included as a URL; [`artwork`] downloads the
//! box art afterwards, in the background, for the games that will show it.
//!
//! What a lookup stores is merged into what the game had ([`merge_row`]): only the fields the
//! job was asked to refresh ([`Scope::fields`]) are replaced, a source that finds nothing never
//! clears one, and each field remembers which source filled it (`field_sources`). Each source's
//! own id for the game is kept too (`provider_ids`), so a later lookup asks for that game rather
//! than matching the file again.

pub(crate) mod artwork;

use sovemu_library::info::{
    Found, GameInfo, LookupInput, MetaError, Provider, Quota, Step, http_client, md5_file,
};

use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crate::AppState;

/// A configured provider, shared by the lookup and box art jobs.
pub(crate) type SharedProvider = Arc<dyn Provider>;

/// The games a status lists as just looked up.
const RECENT: usize = 20;

/// The game being looked up now.
#[derive(Debug, Clone)]
pub struct Current {
    pub game_id: i64,
    pub title: String,
    pub system: String,
    /// The provider being asked, if one is.
    pub provider: Option<&'static str>,
    pub step: Step,
}

/// A system's share of a job.
#[derive(Debug, Clone, Default)]
pub struct SystemProgress {
    pub system: String,
    pub total: u64,
    pub done: u64,
}

/// What one provider answered in a job.
#[derive(Debug, Clone)]
pub struct ProviderProgress {
    pub id: &'static str,
    pub name: &'static str,
    pub asked: u64,
    pub found: u64,
    pub not_found: u64,
    pub errors: u64,
    /// Today's quota, as the service last reported it.
    pub quota: Option<Quota>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Found,
    NotFound,
    /// Not stored: asked again next time.
    Failed,
}

/// A game the job has just looked up.
#[derive(Debug, Clone)]
pub struct Recent {
    pub game_id: i64,
    /// Its title after the lookup.
    pub title: String,
    pub system: String,
    pub outcome: Outcome,
    /// The providers that found something.
    pub sources: Vec<&'static str>,
    /// Why it failed, if it did.
    pub detail: Option<String>,
}

/// Progress of the current (or last) metadata job.
#[derive(Debug, Clone, Default)]
pub struct JobStatus {
    pub running: bool,
    pub total: u64,
    pub done: u64,
    pub found: u64,
    pub not_found: u64,
    pub failed: u64,
    /// Which sources are configured, in order.
    pub sources: Vec<&'static str>,
    /// Why a source stopped early (bad credentials, quota) or is not used, if one did.
    pub notes: Vec<String>,
    /// The box art downloads that follow a lookup.
    pub artwork: artwork::ArtworkStatus,
    pub current: Option<Current>,
    /// Unix times.
    pub started_at: Option<f64>,
    pub finished_at: Option<f64>,
    /// Seconds left, from the time per game so far.
    pub eta_seconds: Option<f64>,
    pub systems: Vec<SystemProgress>,
    pub providers: Vec<ProviderProgress>,
    /// The last games looked up, most recent first.
    pub recent: VecDeque<Recent>,
}

/// A field of game information, for refreshing only some ([`Scope::fields`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Field {
    Title,
    Description,
    ReleaseDate,
    Developer,
    Publisher,
    Genre,
    Players,
    Rating,
    BoxArt,
}

impl Field {
    /// Its key in `field_sources`.
    pub(crate) fn key(self) -> &'static str {
        match self {
            Field::Title => "title",
            Field::Description => "description",
            Field::ReleaseDate => "release_date",
            Field::Developer => "developer",
            Field::Publisher => "publisher",
            Field::Genre => "genre",
            Field::Players => "players",
            Field::Rating => "rating",
            Field::BoxArt => "box_art",
        }
    }
}

/// The configured sources and the job's shared state.
pub struct Metadata {
    /// In the order they are asked.
    pub providers: Vec<Arc<dyn Provider>>,
    /// Why a configured source is not in `providers` (no credentials), shown with each job.
    pub setup_notes: Vec<String>,
    pub media_dir: PathBuf,
    pub status: Mutex<JobStatus>,
    pub artwork: Mutex<artwork::ArtworkStatus>,
    /// Box art downloads at once.
    pub artwork_downloads: usize,
    http: reqwest::Client,
    /// When the running job started, for its ETA.
    started: Mutex<Option<Instant>>,
}

impl std::fmt::Debug for Metadata {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Metadata").finish_non_exhaustive()
    }
}

impl Metadata {
    pub fn new(
        paths: &crate::paths::DataPaths,
        providers: Vec<Arc<dyn Provider>>,
        setup_notes: Vec<String>,
        artwork_downloads: usize,
    ) -> Arc<Self> {
        let sources = providers.iter().map(|p| p.id()).collect();
        Arc::new(Self {
            providers,
            status: Mutex::new(JobStatus {
                sources,
                notes: setup_notes.clone(),
                ..JobStatus::default()
            }),
            setup_notes,
            media_dir: paths.artwork(),
            artwork: Mutex::new(artwork::ArtworkStatus::default()),
            artwork_downloads,
            http: http_client(),
            started: Mutex::new(None),
        })
    }

    pub fn status(&self) -> JobStatus {
        let mut status = self.status.lock().unwrap_or_else(|e| e.into_inner()).clone();
        status.artwork = self.artwork_status();
        let started = *self.started.lock().unwrap_or_else(|e| e.into_inner());
        status.eta_seconds = match started {
            Some(t) if status.running && status.done > 0 && status.total >= status.done => {
                let per_game = t.elapsed().as_secs_f64() / status.done as f64;
                Some(per_game * (status.total - status.done) as f64)
            }
            _ => None,
        };
        for p in &mut status.providers {
            if let Some(provider) = self.providers.iter().find(|x| x.id() == p.id) {
                p.quota = provider.quota();
            }
        }
        status
    }

    pub fn artwork_status(&self) -> artwork::ArtworkStatus {
        self.artwork.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// The provider with this id, if it is configured.
    pub fn provider(&self, id: &str) -> Option<&Arc<dyn Provider>> {
        self.providers.iter().find(|p| p.id() == id)
    }

    fn update(&self, f: impl FnOnce(&mut JobStatus)) {
        f(&mut self.status.lock().unwrap_or_else(|e| e.into_inner()));
    }
}

fn now_unix() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
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
    /// Last looked up longer ago than [`Scope::stale_after_days`].
    stale: bool,
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

/// Which games a lookup covers, and what it may change.
#[derive(Debug, Clone, Default)]
pub(crate) struct Scope {
    /// One system's, or every system's.
    pub(crate) system: Option<String>,
    /// One game only (the admin's "look it up again").
    pub(crate) game: Option<i64>,
    /// Games that have information already, too.
    pub(crate) force: bool,
    /// Only these fields are replaced (empty: all of them); the rest are kept as they are.
    pub(crate) fields: Vec<Field>,
    /// Only these providers are asked (empty: every configured one).
    pub(crate) providers: Vec<String>,
    /// Games last looked up more than this many days ago, too.
    pub(crate) stale_after_days: Option<u32>,
}

impl Scope {
    /// The lookup that follows a scan or a schedule: games without information, and games
    /// whose information is older than `SOVEMU_METADATA_REFRESH_DAYS` (when set).
    pub(crate) fn automatic(config: &crate::Config) -> Self {
        Self {
            stale_after_days: (config.metadata_refresh_days > 0)
                .then_some(config.metadata_refresh_days),
            ..Self::default()
        }
    }
}

/// What a game has stored, read before a lookup so the lookup merges into it.
#[derive(Debug, Clone, Default, PartialEq, sqlx::FromRow)]
pub(crate) struct Row {
    pub(crate) title: Option<String>,
    pub(crate) description: Option<String>,
    pub(crate) release_date: Option<String>,
    pub(crate) developer: Option<String>,
    pub(crate) publisher: Option<String>,
    pub(crate) genre: Option<String>,
    pub(crate) players: Option<String>,
    pub(crate) rating: Option<f64>,
    pub(crate) box_art_url: Option<String>,
    pub(crate) box_art_source: Option<String>,
    /// JSON: field key → provider id.
    pub(crate) field_sources: String,
    /// JSON: provider id → its id for the game.
    pub(crate) provider_ids: String,
    pub(crate) tried: String,
}

impl Row {
    fn sources(&self) -> BTreeMap<String, String> {
        serde_json::from_str(&self.field_sources).unwrap_or_default()
    }

    fn ids(&self) -> BTreeMap<String, String> {
        serde_json::from_str(&self.provider_ids).unwrap_or_default()
    }
}

/// What one lookup found: the fields, the provider that filled each, and the providers' ids.
#[derive(Debug, Default)]
pub(crate) struct Lookup {
    pub(crate) info: GameInfo,
    pub(crate) field_sources: BTreeMap<&'static str, &'static str>,
    pub(crate) ids: BTreeMap<&'static str, String>,
    /// The providers that answered, found or not.
    pub(crate) tried: Vec<&'static str>,
}

impl Lookup {
    /// Adds what `provider` found: the fields still empty are its.
    fn add(&mut self, provider: &'static str, found: Found) {
        let before = self.info.clone();
        self.info.merge(found.info);
        let filled = |a: bool, b: bool| !a && b;
        let i = &self.info;
        for (field, was, now) in [
            (Field::Title, before.title.is_some(), i.title.is_some()),
            (Field::Description, before.description.is_some(), i.description.is_some()),
            (Field::ReleaseDate, before.release_date.is_some(), i.release_date.is_some()),
            (Field::Developer, before.developer.is_some(), i.developer.is_some()),
            (Field::Publisher, before.publisher.is_some(), i.publisher.is_some()),
            (Field::Genre, before.genre.is_some(), i.genre.is_some()),
            (Field::Players, before.players.is_some(), i.players.is_some()),
            (Field::Rating, before.rating.is_some(), i.rating.is_some()),
            (Field::BoxArt, before.box_art_url.is_some(), i.box_art_url.is_some()),
        ] {
            if filled(was, now) {
                self.field_sources.insert(field.key(), provider);
            }
        }
        if let Some(id) = found.id {
            self.ids.insert(provider, id);
        }
    }
}

/// `old` with what `new` found merged in: a field is replaced only if `fields` allows it (empty:
/// every field) and `new` has a value for it, so a lookup never clears what a game had. The
/// providers asked and the providers' ids are added to the old ones.
pub(crate) fn merge_row(old: &Row, new: &Lookup, fields: &[Field]) -> Row {
    let wanted = |f: Field| fields.is_empty() || fields.contains(&f);
    let mut sources = old.sources();
    let mut row = old.clone();
    macro_rules! take {
        ($field:expr, $name:ident, $from:expr) => {
            if wanted($field) && $from.is_some() {
                row.$name = $from.clone();
                if let Some(p) = new.field_sources.get($field.key()) {
                    sources.insert($field.key().to_owned(), (*p).to_owned());
                }
            }
        };
    }
    let i = &new.info;
    take!(Field::Title, title, i.title);
    take!(Field::Description, description, i.description);
    take!(Field::ReleaseDate, release_date, i.release_date);
    take!(Field::Developer, developer, i.developer);
    take!(Field::Publisher, publisher, i.publisher);
    take!(Field::Genre, genre, i.genre);
    take!(Field::Players, players, i.players);
    take!(Field::Rating, rating, i.rating);
    take!(Field::BoxArt, box_art_url, i.box_art_url);
    row.box_art_source = row.box_art_url.as_ref().and_then(|_| sources.get("box_art").cloned());
    row.field_sources = serde_json::to_string(&sources).unwrap_or_else(|_| "{}".into());
    let mut ids = old.ids();
    ids.extend(new.ids.iter().map(|(k, v)| ((*k).to_owned(), v.clone())));
    row.provider_ids = serde_json::to_string(&ids).unwrap_or_else(|_| "{}".into());
    let mut tried: Vec<String> =
        old.tried.split(',').filter(|t| !t.is_empty()).map(str::to_owned).collect();
    for t in &new.tried {
        if !tried.iter().any(|x| x == t) {
            tried.push((*t).to_owned());
        }
    }
    row.tried = tried.join(",");
    row
}

/// Looks up the games `scope` covers, one at a time, asking the configured providers in order,
/// and stores what it finds. By default those are the games without information, and the games
/// no service found while a service they were not asked is now configured. Returns at once if
/// a job is already running.
pub(crate) async fn run_job(state: AppState, scope: Scope) {
    let meta = state.metadata.clone();
    {
        let mut s = meta.status.lock().unwrap_or_else(|e| e.into_inner());
        if s.running {
            return;
        }
        let sources = std::mem::take(&mut s.sources);
        *s = JobStatus {
            running: true,
            sources,
            notes: meta.setup_notes.clone(),
            started_at: Some(now_unix()),
            ..JobStatus::default()
        };
    }
    *meta.started.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
    // Clears `running` however the job ends: a panic (which `Jobs` records as a failed run)
    // would otherwise leave every later lookup returning at once.
    let _running = scopeguard(meta.clone(), |m| {
        m.update(|s| {
            s.running = false;
            s.current = None;
        })
    });
    let providers: Vec<Arc<dyn Provider>> = meta
        .providers
        .iter()
        .filter(|p| scope.providers.is_empty() || scope.providers.iter().any(|id| id == p.id()))
        .cloned()
        .collect();
    let available: Vec<&str> = providers.iter().map(|p| p.id()).collect();
    meta.update(|s| {
        s.providers = providers
            .iter()
            .map(|p| ProviderProgress {
                id: p.id(),
                name: p.name(),
                asked: 0,
                found: 0,
                not_found: 0,
                errors: 0,
                quota: None,
            })
            .collect()
    });

    let stale = scope.stale_after_days.map(|d| format!("-{d} days"));
    let games: Vec<Pending> = sqlx::query_as::<_, Pending>(
        "SELECT g.id, g.system, g.path, g.title, g.size, m.status, m.tried,
           (?4 IS NOT NULL AND m.fetched_at < datetime('now', ?4)) AS stale
         FROM games g
         LEFT JOIN game_metadata m ON m.game_id = g.id
         WHERE (?1 IS NULL OR g.system = ?1)
           AND (?3 IS NULL OR g.id = ?3)
         ORDER BY g.system, g.title",
    )
    .bind(&scope.system)
    .bind(scope.force)
    .bind(scope.game)
    .bind(&stale)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default()
    .into_iter()
    .filter(|g| {
        scope.force || g.stale || needs_lookup(g.status.as_deref(), g.tried.as_deref(), &available)
    })
    .collect();
    let mut systems: Vec<SystemProgress> = Vec::new();
    for g in &games {
        match systems.last_mut() {
            Some(s) if s.system == g.system => s.total += 1,
            _ => systems.push(SystemProgress { system: g.system.clone(), total: 1, done: 0 }),
        }
    }
    meta.update(|s| {
        s.total = games.len() as u64;
        s.systems = systems;
    });
    tracing::info!(count = games.len(), sources = ?available, "metadata lookup started");

    // Providers that stopped (credentials, quota, closed) are left alone for the rest of the job.
    let mut stopped: Vec<&'static str> = Vec::new();
    for game in games {
        let old: Row = sqlx::query_as(
            "SELECT title, description, release_date, developer, publisher, genre, players,
               rating, box_art_url, box_art_source, field_sources, provider_ids, tried
             FROM game_metadata WHERE game_id = ?",
        )
        .bind(game.id)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| Row {
            field_sources: "{}".into(),
            provider_ids: "{}".into(),
            ..Row::default()
        });
        meta.update(|s| {
            s.current = Some(Current {
                game_id: game.id,
                title: game.title.clone(),
                system: game.system.clone(),
                provider: None,
                step: Step::Hashing,
            })
        });
        let known_ids = old.ids();
        let path = state.config.roms_dir.join(&game.path);
        // Hashed only for a provider that matches by hash and does not know the game yet.
        let hash = providers.iter().any(|p| {
            p.wants_md5() && !stopped.contains(&p.id()) && !known_ids.contains_key(p.id())
        });
        let input = LookupInput {
            system: game.system.clone(),
            title: game.title.clone(),
            file_name: Path::new(&game.path)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            size: game.size as u64,
            md5: if hash {
                tokio::task::spawn_blocking(move || md5_file(&path)).await.ok().flatten()
            } else {
                None
            },
            known_ids,
        };

        let mut found = Lookup::default();
        let mut failed = None;
        for provider in &providers {
            let id = provider.id();
            if stopped.contains(&id) || found.info.complete() {
                continue;
            }
            meta.update(|s| {
                if let Some(c) = &mut s.current {
                    c.provider = Some(id);
                    c.step = Step::Asking;
                }
            });
            let progress_meta = meta.clone();
            let progress = move |step: Step| {
                progress_meta.update(|s| {
                    if let Some(c) = &mut s.current {
                        c.step = step;
                    }
                })
            };
            let answer = provider.lookup(&input, &progress).await;
            let count = |f: fn(&mut ProviderProgress)| {
                meta.update(|s| {
                    if let Some(p) = s.providers.iter_mut().find(|p| p.id == id) {
                        p.asked += 1;
                        f(p);
                    }
                })
            };
            match answer {
                Ok(Some(answer)) => {
                    count(|p| p.found += 1);
                    found.add(id, answer);
                    found.tried.push(id);
                }
                Ok(None) => {
                    count(|p| p.not_found += 1);
                    found.tried.push(id);
                }
                Err(
                    e @ (MetaError::Unauthorized(_)
                    | MetaError::Quota(_)
                    | MetaError::Unavailable(_)),
                ) => {
                    count(|p| p.errors += 1);
                    tracing::warn!(provider = id, error = %e, "metadata: provider stopped");
                    let note = format!("{}: {e}", provider.name());
                    meta.update(|s| s.notes.push(note));
                    stopped.push(id);
                    failed = Some(e.to_string());
                }
                Err(e) => {
                    count(|p| p.errors += 1);
                    tracing::debug!(game = %game.title, provider = id, error = %e, "metadata: lookup failed");
                    failed = Some(e.to_string());
                }
            }
        }

        let any = !found.field_sources.is_empty();
        let outcome = if any {
            Outcome::Found
        } else if failed.is_some() {
            Outcome::Failed
        } else {
            Outcome::NotFound
        };
        // A lookup that failed and found nothing is not stored, so the next job asks again.
        if outcome != Outcome::Failed {
            let row = merge_row(&old, &found, &scope.fields);
            if let Err(e) = store(&state, game.id, &row, &meta.providers).await {
                tracing::warn!(game = %game.title, error = %e, "metadata: cannot store");
            }
        }
        let title = found.info.title.clone().or(old.title.clone()).unwrap_or(game.title.clone());
        let sources: Vec<&'static str> = providers
            .iter()
            .map(|p| p.id())
            .filter(|id| found.field_sources.values().any(|s| s == id))
            .collect();
        meta.update(|s| {
            s.done += 1;
            match outcome {
                Outcome::Found => s.found += 1,
                Outcome::NotFound => s.not_found += 1,
                Outcome::Failed => s.failed += 1,
            }
            if let Some(sys) = s.systems.iter_mut().find(|x| x.system == game.system) {
                sys.done += 1;
            }
            s.recent.push_front(Recent {
                game_id: game.id,
                title,
                system: game.system.clone(),
                outcome,
                sources,
                detail: if outcome == Outcome::Failed { failed } else { None },
            });
            s.recent.truncate(RECENT);
        });
    }
    // Game information renames games: which share a title may have changed.
    if let Err(e) = crate::releases::refresh(&state.db).await {
        tracing::warn!(error = %e, "metadata: updating release labels");
    }
    // Claimed before this job reads as finished, so no one sees both idle in between.
    let artwork = artwork::claim(&meta);
    meta.update(|s| {
        s.running = false;
        s.current = None;
        s.finished_at = Some(now_unix());
    });
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

/// Writes `row` for `game_id`. A new box art URL is downloaded afresh: the old file is dropped
/// and its attempts start again at 0.
async fn store(
    state: &AppState,
    game_id: i64,
    row: &Row,
    providers: &[Arc<dyn Provider>],
) -> Result<(), sqlx::Error> {
    let sources_map = row.sources();
    let sources: Vec<&str> = providers
        .iter()
        .map(|p| p.id())
        .filter(|id| sources_map.values().any(|s| s == id))
        .collect();
    sqlx::query(
        "INSERT INTO game_metadata (game_id, status, sources, title, description, release_date,
           developer, publisher, genre, players, rating, box_art_url, box_art_source, tried,
           field_sources, provider_ids, fetched_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, datetime('now'))
         ON CONFLICT(game_id) DO UPDATE SET status = excluded.status, sources = excluded.sources,
           title = excluded.title, description = excluded.description,
           release_date = excluded.release_date, developer = excluded.developer,
           publisher = excluded.publisher, genre = excluded.genre, players = excluded.players,
           rating = excluded.rating,
           box_art = CASE WHEN excluded.box_art_url IS game_metadata.box_art_url
             THEN game_metadata.box_art ELSE NULL END,
           box_art_attempts = CASE WHEN excluded.box_art_url IS game_metadata.box_art_url
             THEN game_metadata.box_art_attempts ELSE 0 END,
           box_art_url = excluded.box_art_url, box_art_source = excluded.box_art_source,
           tried = excluded.tried, field_sources = excluded.field_sources,
           provider_ids = excluded.provider_ids, fetched_at = excluded.fetched_at",
    )
    .bind(game_id)
    .bind(if sources_map.is_empty() { "not_found" } else { "found" })
    .bind(sources.join(","))
    .bind(&row.title)
    .bind(&row.description)
    .bind(&row.release_date)
    .bind(&row.developer)
    .bind(&row.publisher)
    .bind(&row.genre)
    .bind(&row.players)
    .bind(row.rating)
    .bind(&row.box_art_url)
    .bind(&row.box_art_source)
    .bind(&row.tried)
    .bind(&row.field_sources)
    .bind(&row.provider_ids)
    .execute(&state.db)
    .await?;
    Ok(())
}

/// Removes what earlier versions left: LaunchBox's database dump (no longer a source), and
/// downloaded box art no game refers to (all of it after the lookup data was cleared, in
/// migration 0030). Box art is only ever `<game id>-box.<png|jpg|webp>` in the media folder;
/// nothing else there, and nothing in the library, is touched.
pub(crate) async fn tidy(paths: &crate::paths::DataPaths, db: &sqlx::SqlitePool) {
    let old = paths.removed_launchbox();
    if old.exists() {
        match tokio::fs::remove_dir_all(&old).await {
            Ok(()) => tracing::info!(path = %old.display(), "removed the LaunchBox database dump"),
            Err(e) => {
                tracing::warn!(path = %old.display(), error = %e, "cannot remove the LaunchBox dump")
            }
        }
    }
    let Ok(kept) = sqlx::query_scalar::<_, String>(
        "SELECT box_art FROM game_metadata WHERE box_art IS NOT NULL",
    )
    .fetch_all(db)
    .await
    else {
        return;
    };
    let Ok(mut dir) = tokio::fs::read_dir(paths.artwork()).await else { return };
    let mut removed = 0u64;
    while let Ok(Some(entry)) = dir.next_entry().await {
        let name = entry.file_name().to_string_lossy().into_owned();
        if is_box_art_file(&name)
            && !kept.contains(&name)
            && tokio::fs::remove_file(entry.path()).await.is_ok()
        {
            removed += 1;
        }
    }
    if removed > 0 {
        tracing::info!(files = removed, "removed box art no game refers to");
    }
}

/// Whether `name` is a downloaded box art file (`<game id>-box.<ext>`).
fn is_box_art_file(name: &str) -> bool {
    let Some((id, ext)) = name.split_once("-box.") else { return false };
    !id.is_empty()
        && id.bytes().all(|b| b.is_ascii_digit())
        && ["png", "jpg", "webp"].contains(&ext)
}

/// Box art larger than this is not an image anyone meant to send: refused rather than kept.
const MAX_BOX_ART_BYTES: usize = 32 << 20;

/// The image format of `bytes`, from its first bytes (PNG, JPEG or WebP); none for anything
/// else (an HTML error page sent with a 200, say).
fn image_extension(bytes: &[u8]) -> Option<&'static str> {
    match bytes {
        [0x89, b'P', b'N', b'G', ..] => Some("png"),
        [0xFF, 0xD8, 0xFF, ..] => Some("jpg"),
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => Some("webp"),
        _ => None,
    }
}

/// Downloads box art into the media directory; returns its file name. Only an image, of a sane
/// size, written whole (a temp file, then a rename); a copy kept under another format goes.
async fn download_box_art(meta: &Metadata, game_id: i64, url: &str) -> Option<String> {
    let mut response = meta.http.get(url).send().await.ok()?.error_for_status().ok()?;
    if response.content_length().is_some_and(|n| n > MAX_BOX_ART_BYTES as u64) {
        return None;
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.ok()? {
        if bytes.len() + chunk.len() > MAX_BOX_ART_BYTES {
            return None;
        }
        bytes.extend_from_slice(&chunk);
    }
    let ext = image_extension(&bytes)?;
    let name = format!("{game_id}-box.{ext}");
    tokio::fs::create_dir_all(&meta.media_dir).await.ok()?;
    let temp = meta.media_dir.join(format!("{name}.part"));
    tokio::fs::write(&temp, &bytes).await.ok()?;
    tokio::fs::rename(&temp, meta.media_dir.join(&name)).await.ok()?;
    for other in ["png", "jpg", "webp"].into_iter().filter(|e| *e != ext) {
        let _ = tokio::fs::remove_file(meta.media_dir.join(format!("{game_id}-box.{other}"))).await;
    }
    Some(name)
}

/// Runs `f(value)` when dropped: on return, `?`, or a panic unwinding.
pub(crate) fn scopeguard<T, F: FnOnce(&T)>(value: T, f: F) -> impl Drop {
    struct Guard<T, F: FnOnce(&T)>(T, Option<F>);
    impl<T, F: FnOnce(&T)> Drop for Guard<T, F> {
        fn drop(&mut self) {
            if let Some(f) = self.1.take() {
                f(&self.0);
            }
        }
    }
    Guard(value, Some(f))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn box_art_is_told_by_its_bytes() {
        assert_eq!(image_extension(b"\x89PNG\r\n\x1a\n...."), Some("png"));
        assert_eq!(image_extension(&[0xFF, 0xD8, 0xFF, 0xE0, 0, 0]), Some("jpg"));
        assert_eq!(image_extension(b"RIFF\0\0\0\0WEBPVP8 "), Some("webp"));
        assert_eq!(image_extension(b"<!DOCTYPE html><html>"), None, "an error page");
        assert_eq!(image_extension(b""), None);
    }

    #[test]
    fn a_scope_guard_runs_on_panic_too() {
        use std::sync::atomic::{AtomicBool, Ordering};
        static RAN: AtomicBool = AtomicBool::new(false);
        let unwound = std::panic::catch_unwind(|| {
            let _g = scopeguard((), |_| RAN.store(true, Ordering::SeqCst));
            panic!("a job that panics");
        });
        assert!(unwound.is_err());
        assert!(RAN.load(Ordering::SeqCst));
    }

    fn found(title: &str, description: Option<&str>, id: Option<&str>) -> Found {
        Found {
            info: GameInfo {
                title: Some(title.into()),
                description: description.map(Into::into),
                ..GameInfo::default()
            },
            id: id.map(Into::into),
        }
    }

    fn empty_row() -> Row {
        Row { field_sources: "{}".into(), provider_ids: "{}".into(), ..Row::default() }
    }

    #[test]
    fn a_lookup_remembers_which_source_filled_each_field() {
        let mut l = Lookup::default();
        l.add("fbneo", found("Metal Slug", None, None));
        l.add("screenscraper", found("METAL SLUG", Some("Run and gun."), Some("42")));
        assert_eq!(l.info.title.as_deref(), Some("Metal Slug"), "the earlier source wins");
        assert_eq!(l.field_sources["title"], "fbneo");
        assert_eq!(l.field_sources["description"], "screenscraper");
        assert_eq!(l.ids["screenscraper"], "42");
        assert!(!l.ids.contains_key("fbneo"));
    }

    #[test]
    fn merging_never_clears_and_keeps_fields_not_asked_for() {
        let mut first = Lookup::default();
        first.add("screenscraper", found("Castlevania", Some("Old text."), Some("1234")));
        first.tried.push("screenscraper");
        let row = merge_row(&empty_row(), &first, &[]);
        assert_eq!(row.title.as_deref(), Some("Castlevania"));
        assert_eq!(row.tried, "screenscraper");
        assert!(row.provider_ids.contains("1234"));

        // A refresh of the description only: the new title is not taken.
        let mut second = Lookup::default();
        second.add("screenscraper", found("CASTLEVANIA", Some("New text."), None));
        let row = merge_row(&row, &second, &[Field::Description]);
        assert_eq!(row.title.as_deref(), Some("Castlevania"));
        assert_eq!(row.description.as_deref(), Some("New text."));
        assert!(row.provider_ids.contains("1234"), "ids are kept");

        // A source that finds nothing changes nothing.
        let row2 = merge_row(&row, &Lookup::default(), &[]);
        assert_eq!(row2, row);
    }

    #[test]
    fn box_art_files_are_told_by_name() {
        assert!(is_box_art_file("12-box.png"));
        assert!(is_box_art_file("3-box.webp"));
        assert!(!is_box_art_file("12-box.png.part"));
        assert!(!is_box_art_file("thumbs"));
        assert!(!is_box_art_file("x-box.png"));
        assert!(!is_box_art_file("-box.jpg"));
    }

    #[test]
    fn asks_again_only_when_a_new_service_could_answer() {
        let all = ["fbneo", "screenscraper"];
        assert!(needs_lookup(None, None, &["fbneo"]), "never looked up");
        assert!(!needs_lookup(Some("found"), Some("fbneo"), &all), "found stays found");
        assert!(!needs_lookup(Some("not_found"), Some("fbneo"), &["fbneo"]), "nothing new");
        assert!(needs_lookup(Some("not_found"), Some("fbneo"), &all), "credentials added");
        assert!(needs_lookup(Some("not_found"), Some(""), &["fbneo"]), "before this was kept");
        assert!(!needs_lookup(Some("not_found"), Some("fbneo,screenscraper"), &all));
    }
}
