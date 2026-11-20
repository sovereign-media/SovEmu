//! Game information services: what each knows about a game ([`GameInfo`]) and the clients that
//! ask them. Each client turns a game (`LookupInput`) into what its service knows; the server's
//! lookup job decides which to ask, merges the answers and stores them.
//!
//! - [`fbneo`]: FinalBurn Neo's own DATs, for arcade and Neo Geo sets. No account needed.
//! - [`launchbox`]: the LaunchBox Games Database, from the dump it publishes. By name and
//!   platform. No account needed.
//! - [`screenscraper`]: ScreenScraper, by MD5 + size + file name. Needs developer credentials.
//! - [`thegamesdb`]: TheGamesDB, by name and platform. Needs an API key.

pub mod fbneo;
pub mod launchbox;
pub mod screenscraper;
pub mod thegamesdb;

use std::path::Path;
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
pub(crate) fn status_error(service: &str, status: reqwest::StatusCode, body: &str) -> MetaError {
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

/// The HTTP client every source uses: a timeout, and a product name in the user agent.
pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent(concat!("SovEmu/", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("a TLS backend is compiled in")
}

/// Lower-case alphanumerics only, for comparing titles across sources.
pub(crate) fn normalize(title: &str) -> String {
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
