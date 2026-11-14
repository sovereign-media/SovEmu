//! Local accounts: argon2 password hashes, and a random session token in an HttpOnly cookie
//! (only its SHA-256 is stored). The first account, made through first-run setup, is the admin;
//! the admin adds the rest. OIDC sign-in comes later (docs/ROADMAP.md).

use argon2::Argon2;
use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use axum::Json;
use axum::extract::{FromRef, FromRequestParts, State};
use axum::http::header::{COOKIE, SET_COOKIE};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;

use crate::AppState;
use crate::error::{ApiError, ApiResult};

pub const COOKIE_NAME: &str = "sovemu_session";
const SESSION_DAYS: i64 = 30;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub is_admin: bool,
}

#[derive(Debug, Deserialize)]
pub struct Credentials {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct NewUser {
    pub username: String,
    pub password: String,
    #[serde(default)]
    pub is_admin: bool,
}

pub fn hash_password(password: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| anyhow::anyhow!("hashing password: {e}"))
}

pub fn verify_password(password: &str, hash: &str) -> bool {
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

pub async fn create_user(pool: &SqlitePool, new: &NewUser) -> ApiResult<User> {
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

/// Creates a sign-in session and returns the `Set-Cookie` value for it.
async fn start_session(pool: &SqlitePool, user_id: i64, secure: bool) -> ApiResult<HeaderValue> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(ApiError::internal)?;
    let token = hex::encode(bytes);
    sqlx::query(
        "INSERT INTO auth_sessions (token_hash, user_id, expires_at)
         VALUES (?, ?, datetime('now', ?))",
    )
    .bind(token_hash(&token))
    .bind(user_id)
    .bind(format!("+{SESSION_DAYS} days"))
    .execute(pool)
    .await?;
    let secure = if secure { "; Secure" } else { "" };
    let cookie = format!(
        "{COOKIE_NAME}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={}{secure}",
        SESSION_DAYS * 86_400
    );
    HeaderValue::from_str(&cookie).map_err(ApiError::internal)
}

fn cookie_token(headers: &HeaderMap) -> Option<String> {
    headers.get_all(COOKIE).iter().filter_map(|v| v.to_str().ok()).find_map(|cookies| {
        cookies.split(';').find_map(|c| {
            let (name, value) = c.trim().split_once('=')?;
            (name == COOKIE_NAME && !value.is_empty()).then(|| value.to_owned())
        })
    })
}

/// The signed-in user. Rejects with 401 if there is none.
#[derive(Debug, Clone)]
pub struct CurrentUser(pub User);

impl<S> FromRequestParts<S> for CurrentUser
where
    S: Send + Sync,
    AppState: FromRef<S>,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let state = AppState::from_ref(state);
        let token = cookie_token(&parts.headers).ok_or_else(ApiError::unauthorized)?;
        let user = sqlx::query_as::<_, User>(
            "SELECT u.id, u.username, u.is_admin FROM auth_sessions s JOIN users u ON u.id = s.user_id
             WHERE s.token_hash = ? AND s.expires_at > datetime('now')",
        )
        .bind(token_hash(&token))
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(ApiError::unauthorized)?;
        Ok(Self(user))
    }
}

/// The signed-in user, who must be an admin.
#[derive(Debug, Clone)]
pub struct AdminUser(pub User);

impl<S> FromRequestParts<S> for AdminUser
where
    S: Send + Sync,
    AppState: FromRef<S>,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let CurrentUser(user) = CurrentUser::from_request_parts(parts, state).await?;
        if user.is_admin { Ok(Self(user)) } else { Err(ApiError::forbidden()) }
    }
}

#[derive(Debug, Serialize)]
pub struct Status {
    /// No account exists yet: the client should show first-run setup.
    setup_required: bool,
    user: Option<User>,
}

pub async fn status(State(state): State<AppState>, headers: HeaderMap) -> ApiResult<Json<Status>> {
    let users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users").fetch_one(&state.db).await?;
    let mut parts = axum::http::Request::new(()).into_parts().0;
    parts.headers = headers;
    let user = CurrentUser::from_request_parts(&mut parts, &state).await.ok().map(|u| u.0);
    Ok(Json(Status { setup_required: users == 0, user }))
}

/// First-run setup: creates the admin account. Only works while there are no accounts.
pub async fn setup(
    State(state): State<AppState>,
    Json(creds): Json<Credentials>,
) -> ApiResult<Response> {
    let users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users").fetch_one(&state.db).await?;
    if users > 0 {
        return Err(ApiError::new(StatusCode::CONFLICT, "setup is already done"));
    }
    let user = create_user(
        &state.db,
        &NewUser { username: creds.username, password: creds.password, is_admin: true },
    )
    .await?;
    let cookie = start_session(&state.db, user.id, state.config.secure_cookies).await?;
    Ok(([(SET_COOKIE, cookie)], Json(user)).into_response())
}

pub async fn login(
    State(state): State<AppState>,
    Json(creds): Json<Credentials>,
) -> ApiResult<Response> {
    let now = std::time::Instant::now();
    if let Err(wait) = state.login_throttle.check(&creds.username, now) {
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
        return Err(ApiError::new(StatusCode::UNAUTHORIZED, "wrong username or password"));
    };
    state.login_throttle.succeeded(&creds.username);
    let cookie = start_session(&state.db, user.id, state.config.secure_cookies).await?;
    Ok(([(SET_COOKIE, cookie)], Json(user)).into_response())
}

pub async fn logout(State(state): State<AppState>, headers: HeaderMap) -> ApiResult<Response> {
    if let Some(token) = cookie_token(&headers) {
        sqlx::query("DELETE FROM auth_sessions WHERE token_hash = ?")
            .bind(token_hash(&token))
            .execute(&state.db)
            .await?;
    }
    let clear =
        HeaderValue::from_static("sovemu_session=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0");
    Ok(([(SET_COOKIE, clear)], StatusCode::NO_CONTENT).into_response())
}

pub async fn list_users(
    State(state): State<AppState>,
    _admin: AdminUser,
) -> ApiResult<Json<Vec<User>>> {
    let users =
        sqlx::query_as::<_, User>("SELECT id, username, is_admin FROM users ORDER BY username")
            .fetch_all(&state.db)
            .await?;
    Ok(Json(users))
}

pub async fn add_user(
    State(state): State<AppState>,
    AdminUser(admin): AdminUser,
    Json(new): Json<NewUser>,
) -> ApiResult<(StatusCode, Json<User>)> {
    let user = create_user(&state.db, &new).await?;
    tracing::info!("{} added user {} (admin: {})", admin.username, user.username, user.is_admin);
    Ok((StatusCode::CREATED, Json(user)))
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
