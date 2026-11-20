//! Local accounts: argon2 password hashes and a role (`viewer` or `admin`). The first account,
//! made through first-run setup, is an admin; admins add the rest.
//!
//! Apps never see this module's cookie: they sign in through the server's OpenID Connect
//! provider (`crate::oidc`) and call the API with its bearer tokens. The cookie, a random token
//! of which only the SHA-256 is stored, is the browser's sign-in *at the provider*: it lets
//! `/oauth/authorize` hand out a code without asking for the password again.

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

/// What an account may do.
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    sqlx::Type,
    async_graphql::Enum,
)]
#[serde(rename_all = "lowercase")]
#[sqlx(rename_all = "lowercase")]
pub(crate) enum Role {
    /// Plays games: browse the library, play, keep saves and settings of their own.
    #[default]
    Viewer,
    /// Also runs the server: users, the library, emulator settings.
    Admin,
}

impl Role {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Viewer => "viewer",
            Self::Admin => "admin",
        }
    }
}

impl std::str::FromStr for Role {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "viewer" => Ok(Self::Viewer),
            "admin" => Ok(Self::Admin),
            other => Err(format!("unknown role {other:?} (viewer or admin)")),
        }
    }
}

/// An account.
#[derive(Debug, Clone, Serialize, sqlx::FromRow, async_graphql::SimpleObject)]
#[graphql(complex)]
pub(crate) struct User {
    pub id: i64,
    pub username: String,
    pub role: Role,
}

#[async_graphql::ComplexObject]
impl User {
    /// Whether the role is `ADMIN`.
    #[graphql(name = "isAdmin")]
    async fn admin_role(&self) -> bool {
        self.is_admin()
    }
}

impl User {
    pub(crate) fn is_admin(&self) -> bool {
        self.role == Role::Admin
    }
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
    pub role: Role,
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
        "INSERT INTO users (username, password_hash, role) VALUES (?, ?, ?)
         RETURNING id, username, role",
    )
    .bind(&new.username)
    .bind(hash)
    .bind(new.role)
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
/// otherwise its password and role are brought in line (the environment is the source of
/// truth, so rotating `SOVEMU_ADMIN_PASSWORD` rotates the password).
pub(crate) async fn ensure_admin(
    pool: &SqlitePool,
    username: &str,
    password: &str,
) -> anyhow::Result<()> {
    ensure_user(pool, username, password, Role::Admin).await
}

/// Makes sure a declared user exists with the declared password and role: created if missing,
/// otherwise brought in line (the environment is the source of truth; a changed password signs
/// that user out everywhere: browsers and apps).
pub(crate) async fn ensure_user(
    pool: &SqlitePool,
    username: &str,
    password: &str,
    role: Role,
) -> anyhow::Result<()> {
    validate(username, password).map_err(|e| anyhow::anyhow!("user {username}: {}", e.message))?;
    let existing: Option<(i64, String, Role)> =
        sqlx::query_as("SELECT id, password_hash, role FROM users WHERE username = ?")
            .bind(username)
            .fetch_optional(pool)
            .await?;
    match existing {
        None => {
            let new =
                NewUser { username: username.to_owned(), password: password.to_owned(), role };
            create_user(pool, &new)
                .await
                .map_err(|e| anyhow::anyhow!("creating {username}: {}", e.message))?;
            tracing::info!(
                user.name = %username,
                role = role.as_str(),
                "created a user declared in the environment"
            );
        }
        Some((id, hash, current)) => {
            if !verify_password(password, &hash) {
                sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?")
                    .bind(hash_password(password)?)
                    .bind(id)
                    .execute(pool)
                    .await?;
                sign_out_everywhere(pool, id).await?;
                tracing::info!(user.name = %username, "updated a password from the environment");
            }
            if current != role {
                sqlx::query("UPDATE users SET role = ? WHERE id = ?")
                    .bind(role)
                    .bind(id)
                    .execute(pool)
                    .await?;
                tracing::info!(
                    user.name = %username,
                    role = role.as_str(),
                    "updated a role from the environment"
                );
            }
        }
    }
    Ok(())
}

/// Ends every sign-in of a user: browsers at the provider, and every app's tokens.
pub(crate) async fn sign_out_everywhere(pool: &SqlitePool, user_id: i64) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM auth_sessions WHERE user_id = ?").bind(user_id).execute(pool).await?;
    sqlx::query("DELETE FROM oauth_grants WHERE user_id = ?").bind(user_id).execute(pool).await?;
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

/// Signs a browser in at the provider: a new sign-in session, and the `Set-Cookie` value for it.
pub(crate) async fn start_session(state: &AppState, user_id: i64) -> ApiResult<HeaderValue> {
    let days = state.config.session_days;
    let token = random_token()?;
    sqlx::query(
        "INSERT INTO auth_sessions (token_hash, user_id, expires_at)
         VALUES (?, ?, datetime('now', ?))",
    )
    .bind(token_hash(&token))
    .bind(user_id)
    .bind(format!("+{days} days"))
    .execute(&state.db)
    .await?;
    let secure = if state.config.secure_cookies { "; Secure" } else { "" };
    let cookie = format!(
        "{COOKIE_NAME}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={}{secure}",
        u64::from(days) * 86_400
    );
    HeaderValue::from_str(&cookie).map_err(ApiError::internal)
}

/// 32 random bytes as hex: a session token, an authorization code, an access or refresh token.
pub(crate) fn random_token() -> ApiResult<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(ApiError::internal)?;
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

/// The browser's sign-in at the provider: the user its cookie names, and when they signed in
/// (Unix seconds), if the sign-in is current.
pub(crate) async fn browser_user(
    db: &SqlitePool,
    headers: &HeaderMap,
) -> ApiResult<Option<(User, i64)>> {
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

/// First-run setup: creates the admin account and signs the browser in at the provider (the
/// web client then finishes signing in through `/oauth/authorize`). Only works while there are
/// no accounts. Returns the user and the `Set-Cookie` value.
pub(crate) async fn setup(state: &AppState, creds: Credentials) -> ApiResult<(User, HeaderValue)> {
    let users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users").fetch_one(&state.db).await?;
    if users > 0 {
        return Err(ApiError::new(StatusCode::CONFLICT, "setup is already done"));
    }
    let user = create_user(
        &state.db,
        &NewUser { username: creds.username, password: creds.password, role: Role::Admin },
    )
    .await?;
    let cookie = start_session(state, user.id).await?;
    Ok((user, cookie))
}

/// Checks a username and password: the sign-in form's and the password grant's one check.
/// `client` is the address it came from (`client_ip`), when known: failures count against it
/// as well as the username, so one address cannot try many names. Wrong credentials are
/// `UNAUTHORIZED`; a locked-out name or address is `TOO_MANY_REQUESTS`.
pub(crate) async fn check_password(
    state: &AppState,
    creds: &Credentials,
    client: Option<IpAddr>,
) -> ApiResult<User> {
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
    let row: Option<(i64, String, Role, String)> =
        sqlx::query_as("SELECT id, username, role, password_hash FROM users WHERE username = ?")
            .bind(&creds.username)
            .fetch_optional(&state.db)
            .await?;
    // Hash even for an unknown user, so timing does not reveal which usernames exist.
    let (user, hash) = match row {
        Some((id, username, role, hash)) => (Some(User { id, username, role }), hash),
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
    Ok(user)
}

/// Ends the browser's sign-in at the provider; returns the `Set-Cookie` value that clears the
/// cookie.
pub(crate) async fn end_session(state: &AppState, headers: &HeaderMap) -> ApiResult<HeaderValue> {
    if let Some(token) = cookie_token(headers) {
        sqlx::query("DELETE FROM auth_sessions WHERE token_hash = ?")
            .bind(token_hash(&token))
            .execute(&state.db)
            .await?;
    }
    Ok(HeaderValue::from_static("sovemu_session=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0"))
}

pub(crate) async fn list_users(state: &AppState) -> ApiResult<Vec<User>> {
    Ok(sqlx::query_as::<_, User>("SELECT id, username, role FROM users ORDER BY username")
        .fetch_all(&state.db)
        .await?)
}

pub(crate) async fn add_user(state: &AppState, admin: &User, new: &NewUser) -> ApiResult<User> {
    let user = create_user(&state.db, new).await?;
    tracing::info!(
        by = %admin.username,
        user.name = %user.username,
        role = user.role.as_str(),
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
    fn parses_roles() {
        assert_eq!("viewer".parse(), Ok(Role::Viewer));
        assert_eq!(" Admin ".parse(), Ok(Role::Admin));
        assert!("owner".parse::<Role>().is_err());
        assert_eq!(Role::default(), Role::Viewer);
    }

    #[test]
    fn validates_usernames_and_passwords() {
        assert!(validate("bruce", "longenough").is_ok());
        assert!(validate("", "longenough").is_err());
        assert!(validate("has space", "longenough").is_err());
        assert!(validate("bruce", "short").is_err());
    }
}
