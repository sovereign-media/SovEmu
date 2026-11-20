//! The browser's sign-in at the server: a random token in an HttpOnly cookie, of which only the
//! SHA-256 is stored (`auth_sessions`). It lets `/oauth/authorize` and the device page know who
//! the browser belongs to without asking for the password again. Nothing else reads it: the API
//! takes bearer tokens only.

use axum::http::header::COOKIE;
use axum::http::{HeaderMap, HeaderValue};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;

use crate::accounts::{Role, User};
use crate::error::{Error, Result};

pub(crate) const COOKIE_NAME: &str = "sovemu_session";

/// 32 random bytes as hex.
pub(crate) fn random_token() -> Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(Error::internal)?;
    Ok(hex::encode(bytes))
}

/// What is stored for a token: its SHA-256, as hex.
pub(crate) fn token_hash(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

pub(crate) fn cookie_token(headers: &HeaderMap) -> Option<String> {
    headers.get_all(COOKIE).iter().filter_map(|v| v.to_str().ok()).find_map(|cookies| {
        cookies.split(';').find_map(|c| {
            let (name, value) = c.trim().split_once('=')?;
            (name == COOKIE_NAME && !value.is_empty()).then(|| value.to_owned())
        })
    })
}

/// Signs a browser in: a new session, and the `Set-Cookie` value for it.
pub(crate) async fn start(
    db: &SqlitePool,
    user_id: i64,
    days: u32,
    secure: bool,
) -> Result<HeaderValue> {
    let token = random_token()?;
    sqlx::query(
        "INSERT INTO auth_sessions (token_hash, user_id, expires_at)
         VALUES (?, ?, datetime('now', ?))",
    )
    .bind(token_hash(&token))
    .bind(user_id)
    .bind(format!("+{days} days"))
    .execute(db)
    .await?;
    let secure = if secure { "; Secure" } else { "" };
    let cookie = format!(
        "{COOKIE_NAME}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={}{secure}",
        u64::from(days) * 86_400
    );
    HeaderValue::from_str(&cookie).map_err(Error::internal)
}

/// The user a browser's cookie names, and when they signed in (Unix seconds), if current.
pub(crate) async fn user(db: &SqlitePool, headers: &HeaderMap) -> Result<Option<(User, i64)>> {
    let Some(token) = cookie_token(headers) else { return Ok(None) };
    let row: Option<(i64, String, Role, i64)> = sqlx::query_as(
        "SELECT u.id, u.username, u.role, CAST(strftime('%s', s.created_at) AS INTEGER)
         FROM auth_sessions s JOIN users u ON u.id = s.user_id
         WHERE s.token_hash = ? AND s.expires_at > datetime('now')",
    )
    .bind(token_hash(&token))
    .fetch_optional(db)
    .await?;
    Ok(row.map(|(id, username, role, at)| (User { id, username, role }, at)))
}

/// Ends a browser's sign-in; the `Set-Cookie` value that clears the cookie.
pub(crate) async fn end(db: &SqlitePool, headers: &HeaderMap) -> Result<HeaderValue> {
    if let Some(token) = cookie_token(headers) {
        sqlx::query("DELETE FROM auth_sessions WHERE token_hash = ?")
            .bind(token_hash(&token))
            .execute(db)
            .await?;
    }
    Ok(HeaderValue::from_static("sovemu_session=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0"))
}

/// Ends every browser sign-in of a user.
pub(crate) async fn end_all(db: &SqlitePool, user_id: i64) -> Result<()> {
    sqlx::query("DELETE FROM auth_sessions WHERE user_id = ?").bind(user_id).execute(db).await?;
    Ok(())
}

/// Deletes expired sessions; how many.
pub(crate) async fn expire(db: &SqlitePool) -> Result<u64> {
    Ok(sqlx::query("DELETE FROM auth_sessions WHERE expires_at <= datetime('now')")
        .execute(db)
        .await?
        .rows_affected())
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;

    #[test]
    fn reads_the_session_cookie() {
        let mut headers = HeaderMap::new();
        headers.insert(COOKIE, HeaderValue::from_static("a=1; sovemu_session=abc123; b=2"));
        assert_eq!(cookie_token(&headers).as_deref(), Some("abc123"));
        headers.insert(COOKIE, HeaderValue::from_static("sovemu_session="));
        assert_eq!(cookie_token(&headers), None);
    }
}
