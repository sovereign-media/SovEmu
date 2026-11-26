//! Short-lived relay credentials, minted by the server for each signed-in player.
//!
//! The scheme is the TURN REST API's (coturn's `use-auth-secret`): the username is
//! `<expiry, Unix seconds>:<user>` and the password is base64(HMAC-SHA1(secret, username)). So the
//! relay keeps no table of credentials: it works the password out from the username, and refuses
//! one whose expiry has passed.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use hmac::{Hmac, Mac};

/// A player's relay credentials, for an `RTCIceServer`'s `username` and `credential`.
#[derive(Clone, PartialEq, Eq)]
pub struct Credentials {
    pub username: String,
    pub password: String,
    /// When they stop working, in Unix seconds.
    pub expires: u64,
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credentials")
            .field("username", &self.username)
            .field("password", &"<redacted>")
            .field("expires", &self.expires)
            .finish()
    }
}

/// Credentials for `user` lasting at least `lifetime` from `now`. The expiry is rounded up to the
/// hour, so a player asking again within the hour gets the same ones (and the relay learns one
/// username per player per hour, not one per request).
pub fn mint(secret: &[u8], user: &str, lifetime: Duration, now: SystemTime) -> Credentials {
    let at = now.duration_since(UNIX_EPOCH).unwrap_or_default() + lifetime;
    let expires = at.as_secs().div_ceil(3600) * 3600;
    let username = format!("{expires}:{user}");
    Credentials { password: password(secret, &username), username, expires }
}

/// The password for `username`.
pub(crate) fn password(secret: &[u8], username: &str) -> String {
    let mut mac = Hmac::<sha1::Sha1>::new_from_slice(secret).expect("HMAC takes any key length");
    mac.update(username.as_bytes());
    base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes())
}

/// Whether `username` is one this scheme minted that has not expired at `now`.
pub(crate) fn current(username: &str, now: SystemTime) -> bool {
    let now = now.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    username
        .split_once(':')
        .and_then(|(expires, _)| expires.parse::<u64>().ok())
        .is_some_and(|expires| expires > now)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_password_is_coturns() {
        // coturn's use-auth-secret: base64(hmac-sha1(secret, "<expiry>:<user>")), worked out
        // with `printf '1700000000:alice' | openssl dgst -sha1 -hmac s3cret -binary | base64`.
        assert_eq!(password(b"s3cret", "1700000000:alice"), "TtElzSjT0GdnTQ9xdcRNxx96yQs=");
    }

    #[test]
    fn expiry_is_rounded_up_to_the_hour() {
        let now = UNIX_EPOCH + Duration::from_secs(10 * 3600 + 5);
        let a = mint(b"k", "7", Duration::from_secs(3600), now);
        assert_eq!(a.expires, 12 * 3600);
        assert_eq!(a.username, format!("{}:7", 12 * 3600));
        // Asked again later in the same hour: the same credentials.
        let b = mint(b"k", "7", Duration::from_secs(3600), now + Duration::from_secs(600));
        assert_eq!(a, b);
    }

    #[test]
    fn expired_and_foreign_usernames_are_not_current() {
        let now = UNIX_EPOCH + Duration::from_secs(5000);
        assert!(current("5001:7", now));
        assert!(!current("5000:7", now));
        assert!(!current("alice", now));
        assert!(!current("soon:7", now));
    }

    #[test]
    fn debug_hides_the_password() {
        let c = mint(b"k", "7", Duration::from_secs(60), UNIX_EPOCH);
        assert!(!format!("{c:?}").contains(&c.password));
    }
}
