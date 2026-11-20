//! Operational routes and safeguards: health, sign-in throttling, security headers and
//! response compression. Metrics are OpenTelemetry's (`telemetry.rs`).

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::extract::{Request, State};
use axum::http::{HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use tower_http::compression::Predicate;
use tower_http::compression::predicate::{DefaultPredicate, NotForContentType};

use crate::AppState;

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

pub async fn health(State(state): State<AppState>) -> Response {
    match sqlx::query_scalar::<_, i64>("SELECT 1").fetch_one(&state.db).await {
        Ok(_) => "ok".into_response(),
        Err(e) => {
            tracing::error!("health check: {e}");
            (StatusCode::SERVICE_UNAVAILABLE, "database unavailable").into_response()
        }
    }
}

/// Compresses responses (gzip or Brotli, as the client accepts): the GraphQL API's JSON and the
/// web client's scripts and styles. Not images, videos or anything tiny, which compress poorly or
/// not at all.
pub fn compression() -> tower_http::compression::CompressionLayer<impl Predicate> {
    let predicate = DefaultPredicate::new().and(NotForContentType::const_new("video/"));
    tower_http::compression::CompressionLayer::new().compress_when(predicate)
}

/// Headers every response carries.
pub async fn security_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    for (name, value) in [
        ("x-content-type-options", "nosniff"),
        ("x-frame-options", "DENY"),
        ("referrer-policy", "same-origin"),
        (
            "permissions-policy",
            "gamepad=(self), fullscreen=(self), camera=(), microphone=(), geolocation=()",
        ),
    ] {
        headers.entry(name).or_insert(HeaderValue::from_static(value));
    }
    response
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
