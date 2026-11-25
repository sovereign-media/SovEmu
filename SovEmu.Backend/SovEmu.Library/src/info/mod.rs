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
    /// The ESRB's age rating, as [`Esrb::code`] gives it (`"E10+"`).
    pub esrb: Option<String>,
    /// Ways to play ("Single player, Multiplayer (simultaneous)"), comma-separated.
    pub modes: Option<String>,
    /// The series and families it belongs to ("Castlevania"), comma-separated.
    pub series: Option<String>,
    /// Its themes ("Horror, Fantasy"), comma-separated.
    pub themes: Option<String>,
    /// Front box art.
    pub box_art_url: Option<String>,
    /// A screenshot of the game, shown behind it (the `image` artwork).
    pub screenshot_url: Option<String>,
    /// Its logo (the `marquee` artwork).
    pub marquee_url: Option<String>,
    /// A gameplay video (MP4).
    pub video_url: Option<String>,
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
            esrb,
            modes,
            series,
            themes,
            box_art_url,
            screenshot_url,
            marquee_url,
            video_url
        );
    }

    /// Whether the fields a game page shows are all present.
    pub fn complete(&self) -> bool {
        self.description.is_some() && self.release_date.is_some() && self.box_art_url.is_some()
    }
}

/// The ESRB's age ratings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Esrb {
    EarlyChildhood,
    Everyone,
    Everyone10Plus,
    Teen,
    Mature,
    AdultsOnly,
    RatingPending,
}

impl Esrb {
    pub const ALL: [Esrb; 7] = [
        Esrb::EarlyChildhood,
        Esrb::Everyone,
        Esrb::Everyone10Plus,
        Esrb::Teen,
        Esrb::Mature,
        Esrb::AdultsOnly,
        Esrb::RatingPending,
    ];

    /// As the ESRB writes it, and as it is stored: `EC`, `E`, `E10+`, `T`, `M`, `AO`, `RP`.
    pub fn code(self) -> &'static str {
        match self {
            Esrb::EarlyChildhood => "EC",
            Esrb::Everyone => "E",
            Esrb::Everyone10Plus => "E10+",
            Esrb::Teen => "T",
            Esrb::Mature => "M",
            Esrb::AdultsOnly => "AO",
            Esrb::RatingPending => "RP",
        }
    }

    /// A rating as a source writes it: the code in any case, with or without "ESRB" before it
    /// ("ESRB - T", "e10"), and "K-A" (Kids to Adults, the ESRB's name for E until 1998).
    pub fn parse(text: &str) -> Option<Esrb> {
        let upper = text.trim().to_ascii_uppercase();
        let code: String = upper
            .strip_prefix("ESRB")
            .unwrap_or(&upper)
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '+')
            .collect();
        match code.trim_end_matches('+') {
            "EC" => Some(Esrb::EarlyChildhood),
            "E" | "KA" => Some(Esrb::Everyone),
            "E10" => Some(Esrb::Everyone10Plus),
            "T" => Some(Esrb::Teen),
            "M" => Some(Esrb::Mature),
            "AO" => Some(Esrb::AdultsOnly),
            "RP" => Some(Esrb::RatingPending),
            _ => None,
        }
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

/// What a service says about the account a provider uses: its level and the limits that come
/// with it. Every field is optional; a service reports what it knows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Account {
    /// The user name, when signed in as a user.
    pub user: Option<String>,
    /// The service's own level number for the account.
    pub level: Option<u32>,
    /// The account's contribution level (ScreenScraper: 2 adds a thread, 3 or more add five).
    pub contribution: Option<u32>,
    /// Requests the account may have open at once.
    pub max_threads: Option<u32>,
    pub max_requests_per_minute: Option<u32>,
    pub max_requests_per_day: Option<u64>,
    /// Lookups a day that may find nothing.
    pub max_misses_per_day: Option<u64>,
    /// Download speed the account is allowed, KB/s.
    pub max_download_kbps: Option<u32>,
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
    /// Once before a job's lookups (and once at startup): checks the account and learns its
    /// limits. Returns notes for the admin (an account that was not accepted, say); an error
    /// means the provider cannot be used for this job.
    fn prepare(&self) -> BoxFuture<'_, Result<Vec<String>, MetaError>> {
        Box::pin(async { Ok(Vec::new()) })
    }
    /// The account it uses, as the service last described it.
    fn account(&self) -> Option<Account> {
        None
    }
    /// Lookups it can take at once (the job looks this many games up together).
    fn concurrency(&self) -> usize {
        1
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
    /// The URL to download a media URL it gave from (box art, a video): credentials added
    /// again, and for an image, at most `max_px` wide and high where the service can scale it.
    fn media_url(&self, stored: &str, max_px: Option<u32>) -> String {
        let _ = max_px;
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
    fn esrb_ratings_as_sources_write_them() {
        for (text, want) in [
            ("T", Some(Esrb::Teen)),
            ("e10+", Some(Esrb::Everyone10Plus)),
            ("E10", Some(Esrb::Everyone10Plus)),
            ("ESRB - M", Some(Esrb::Mature)),
            ("K-A", Some(Esrb::Everyone)),
            (" AO ", Some(Esrb::AdultsOnly)),
            ("12", None),
            ("", None),
        ] {
            assert_eq!(Esrb::parse(text), want, "{text:?}");
        }
        for r in Esrb::ALL {
            assert_eq!(Esrb::parse(r.code()), Some(r), "its own code reads back");
        }
    }

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
