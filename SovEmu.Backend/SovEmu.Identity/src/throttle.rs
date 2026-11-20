//! Sign-in throttling: failed sign-ins per username and per client address, locked out for a
//! while after too many.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Default failed sign-ins allowed per username within [`LOCKOUT_WINDOW`].
pub const MAX_FAILURES: u32 = 5;
/// Default failed sign-ins allowed per client address, across usernames: enough for a household
/// behind one router to mistype, too few to try many names.
pub const MAX_FAILURES_PER_IP: u32 = 20;
pub const LOCKOUT_WINDOW: Duration = Duration::from_secs(300);

/// Counts failed sign-ins per key (a username, or a client address) and locks the key out after
/// too many. In memory: a
/// restart forgets, which is fine for slowing down guessing on a home server.
#[derive(Debug)]
pub struct LoginThrottle {
    failures: Mutex<HashMap<String, (u32, Instant)>>,
    max_failures: u32,
    window: Duration,
}

impl Default for LoginThrottle {
    fn default() -> Self {
        Self::new(MAX_FAILURES, LOCKOUT_WINDOW)
    }
}

impl LoginThrottle {
    /// `max_failures` of 0 turns the throttle off.
    pub fn new(max_failures: u32, window: Duration) -> Self {
        Self { failures: Mutex::default(), max_failures, window }
    }

    fn key(username: &str) -> String {
        username.trim().to_lowercase()
    }

    /// `Err(seconds to wait)` if this username is locked out.
    pub fn check(&self, username: &str, now: Instant) -> Result<(), u64> {
        let map = self.failures.lock().unwrap_or_else(|e| e.into_inner());
        match map.get(&Self::key(username)) {
            Some(&(count, since))
                if self.max_failures > 0
                    && count >= self.max_failures
                    && now.duration_since(since) < self.window =>
            {
                Err((self.window - now.duration_since(since)).as_secs().max(1))
            }
            _ => Ok(()),
        }
    }

    pub fn failed(&self, username: &str, now: Instant) {
        let mut map = self.failures.lock().unwrap_or_else(|e| e.into_inner());
        let entry = map.entry(Self::key(username)).or_insert((0, now));
        if now.duration_since(entry.1) >= self.window {
            *entry = (0, now);
        }
        entry.0 += 1;
        if map.len() > 10_000 {
            let window = self.window;
            map.retain(|_, (_, since)| now.duration_since(*since) < window);
        }
    }

    pub fn succeeded(&self, username: &str) {
        self.failures.lock().unwrap_or_else(|e| e.into_inner()).remove(&Self::key(username));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locks_a_username_out_after_repeated_failures() {
        let t = LoginThrottle::default();
        let now = Instant::now();
        for _ in 0..MAX_FAILURES {
            assert!(t.check("Bruce", now).is_ok());
            t.failed("bruce", now);
        }
        let wait = t.check("BRUCE ", now).unwrap_err();
        assert!(wait > 0 && wait <= LOCKOUT_WINDOW.as_secs());
        assert!(t.check("someone-else", now).is_ok(), "per username");
        assert!(t.check("bruce", now + LOCKOUT_WINDOW).is_ok(), "the lockout expires");
        t.succeeded("bruce");
        assert!(t.check("bruce", now).is_ok(), "success clears it");
    }

    #[test]
    fn the_throttle_is_configurable() {
        let now = Instant::now();
        let strict = LoginThrottle::new(1, Duration::from_secs(60));
        strict.failed("a", now);
        assert_eq!(strict.check("a", now), Err(60));
        let off = LoginThrottle::new(0, Duration::from_secs(60));
        for _ in 0..100 {
            off.failed("a", now);
        }
        assert!(off.check("a", now).is_ok(), "0 turns it off");
    }
}
