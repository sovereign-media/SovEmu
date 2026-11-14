//! Operational routes and safeguards: health, Prometheus metrics, client configuration (ICE
//! servers), sign-in throttling and security headers.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::Json;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

use crate::AppState;
use crate::auth::CurrentUser;
use crate::error::{ApiError, ApiResult};

/// Failed sign-ins allowed per username within [`LOCKOUT_WINDOW`].
pub const MAX_FAILURES: u32 = 5;
pub const LOCKOUT_WINDOW: Duration = Duration::from_secs(300);

/// Counts failed sign-ins per username and locks a name out after too many. In memory: a
/// restart forgets, which is fine for slowing down guessing on a home server.
#[derive(Debug, Default)]
pub struct LoginThrottle {
    failures: Mutex<HashMap<String, (u32, Instant)>>,
}

impl LoginThrottle {
    fn key(username: &str) -> String {
        username.trim().to_lowercase()
    }

    /// `Err(seconds to wait)` if this username is locked out.
    pub fn check(&self, username: &str, now: Instant) -> Result<(), u64> {
        let map = self.failures.lock().unwrap_or_else(|e| e.into_inner());
        match map.get(&Self::key(username)) {
            Some(&(count, since))
                if count >= MAX_FAILURES && now.duration_since(since) < LOCKOUT_WINDOW =>
            {
                Err((LOCKOUT_WINDOW - now.duration_since(since)).as_secs().max(1))
            }
            _ => Ok(()),
        }
    }

    pub fn failed(&self, username: &str, now: Instant) {
        let mut map = self.failures.lock().unwrap_or_else(|e| e.into_inner());
        let entry = map.entry(Self::key(username)).or_insert((0, now));
        if now.duration_since(entry.1) >= LOCKOUT_WINDOW {
            *entry = (0, now);
        }
        entry.0 += 1;
        if map.len() > 10_000 {
            map.retain(|_, (_, since)| now.duration_since(*since) < LOCKOUT_WINDOW);
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

/// Prometheus metrics. Off unless `SOVEMU_METRICS_TOKEN` is set; then a bearer token.
pub async fn metrics(State(state): State<AppState>, headers: HeaderMap) -> ApiResult<Response> {
    let Some(token) = &state.config.metrics_token else { return Err(ApiError::not_found("route")) };
    let presented = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    if !constant_time_eq(presented.as_bytes(), token.as_bytes()) {
        return Err(ApiError::unauthorized());
    }
    let users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users").fetch_one(&state.db).await?;
    let games: Vec<(String, i64)> =
        sqlx::query_as("SELECT system, COUNT(*) FROM games GROUP BY system ORDER BY system")
            .fetch_all(&state.db)
            .await?;
    let (first, last) = state.config.udp_ports;
    let mut out = String::new();
    out.push_str("# HELP sovemu_sessions Game sessions running.\n# TYPE sovemu_sessions gauge\n");
    out.push_str(&format!("sovemu_sessions {}\n", state.workers.count()));
    out.push_str("# HELP sovemu_session_capacity Concurrent sessions the UDP port range allows.\n");
    out.push_str("# TYPE sovemu_session_capacity gauge\n");
    out.push_str(&format!("sovemu_session_capacity {}\n", u32::from(last - first) + 1));
    out.push_str("# HELP sovemu_users Accounts.\n# TYPE sovemu_users gauge\n");
    out.push_str(&format!("sovemu_users {users}\n"));
    out.push_str(
        "# HELP sovemu_games Games in the library, by system.\n# TYPE sovemu_games gauge\n",
    );
    for (system, count) in games {
        out.push_str(&format!("sovemu_games{{system=\"{system}\"}} {count}\n"));
    }
    Ok(([(header::CONTENT_TYPE, "text/plain; version=0.0.4")], out).into_response())
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[derive(Debug, Serialize)]
pub struct ClientConfig {
    /// `RTCIceServer` objects for the browser (STUN/TURN), from `SOVEMU_ICE_SERVERS`.
    ice_servers: serde_json::Value,
}

pub async fn client_config(
    State(state): State<AppState>,
    _user: CurrentUser,
) -> Json<ClientConfig> {
    Json(ClientConfig { ice_servers: state.config.ice_servers.clone() })
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
    fn compares_tokens_in_constant_time() {
        assert!(constant_time_eq(b"secret", b"secret"));
        assert!(!constant_time_eq(b"secret", b"secreT"));
        assert!(!constant_time_eq(b"secret", b"secrets"));
    }
}
