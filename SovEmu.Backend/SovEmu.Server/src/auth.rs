//! Local accounts: argon2 password hashes, and a random session token in an HttpOnly cookie
//! (only its SHA-256 is stored). The first account, made through first-run setup, is the admin;
//! the admin adds the rest. OIDC sign-in comes later (docs/ROADMAP.md).

use std::net::IpAddr;

use argon2::Argon2;
use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use axum::http::header::COOKIE;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;

use crate::AppState;
use crate::error::{ApiError, ApiResult};

pub(crate) const COOKIE_NAME: &str = "sovemu_session";

/// An account.
#[derive(Debug, Clone, Serialize, sqlx::FromRow, async_graphql::SimpleObject)]
pub(crate) struct User {
    pub id: i64,
    pub username: String,
    pub is_admin: bool,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Credentials {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct NewUser {
    pub username: String,
    pub password: String,
    #[serde(default)]
    pub is_admin: bool,
}

pub(crate) fn hash_password(password: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| anyhow::anyhow!("hashing password: {e}"))
}

pub(crate) fn verify_password(password: &str, hash: &str) -> bool {
    PasswordHash::new(hash)
        .map(|parsed| Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok())
        .unwrap_or(false)
}

fn token_hash(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

fn validate(username: &str, password: &str) -> ApiResult<()> {
    let ok_name = (1..=32).contains(&username.chars().count())
        && username.chars().all(|c| c.is_alphanumeric() || "-_.".contains(c));
    if !ok_name {
        return Err(ApiError::bad_request("usernames are 1-32 letters, digits, '-', '_' or '.'"));
    }
    if password.chars().count() < 8 {
        return Err(ApiError::bad_request("passwords need at least 8 characters"));
    }
    Ok(())
}

pub(crate) async fn create_user(pool: &SqlitePool, new: &NewUser) -> ApiResult<User> {
    validate(&new.username, &new.password)?;
    let hash = hash_password(&new.password)?;
    let result = sqlx::query_as::<_, User>(
        "INSERT INTO users (username, password_hash, is_admin) VALUES (?, ?, ?)
         RETURNING id, username, is_admin",
    )
    .bind(&new.username)
    .bind(hash)
    .bind(new.is_admin)
    .fetch_one(pool)
    .await;
    match result {
        Ok(user) => Ok(user),
        Err(sqlx::Error::Database(e)) if e.is_unique_violation() => {
            Err(ApiError::new(StatusCode::CONFLICT, "that username is taken"))
        }
        Err(e) => Err(e.into()),
    }
}

/// Makes sure the configured admin exists with the configured password: created if missing,
/// otherwise its password and admin flag are brought in line (the environment is the source of
/// truth, so rotating `SOVEMU_ADMIN_PASSWORD` rotates the password).
pub(crate) async fn ensure_admin(
    pool: &SqlitePool,
    username: &str,
    password: &str,
) -> anyhow::Result<()> {
    ensure_user(pool, username, password, true).await
}

/// Makes sure a declared user exists with the declared password and admin flag: created if
/// missing, otherwise brought in line (the environment is the source of truth; a changed
/// password signs that user's browsers out).
pub(crate) async fn ensure_user(
    pool: &SqlitePool,
    username: &str,
    password: &str,
    admin: bool,
) -> anyhow::Result<()> {
    validate(username, password).map_err(|e| anyhow::anyhow!("user {username}: {}", e.message))?;
    let existing: Option<(i64, String, bool)> =
        sqlx::query_as("SELECT id, password_hash, is_admin FROM users WHERE username = ?")
            .bind(username)
            .fetch_optional(pool)
            .await?;
    match existing {
        None => {
            let new = NewUser {
                username: username.to_owned(),
                password: password.to_owned(),
                is_admin: admin,
            };
            create_user(pool, &new)
                .await
                .map_err(|e| anyhow::anyhow!("creating {username}: {}", e.message))?;
            tracing::info!(user.name = %username, admin, "created a user declared in the environment");
        }
        Some((id, hash, is_admin)) => {
            if !verify_password(password, &hash) {
                sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?")
                    .bind(hash_password(password)?)
                    .bind(id)
                    .execute(pool)
                    .await?;
                sqlx::query("DELETE FROM auth_sessions WHERE user_id = ?")
                    .bind(id)
                    .execute(pool)
                    .await?;
                tracing::info!(user.name = %username, "updated a password from the environment");
            }
            if is_admin != admin {
                sqlx::query("UPDATE users SET is_admin = ? WHERE id = ?")
                    .bind(admin)
                    .bind(id)
                    .execute(pool)
                    .await?;
            }
        }
    }
    Ok(())
}

/// Deletes every user not in `keep` (the declared users), for `SOVEMU_USERS_PRUNE`.
pub(crate) async fn prune_users(pool: &SqlitePool, keep: &[String]) -> anyhow::Result<u64> {
    let all: Vec<(i64, String)> =
        sqlx::query_as("SELECT id, username FROM users").fetch_all(pool).await?;
    let mut removed = 0;
    for (id, name) in all {
        if !keep.iter().any(|k| k.eq_ignore_ascii_case(&name)) {
            sqlx::query("DELETE FROM users WHERE id = ?").bind(id).execute(pool).await?;
            tracing::info!(user.name = %name, "removed a user not declared in the environment");
            removed += 1;
        }
    }
    Ok(removed)
}

/// Creates a sign-in session and returns the `Set-Cookie` value for it.
async fn start_session(
    pool: &SqlitePool,
    user_id: i64,
    secure: bool,
    days: u32,
) -> ApiResult<HeaderValue> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(ApiError::internal)?;
    let token = hex::encode(bytes);
    sqlx::query(
        "INSERT INTO auth_sessions (token_hash, user_id, expires_at)
         VALUES (?, ?, datetime('now', ?))",
    )
    .bind(token_hash(&token))
    .bind(user_id)
    .bind(format!("+{days} days"))
    .execute(pool)
    .await?;
    let secure = if secure { "; Secure" } else { "" };
    let cookie = format!(
        "{COOKIE_NAME}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={}{secure}",
        u64::from(days) * 86_400
    );
    HeaderValue::from_str(&cookie).map_err(ApiError::internal)
}

pub(crate) fn cookie_token(headers: &HeaderMap) -> Option<String> {
    headers.get_all(COOKIE).iter().filter_map(|v| v.to_str().ok()).find_map(|cookies| {
        cookies.split(';').find_map(|c| {
            let (name, value) = c.trim().split_once('=')?;
            (name == COOKIE_NAME && !value.is_empty()).then(|| value.to_owned())
        })
    })
}

/// Every signed-in request's first query.
pub(crate) const SESSION_USER: &str = "SELECT u.id, u.username, u.is_admin FROM auth_sessions s
     JOIN users u ON u.id = s.user_id
     WHERE s.token_hash = ? AND s.expires_at > datetime('now')";

/// The user a sign-in cookie's token belongs to, if it is current.
pub(crate) async fn user_for_token(
    db: &SqlitePool,
    token: Option<&str>,
) -> ApiResult<Option<User>> {
    let Some(token) = token else { return Ok(None) };
    Ok(sqlx::query_as::<_, User>(SESSION_USER).bind(token_hash(token)).fetch_optional(db).await?)
}

/// First-run setup: creates the admin account and signs it in. Only works while there are no
/// accounts. Returns the user and the `Set-Cookie` value.
pub(crate) async fn setup(state: &AppState, creds: Credentials) -> ApiResult<(User, HeaderValue)> {
    let users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users").fetch_one(&state.db).await?;
    if users > 0 {
        return Err(ApiError::new(StatusCode::CONFLICT, "setup is already done"));
    }
    let user = create_user(
        &state.db,
        &NewUser { username: creds.username, password: creds.password, is_admin: true },
    )
    .await?;
    let cookie =
        start_session(&state.db, user.id, state.config.secure_cookies, state.config.session_days)
            .await?;
    Ok((user, cookie))
}

/// Signs in. Returns the user and the `Set-Cookie` value.
/// `client` is the address it came from (`client_ip`), when known: failures count against it
/// as well as the username, so one address cannot try many names.
pub(crate) async fn login(
    state: &AppState,
    creds: Credentials,
    client: Option<IpAddr>,
) -> ApiResult<(User, HeaderValue)> {
    let now = std::time::Instant::now();
    let address = client.map(|ip| ip.to_string());
    let locked =
        state.login_throttle.check(&creds.username, now).err().or_else(|| {
            address.as_deref().and_then(|a| state.login_ip_throttle.check(a, now).err())
        });
    if let Some(wait) = locked {
        tracing::warn!(user.name = %creds.username, wait_s = wait, "sign-in refused: locked out");
        return Err(ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            format!("too many failed attempts; try again in {wait} s"),
        ));
    }
    let row: Option<(i64, String, bool, String)> = sqlx::query_as(
        "SELECT id, username, is_admin, password_hash FROM users WHERE username = ?",
    )
    .bind(&creds.username)
    .fetch_optional(&state.db)
    .await?;
    // Hash even for an unknown user, so timing does not reveal which usernames exist.
    let (user, hash) = match row {
        Some((id, username, is_admin, hash)) => (Some(User { id, username, is_admin }), hash),
        None => (None, state.dummy_hash.clone()),
    };
    let valid = verify_password(&creds.password, &hash);
    let Some(user) = user.filter(|_| valid) else {
        state.login_throttle.failed(&creds.username, now);
        if let Some(a) = &address {
            state.login_ip_throttle.failed(a, now);
        }
        tracing::warn!(user.name = %creds.username, "sign-in failed");
        return Err(ApiError::new(StatusCode::UNAUTHORIZED, "wrong username or password"));
    };
    state.login_throttle.succeeded(&creds.username);
    tracing::info!(user.name = %user.username, user.id = user.id, "signed in");
    let cookie =
        start_session(&state.db, user.id, state.config.secure_cookies, state.config.session_days)
            .await?;
    Ok((user, cookie))
}

/// Ends the sign-in session; returns the `Set-Cookie` value that clears the cookie.
pub(crate) async fn logout(state: &AppState, token: Option<&str>) -> ApiResult<HeaderValue> {
    if let Some(token) = token {
        sqlx::query("DELETE FROM auth_sessions WHERE token_hash = ?")
            .bind(token_hash(token))
            .execute(&state.db)
            .await?;
    }
    Ok(HeaderValue::from_static("sovemu_session=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0"))
}

pub(crate) async fn list_users(state: &AppState) -> ApiResult<Vec<User>> {
    Ok(sqlx::query_as::<_, User>("SELECT id, username, is_admin FROM users ORDER BY username")
        .fetch_all(&state.db)
        .await?)
}

pub(crate) async fn add_user(state: &AppState, admin: &User, new: &NewUser) -> ApiResult<User> {
    let user = create_user(&state.db, new).await?;
    tracing::info!(
        by = %admin.username,
        user.name = %user.username,
        admin = user.is_admin,
        "added a user"
    );
    Ok(user)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passwords_hash_and_verify() {
        let hash = hash_password("correct horse").unwrap();
        assert!(hash.starts_with("$argon2"));
        assert!(verify_password("correct horse", &hash));
        assert!(!verify_password("wrong horse", &hash));
        assert!(!verify_password("x", "not a hash"));
    }

    #[test]
    fn reads_the_session_cookie() {
        let mut headers = HeaderMap::new();
        headers.insert(COOKIE, HeaderValue::from_static("a=1; sovemu_session=abc123; b=2"));
        assert_eq!(cookie_token(&headers).as_deref(), Some("abc123"));
        headers.insert(COOKIE, HeaderValue::from_static("sovemu_session="));
        assert_eq!(cookie_token(&headers), None);
    }

    #[test]
    fn validates_usernames_and_passwords() {
        assert!(validate("bruce", "longenough").is_ok());
        assert!(validate("", "longenough").is_err());
        assert!(validate("has space", "longenough").is_err());
        assert!(validate("bruce", "short").is_err());
    }
}
