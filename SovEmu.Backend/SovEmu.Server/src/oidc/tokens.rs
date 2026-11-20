//! Codes, grants and tokens in the database. Only SHA-256 hashes of codes and tokens are stored.
//!
//! A *grant* is one sign-in of one app: every access and refresh token it is given hangs off it,
//! and deleting it (sign-out, a revoked token, a replayed refresh token, a changed password)
//! ends that sign-in. Access tokens are opaque and checked here on every API request, so a
//! revocation or a role change applies at once.

use sqlx::SqlitePool;

use crate::auth::{Role, User, random_token, token_hash};
use crate::error::ApiResult;

/// How long an authorization code may wait to be exchanged.
const CODE_SECONDS: u32 = 60;

/// A refresh token already exchanged is honoured again for this long: two tabs (or an app's
/// two requests) refreshing at once must not sign the user out. After it, presenting one again
/// means it was copied, and the whole sign-in is ended.
pub(crate) const REUSE_LEEWAY_SECONDS: i64 = 60;

/// Every API request's first query: the user an access token belongs to.
pub(crate) const ACCESS_USER: &str = "SELECT u.id, u.username, u.role FROM oauth_tokens t
     JOIN oauth_grants g ON g.id = t.grant_id
     JOIN users u ON u.id = g.user_id
     WHERE t.token_hash = ? AND t.kind = 'access' AND t.expires_at > datetime('now')";

/// The user an access token belongs to, if it is current.
pub(crate) async fn user_for_access_token(
    db: &SqlitePool,
    token: Option<&str>,
) -> ApiResult<Option<User>> {
    let Some(token) = token else { return Ok(None) };
    Ok(sqlx::query_as::<_, User>(ACCESS_USER).bind(token_hash(token)).fetch_optional(db).await?)
}

/// What a code was issued for.
#[derive(Debug, sqlx::FromRow)]
pub(crate) struct Code {
    pub(crate) client_id: String,
    pub(crate) user_id: i64,
    pub(crate) redirect_uri: String,
    pub(crate) code_challenge: String,
    pub(crate) scope: String,
    pub(crate) nonce: Option<String>,
    pub(crate) auth_time: i64,
}

/// Stores a new authorization code; returns it.
pub(crate) async fn new_code(db: &SqlitePool, code: &Code) -> ApiResult<String> {
    let value = random_token()?;
    sqlx::query(
        "INSERT INTO oauth_codes (code_hash, client_id, user_id, redirect_uri, code_challenge,
             scope, nonce, auth_time, expires_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, datetime('now', ?))",
    )
    .bind(token_hash(&value))
    .bind(&code.client_id)
    .bind(code.user_id)
    .bind(&code.redirect_uri)
    .bind(&code.code_challenge)
    .bind(&code.scope)
    .bind(&code.nonce)
    .bind(code.auth_time)
    .bind(format!("+{CODE_SECONDS} seconds"))
    .execute(db)
    .await?;
    Ok(value)
}

/// Takes a code: what it was issued for, if it exists and is current. Either way it can never be
/// used again.
pub(crate) async fn take_code(db: &SqlitePool, code: &str) -> ApiResult<Option<Code>> {
    #[derive(sqlx::FromRow)]
    struct Row {
        #[sqlx(flatten)]
        code: Code,
        current: bool,
    }
    let row = sqlx::query_as::<_, Row>(
        "DELETE FROM oauth_codes WHERE code_hash = ?
         RETURNING client_id, user_id, redirect_uri, code_challenge, scope, nonce, auth_time,
             expires_at > datetime('now') AS current",
    )
    .bind(token_hash(code))
    .fetch_optional(db)
    .await?;
    Ok(row.filter(|r| r.current).map(|r| r.code))
}

/// A sign-in's details, as every token it is given needs them.
#[derive(Debug, Clone, sqlx::FromRow)]
pub(crate) struct Grant {
    pub(crate) id: i64,
    pub(crate) client_id: String,
    pub(crate) user_id: i64,
    pub(crate) username: String,
    pub(crate) role: Role,
    pub(crate) scope: String,
    pub(crate) auth_time: i64,
}

/// A fresh access and refresh token.
#[derive(Debug)]
pub(crate) struct Issued {
    pub(crate) access: String,
    pub(crate) refresh: String,
    pub(crate) expires_in: u64,
}

/// How long tokens last.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Lifetimes {
    pub(crate) access_minutes: u32,
    /// A refresh token's lifetime; each refresh starts it again, so an app used within this
    /// many days stays signed in.
    pub(crate) refresh_days: u32,
}

/// Starts a sign-in for an app and gives it its first tokens.
pub(crate) async fn new_grant(
    db: &SqlitePool,
    client_id: &str,
    user: &User,
    scope: &str,
    auth_time: i64,
    lifetimes: Lifetimes,
) -> ApiResult<(Grant, Issued)> {
    let mut tx = db.begin().await?;
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO oauth_grants (client_id, user_id, scope, auth_time) VALUES (?, ?, ?, ?)
         RETURNING id",
    )
    .bind(client_id)
    .bind(user.id)
    .bind(scope)
    .bind(auth_time)
    .fetch_one(&mut *tx)
    .await?;
    let issued = issue(&mut tx, id, lifetimes).await?;
    tx.commit().await?;
    let grant = Grant {
        id,
        client_id: client_id.to_owned(),
        user_id: user.id,
        username: user.username.clone(),
        role: user.role,
        scope: scope.to_owned(),
        auth_time,
    };
    Ok((grant, issued))
}

async fn issue(
    tx: &mut sqlx::SqliteConnection,
    grant_id: i64,
    lifetimes: Lifetimes,
) -> ApiResult<Issued> {
    let access = random_token()?;
    let refresh = random_token()?;
    for (token, kind, lifetime) in [
        (&access, "access", format!("+{} minutes", lifetimes.access_minutes)),
        (&refresh, "refresh", format!("+{} days", lifetimes.refresh_days)),
    ] {
        sqlx::query(
            "INSERT INTO oauth_tokens (token_hash, grant_id, kind, expires_at)
             VALUES (?, ?, ?, datetime('now', ?))",
        )
        .bind(token_hash(token))
        .bind(grant_id)
        .bind(kind)
        .bind(lifetime)
        .execute(&mut *tx)
        .await?;
    }
    Ok(Issued { access, refresh, expires_in: u64::from(lifetimes.access_minutes) * 60 })
}

/// Why a refresh token was not exchanged.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum RefreshRefused {
    /// Unknown, expired, revoked, or another app's.
    Invalid,
    /// Exchanged before, outside the leeway: the sign-in has been ended.
    Replayed,
}

/// Exchanges a refresh token for new tokens (the old one is used up).
pub(crate) async fn refresh(
    db: &SqlitePool,
    client_id: &str,
    token: &str,
    lifetimes: Lifetimes,
) -> ApiResult<Result<(Grant, Issued), RefreshRefused>> {
    let mut tx = db.begin().await?;
    let row: Option<(Grant, Option<i64>)> = sqlx::query_as::<_, GrantRow>(
        "SELECT g.id, g.client_id, g.user_id, u.username, u.role, g.scope, g.auth_time,
             CAST((julianday('now') - julianday(t.used_at)) * 86400 AS INTEGER) AS used_ago
         FROM oauth_tokens t
         JOIN oauth_grants g ON g.id = t.grant_id
         JOIN users u ON u.id = g.user_id
         WHERE t.token_hash = ? AND t.kind = 'refresh' AND t.expires_at > datetime('now')",
    )
    .bind(token_hash(token))
    .fetch_optional(&mut *tx)
    .await?
    .map(GrantRow::split);
    let Some((grant, used_ago)) = row.filter(|(g, _)| g.client_id == client_id) else {
        return Ok(Err(RefreshRefused::Invalid));
    };
    if used_ago.is_some_and(|s| s > REUSE_LEEWAY_SECONDS) {
        sqlx::query("DELETE FROM oauth_grants WHERE id = ?")
            .bind(grant.id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        tracing::warn!(
            user.name = %grant.username,
            client = %grant.client_id,
            "a used refresh token was presented again: that sign-in is ended"
        );
        return Ok(Err(RefreshRefused::Replayed));
    }
    sqlx::query(
        "UPDATE oauth_tokens SET used_at = datetime('now')
         WHERE token_hash = ? AND used_at IS NULL",
    )
    .bind(token_hash(token))
    .execute(&mut *tx)
    .await?;
    let issued = issue(&mut tx, grant.id, lifetimes).await?;
    tx.commit().await?;
    Ok(Ok((grant, issued)))
}

#[derive(sqlx::FromRow)]
struct GrantRow {
    #[sqlx(flatten)]
    grant: Grant,
    used_ago: Option<i64>,
}

impl GrantRow {
    fn split(self) -> (Grant, Option<i64>) {
        (self.grant, self.used_ago)
    }
}

/// Revokes a token (access or refresh) of `client_id`'s: ends the sign-in it belongs to. A token
/// that is unknown, or another app's, is ignored (RFC 7009: the answer is the same).
pub(crate) async fn revoke(db: &SqlitePool, client_id: &str, token: &str) -> ApiResult<bool> {
    let ended = sqlx::query(
        "DELETE FROM oauth_grants WHERE client_id = ? AND id =
             (SELECT grant_id FROM oauth_tokens WHERE token_hash = ?)",
    )
    .bind(client_id)
    .bind(token_hash(token))
    .execute(db)
    .await?
    .rows_affected();
    Ok(ended > 0)
}

/// Deletes expired codes and tokens, and sign-ins left with no tokens. Returns how many rows.
pub(crate) async fn expire(db: &SqlitePool) -> sqlx::Result<u64> {
    let mut removed = 0;
    for sql in [
        "DELETE FROM oauth_codes WHERE expires_at <= datetime('now')",
        "DELETE FROM oauth_tokens WHERE expires_at <= datetime('now')",
        "DELETE FROM oauth_grants
         WHERE NOT EXISTS (SELECT 1 FROM oauth_tokens t WHERE t.grant_id = oauth_grants.id)",
    ] {
        removed += sqlx::query(sql).execute(db).await?.rows_affected();
    }
    Ok(removed)
}
