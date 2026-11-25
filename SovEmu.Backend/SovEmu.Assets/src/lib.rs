//! Signed asset URLs, as Sovereign hands out its own: every file URL the API gives a client
//! (artwork, videos, save screenshots, decorations) carries an HMAC-SHA256 signature over the
//! user it was issued for, the file's path and an expiry. The `/media/…` routes serve a file only
//! for a valid, unexpired signature, and serve it as that user: no cookie is needed, so every
//! client (a browser `<img>`, Coil, ExoPlayer, AVPlayer) loads it the same way, and a URL that
//! leaks works for one user's files, for a while.
//!
//! The query parameters are Sovereign's: `Sov-UserId`, `Sov-Expiry` (Unix seconds) and
//! `Sov-Signature` (hex HMAC-SHA256 over `<user id>:<decoded path>:<expiry>`). Other parameters
//! (a screenshot's `?t=` cache-buster) are not signed. The path is signed percent-decoded, so a
//! client that re-encodes it (`%28` for `(`) still matches.
//!
//! Expiries are rounded down to the hour, then `SovEmu__Assets__UrlHours` added: the same file's URL
//! stays the same for up to an hour, so caches keep working. The key is `SovEmu__Assets__SecretKey`, else
//! one generated on first run and kept in `<data>/server.json` (owner-only). Changing it rejects
//! every URL already handed out, until each client asks for fresh ones.
//!
//! This crate signs and checks; the server's `/media/…` middleware calls [`AssetSigner::check`]
//! and serves the file as the user it names.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use hmac::{Hmac, Mac};
use sha2::Sha256;

pub(crate) const USER_PARAM: &str = "Sov-UserId";
pub(crate) const EXPIRY_PARAM: &str = "Sov-Expiry";
pub(crate) const SIGNATURE_PARAM: &str = "Sov-Signature";

/// Signs and checks asset URLs with the server's key.
#[derive(Clone)]
pub struct AssetSigner {
    key: Vec<u8>,
    hours: u64,
}

impl std::fmt::Debug for AssetSigner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AssetSigner").field("hours", &self.hours).finish_non_exhaustive()
    }
}

/// Why a signed URL was refused.
#[derive(Debug, PartialEq, Eq)]
pub enum Refused {
    /// No signature, or a parameter missing or malformed.
    Unsigned,
    /// The signature does not match (another path or user, or a forged one).
    Forged,
    Expired,
}

impl AssetSigner {
    pub fn new(key: Vec<u8>, hours: u64) -> Self {
        Self { key, hours: hours.max(1) }
    }

    /// `url` (a path, maybe with a query) signed for `user_id`.
    pub fn sign(&self, user_id: i64, url: &str) -> String {
        self.sign_at(user_id, url, now())
    }

    fn sign_at(&self, user_id: i64, url: &str, now: u64) -> String {
        let expiry = (now / 3600 + self.hours) * 3600;
        let path = url.split_once('?').map_or(url, |(path, _)| path);
        let signature = self.signature(user_id, &decode(path), expiry);
        let join = if url.contains('?') { '&' } else { '?' };
        format!(
            "{url}{join}{USER_PARAM}={user_id}&{EXPIRY_PARAM}={expiry}&{SIGNATURE_PARAM}={signature}"
        )
    }

    fn signature(&self, user_id: i64, path: &str, expiry: u64) -> String {
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.key).expect("HMAC takes any key length");
        mac.update(format!("{user_id}:{path}:{expiry}").as_bytes());
        hex::encode(mac.finalize().into_bytes())
    }

    /// The user a request's signature was issued for, if it is valid for its path now.
    pub fn check(&self, raw_path: &str, query: Option<&str>) -> Result<i64, Refused> {
        self.verify(raw_path, query, now())
    }

    /// [`check`](Self::check) at a given time (Unix seconds).
    fn verify(&self, raw_path: &str, query: Option<&str>, now: u64) -> Result<i64, Refused> {
        let mut user = None;
        let mut expiry = None;
        let mut signature = None;
        for pair in query.unwrap_or_default().split('&') {
            let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
            match name {
                USER_PARAM => user = value.parse::<i64>().ok(),
                EXPIRY_PARAM => expiry = value.parse::<u64>().ok(),
                SIGNATURE_PARAM => signature = Some(value),
                _ => {}
            }
        }
        let (Some(user), Some(expiry), Some(signature)) = (user, expiry, signature) else {
            return Err(Refused::Unsigned);
        };
        let expected = self.signature(user, &decode(raw_path), expiry);
        if !constant_time_eq(expected.as_bytes(), signature.to_ascii_lowercase().as_bytes()) {
            return Err(Refused::Forged);
        }
        if now >= expiry {
            return Err(Refused::Expired);
        }
        Ok(user)
    }
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// A path with its percent-escapes decoded (what is signed and checked).
fn decode(path: &str) -> String {
    let bytes = path.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(byte) = bytes
                .get(i + 1..i + 3)
                .and_then(|h| std::str::from_utf8(h).ok())
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(byte);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Why a key could not be had.
#[derive(Debug)]
pub enum KeyError {
    /// The system has no source of randomness to generate one (the key's name).
    NoRandomness(&'static str, String),
    /// It could not be kept in `server.json` (the key's name).
    Keep(&'static str, std::path::PathBuf, std::io::Error),
}

impl std::fmt::Display for KeyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoRandomness(name, e) => write!(f, "no randomness to generate the {name}: {e}"),
            Self::Keep(name, file, e) => {
                write!(f, "keeping the {name} in {}: {e}", file.display())
            }
        }
    }
}

impl std::error::Error for KeyError {}

/// The signing key: `configured` (`SovEmu__Assets__SecretKey`), else the one kept in
/// `<data_dir>/server.json`, generated there on first run.
pub fn load_key(configured: Option<&str>, data_dir: &Path) -> Result<Vec<u8>, KeyError> {
    load_secret(configured, data_dir, "assetSigningKey", "asset signing key")
        .map(String::into_bytes)
}

/// A server secret: `configured` (its setting), else the one kept under `field` in
/// `<data_dir>/server.json`, generated there (64 random alphanumeric characters) on first run.
/// `name` is what it is called in logs and errors. The asset signing key is one; the server's
/// token signing key is another.
pub fn load_secret(
    configured: Option<&str>,
    data_dir: &Path,
    field: &str,
    name: &'static str,
) -> Result<String, KeyError> {
    if let Some(key) = configured.map(str::trim).filter(|k| !k.is_empty()) {
        return Ok(key.to_owned());
    }
    keep(data_dir, field, name, || generate_key().map_err(|e| KeyError::NoRandomness(name, e)))
}

/// Whether `<data_dir>/server.json` exists: whether this server has run here before (it writes
/// the file on its first start).
pub fn server_file_exists(data_dir: &Path) -> bool {
    data_dir.join("server.json").exists()
}

/// A per-install identifier, such as an OAuth client id: the one kept under `field` in
/// `<data_dir>/server.json`, else `first` (what an install that ran before the id was kept
/// already used), else a new random one (32 hex digits), kept there from then on. Not a secret,
/// but per install so nobody can predict it. `name` is what it is called in logs and errors.
pub fn load_id(
    data_dir: &Path,
    field: &str,
    name: &'static str,
    first: Option<&str>,
) -> Result<String, KeyError> {
    keep(data_dir, field, name, || match first {
        Some(id) => Ok(id.to_owned()),
        None => generate_id().map_err(|e| KeyError::NoRandomness(name, e)),
    })
}

/// The value kept under `field` in `<data_dir>/server.json`, else `make`'s, written there.
fn keep(
    data_dir: &Path,
    field: &str,
    name: &'static str,
    make: impl FnOnce() -> Result<String, KeyError>,
) -> Result<String, KeyError> {
    let file = data_dir.join("server.json");
    let mut doc: serde_json::Map<String, serde_json::Value> = match std::fs::read(&file) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
        Err(_) => serde_json::Map::new(),
    };
    if let Some(value) = doc.get(field).and_then(|v| v.as_str()).filter(|v| !v.is_empty()) {
        return Ok(value.to_owned());
    }
    let value = make()?;
    doc.insert(field.into(), serde_json::Value::String(value.clone()));
    let bytes = serde_json::to_vec_pretty(&doc).expect("a JSON map serializes");
    write_private(&file, &bytes).map_err(|e| KeyError::Keep(name, file.clone(), e))?;
    tracing::info!(file = %file.display(), value = name, "kept a new value");
    Ok(value)
}

/// 64 random alphanumeric characters.
fn generate_key() -> Result<String, String> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut bytes = [0u8; 64];
    getrandom::fill(&mut bytes).map_err(|e| e.to_string())?;
    // 256 is a multiple of 62 to within 4/256: the bias is far below anything usable.
    Ok(bytes.iter().map(|b| ALPHABET[*b as usize % ALPHABET.len()] as char).collect())
}

/// 128 random bits as 32 lower-case hex digits.
fn generate_id() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|e| e.to_string())?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Writes a file only its owner can read (the key is a secret), via a temporary file.
fn write_private(file: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = file.with_extension("json.tmp");
    {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        let mut f = options.open(&tmp)?;
        std::io::Write::write_all(&mut f, bytes)?;
        f.sync_all()?;
    }
    std::fs::rename(tmp, file)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signer() -> AssetSigner {
        AssetSigner::new(b"test key".to_vec(), 24)
    }

    /// Splits a signed URL into its path and query, as a request would carry them.
    fn parts(url: &str) -> (&str, &str) {
        url.split_once('?').unwrap()
    }

    #[test]
    fn a_signed_url_is_good_for_its_user_path_and_time() {
        let now = 1_700_000_123;
        let url = signer().sign_at(7, "/media/games/42/thumb", now);
        let (path, query) = parts(&url);
        assert!(query.contains("Sov-UserId=7"));
        assert!(query.contains(&format!("Sov-Expiry={}", (now / 3600 + 24) * 3600)), "{query}");
        assert_eq!(signer().verify(path, Some(query), now), Ok(7));
        assert_eq!(signer().verify(path, Some(query), now + 23 * 3600), Ok(7));
        assert_eq!(signer().verify(path, Some(query), now + 25 * 3600), Err(Refused::Expired));
    }

    #[test]
    fn the_url_stays_the_same_within_the_hour() {
        let a = signer().sign_at(7, "/media/games/42/thumb", 3600 * 10 + 5);
        let b = signer().sign_at(7, "/media/games/42/thumb", 3600 * 10 + 3000);
        assert_eq!(a, b, "cacheable");
    }

    #[test]
    fn another_path_user_key_or_a_missing_signature_is_refused() {
        let now = 1_700_000_000;
        let url = signer().sign_at(7, "/media/games/42/thumb", now);
        let (_, query) = parts(&url);
        assert_eq!(
            signer().verify("/media/games/43/thumb", Some(query), now),
            Err(Refused::Forged)
        );
        let other_user = query.replace("Sov-UserId=7", "Sov-UserId=8");
        assert_eq!(
            signer().verify("/media/games/42/thumb", Some(&other_user), now),
            Err(Refused::Forged)
        );
        let other_key = AssetSigner::new(b"other".to_vec(), 24);
        assert_eq!(
            other_key.verify("/media/games/42/thumb", Some(query), now),
            Err(Refused::Forged)
        );
        assert_eq!(signer().verify("/media/games/42/thumb", None, now), Err(Refused::Unsigned));
        assert_eq!(
            signer().verify("/media/games/42/thumb", Some("t=1"), now),
            Err(Refused::Unsigned)
        );
    }

    #[test]
    fn its_own_query_and_the_paths_encoding_do_not_matter() {
        let now = 1_700_000_000;
        // A screenshot's cache-buster stays, unsigned.
        let url = signer().sign_at(7, "/media/games/42/saves/1?t=99", now);
        let (path, query) = parts(&url);
        assert!(query.starts_with("t=99&"));
        assert_eq!(signer().verify(path, Some(query), now), Ok(7));
        // Signed escaped; requested with another escaping of the same name.
        let url =
            signer().sign_at(7, "/media/decorations/a/games/snes/Mother%202%20%28J%29.png", now);
        let (_, query) = parts(&url);
        assert_eq!(
            signer().verify(
                "/media/decorations/a/games/snes/Mother%202%20(J).png",
                Some(query),
                now
            ),
            Ok(7)
        );
    }

    #[test]
    fn the_key_is_generated_once_and_kept() {
        let dir = std::env::temp_dir().join(format!("sovemu-assets-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("server.json"), br#"{"other":"kept"}"#).unwrap();
        let first = load_key(None, &dir).unwrap();
        assert_eq!(first.len(), 64);
        assert_eq!(load_key(None, &dir).unwrap(), first, "reused");
        let doc: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dir.join("server.json")).unwrap()).unwrap();
        assert_eq!(doc["other"], "kept", "the file's other settings stay");
        // Another secret goes beside it, and leaves it alone.
        let token = load_secret(None, &dir, "tokenSigningKey", "token signing key").unwrap();
        assert_eq!(token.len(), 64);
        assert_ne!(token.as_bytes(), first.as_slice());
        assert_eq!(load_key(None, &dir).unwrap(), first);
        assert_eq!(load_secret(None, &dir, "tokenSigningKey", "token signing key").unwrap(), token);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.join("server.json")).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "owner-only");
        }
        assert_eq!(load_key(Some("configured"), &dir).unwrap(), b"configured", "the setting wins");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_id_is_made_once_per_install_and_kept() {
        let dir = std::env::temp_dir().join(format!("sovemu-assets-id-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert!(!server_file_exists(&dir), "a first start");
        let id = load_id(&dir, "deviceClientId", "device client id", None).unwrap();
        assert!(server_file_exists(&dir));
        assert_eq!(id.len(), 32);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
        // Kept: what it was first made is what every later start reads, `first` or not.
        assert_eq!(load_id(&dir, "deviceClientId", "device client id", Some("x")).unwrap(), id);
        let other = load_id(&dir, "mobileClientId", "mobile client id", None).unwrap();
        assert_ne!(other, id, "each id its own");
        // An install from before ids were kept keeps the one it used.
        let legacy = load_id(&dir, "webClientId", "web client id", Some("sovemu-web")).unwrap();
        assert_eq!(legacy, "sovemu-web");
        assert_eq!(load_id(&dir, "webClientId", "web client id", None).unwrap(), "sovemu-web");
        std::fs::remove_dir_all(&dir).ok();
    }
}
