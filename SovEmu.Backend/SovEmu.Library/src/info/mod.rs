//! Game information services: what each knows about a game ([`GameInfo`]) and the clients that
//! ask them. Every source is a [`Provider`]: it turns a game ([`LookupInput`]) into what its
//! service knows. The server's lookup job asks the configured providers in order, merges their
//! answers and stores them; it knows none of them by name, so a source can be replaced or added
//! without touching it.
//!
//! - [`fbneo`]: FinalBurn Neo's own DATs, for arcade and Neo Geo sets. No account needed.
//! - [`screenscraper`]: ScreenScraper, by MD5 + size + file name. Needs developer credentials.

pub mod fbneo;
pub mod screenscraper;

use std::collections::BTreeMap;
use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::time::Duration;

use md5::{Digest, Md5};

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

/// A future a [`Provider`] returns (boxed, so providers can sit in one list).
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// What a provider is doing for the game being looked up, for the job's progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Hashing the file (the job does this, for providers that match by hash).
    Hashing,
    /// Waiting for an answer.
    Asking,
    /// Waiting before asking: pacing between requests, or backing off a busy service.
    Waiting,
}

/// A service's own count of today's requests, as it reports it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Quota {
    pub requests_today: u64,
    pub max_requests_per_day: Option<u64>,
    /// Lookups today that found nothing, which some services cap on their own.
    pub misses_today: u64,
    pub max_misses_per_day: Option<u64>,
}

/// What a provider found: the information, and the service's own id for the game, which a
/// later lookup passes back ([`LookupInput::known_ids`]) to fetch it directly.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Found {
    pub info: GameInfo,
    pub id: Option<String>,
}

/// Reports a provider's [`Step`] while it looks a game up.
pub type Progress<'a> = &'a (dyn Fn(Step) + Send + Sync);

/// A game-information source.
pub trait Provider: Send + Sync + std::fmt::Debug {
    /// Stable id, stored with what it found (`fbneo`, `screenscraper`).
    fn id(&self) -> &'static str;
    /// Shown to admins.
    fn name(&self) -> &'static str;
    /// Whether it matches files by MD5: the job hashes a file only for a provider that does,
    /// and only when it does not know the game's id yet.
    fn wants_md5(&self) -> bool {
        false
    }
    /// What it knows about `input`: `Ok(None)` when it does not know the game.
    fn lookup<'a>(
        &'a self,
        input: &'a LookupInput,
        progress: Progress<'a>,
    ) -> BoxFuture<'a, Result<Option<Found>, MetaError>>;
    /// Today's quota as the service last reported it.
    fn quota(&self) -> Option<Quota> {
        None
    }
    /// The URL to download a box art URL it gave from (credentials added again).
    fn box_art_url(&self, stored: &str) -> String {
        stored.to_owned()
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
    /// The game's id at each provider that found it before, by provider id.
    pub known_ids: BTreeMap<String, String>,
}

#[derive(Debug, PartialEq)]
pub enum MetaError {
    /// Credentials rejected: stop using this source.
    Unauthorized(String),
    /// Out of requests for now: stop using this source for this job.
    Quota(String),
    /// The service is closed or overloaded: stop using it for this job, retry next time.
    Unavailable(String),
    /// Anything else; the game is retried next time.
    Other(String),
}

impl std::fmt::Display for MetaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unauthorized(m) => write!(f, "credentials rejected: {m}"),
            Self::Quota(m) => write!(f, "quota exhausted: {m}"),
            Self::Unavailable(m) => write!(f, "unavailable: {m}"),
            Self::Other(m) => f.write_str(m),
        }
    }
}

impl From<reqwest::Error> for MetaError {
    fn from(e: reqwest::Error) -> Self {
        Self::Other(e.to_string())
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

/// The HTTP client every source uses: a timeout, and a product name in the user agent.
pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent(concat!("SovEmu/", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("a TLS backend is compiled in")
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
    fn hashes_small_files() {
        let p = std::env::temp_dir().join(format!("sovemu-md5-{}", std::process::id()));
        std::fs::write(&p, b"abc").unwrap();
        assert_eq!(md5_file(&p).as_deref(), Some("900150983cd24fb0d6963f7d28e17f72"));
        std::fs::remove_file(p).ok();
    }
}
