//! Local accounts: argon2 password hashes and a role. The first account, made through first-run
//! setup, is an admin; admins add the rest, and the environment can declare them all.

use argon2::Argon2;
use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::error::{Error, Result};

/// What an account may do.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(rename_all = "lowercase")]
pub enum Role {
    /// Plays games: browses the library, plays, keeps saves and settings of their own.
    #[default]
    Viewer,
    /// Also runs the server: users, the library, emulator settings.
    Admin,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Viewer => "viewer",
            Self::Admin => "admin",
        }
    }
}

impl std::str::FromStr for Role {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "viewer" => Ok(Self::Viewer),
            "admin" => Ok(Self::Admin),
            other => Err(format!("unknown role {other:?} (viewer or admin)")),
        }
    }
}

/// An account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, sqlx::FromRow)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub role: Role,
}

impl User {
    pub fn is_admin(&self) -> bool {
        self.role == Role::Admin
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Credentials {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NewUser {
    pub username: String,
    pub password: String,
    #[serde(default)]
    pub role: Role,
}

pub(crate) fn hash_password(password: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| Error::internal(format!("hashing a password: {e}")))
}

pub(crate) fn verify_password(password: &str, hash: &str) -> bool {
    PasswordHash::new(hash)
        .map(|parsed| Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok())
        .unwrap_or(false)
}

fn validate(username: &str, password: &str) -> Result<()> {
    let ok_name = (1..=32).contains(&username.chars().count())
        && username.chars().all(|c| c.is_alphanumeric() || "-_.".contains(c));
    if !ok_name {
        return Err(Error::BadRequest(
            "usernames are 1-32 letters, digits, '-', '_' or '.'".into(),
        ));
    }
    validate_password(password)
}

fn validate_password(password: &str) -> Result<()> {
    if password.chars().count() < 8 {
        return Err(Error::BadRequest("passwords need at least 8 characters".into()));
    }
    Ok(())
}

pub(crate) async fn create_user(pool: &SqlitePool, new: &NewUser) -> Result<User> {
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
            Err(Error::Conflict("that username is taken".into()))
        }
        Err(e) => Err(e.into()),
    }
}

pub(crate) async fn user_by_id(pool: &SqlitePool, id: i64) -> Result<Option<User>> {
    Ok(sqlx::query_as::<_, User>("SELECT id, username, role FROM users WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await?)
}

/// The user and their password hash, by name (case-insensitive).
pub(crate) async fn user_with_hash(
    pool: &SqlitePool,
    username: &str,
) -> Result<Option<(User, String)>> {
    let row: Option<(i64, String, Role, String)> =
        sqlx::query_as("SELECT id, username, role, password_hash FROM users WHERE username = ?")
            .bind(username)
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|(id, username, role, hash)| (User { id, username, role }, hash)))
}

pub(crate) async fn list_users(pool: &SqlitePool) -> Result<Vec<User>> {
    Ok(sqlx::query_as::<_, User>("SELECT id, username, role FROM users ORDER BY username")
        .fetch_all(pool)
        .await?)
}

pub(crate) async fn count_users(pool: &SqlitePool) -> Result<i64> {
    Ok(sqlx::query_scalar("SELECT COUNT(*) FROM users").fetch_one(pool).await?)
}

/// How many admins there are.
pub(crate) async fn count_admins(pool: &SqlitePool) -> Result<i64> {
    Ok(sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE role = 'admin'")
        .fetch_one(pool)
        .await?)
}

pub(crate) async fn set_role(pool: &SqlitePool, id: i64, role: Role) -> Result<()> {
    sqlx::query("UPDATE users SET role = ? WHERE id = ?").bind(role).bind(id).execute(pool).await?;
    Ok(())
}

pub(crate) async fn set_password(pool: &SqlitePool, id: i64, password: &str) -> Result<()> {
    validate_password(password)?;
    sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?")
        .bind(hash_password(password)?)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Deletes an account; everything of theirs in the database goes with it (`ON DELETE CASCADE`).
pub(crate) async fn delete(pool: &SqlitePool, id: i64) -> Result<()> {
    sqlx::query("DELETE FROM users WHERE id = ?").bind(id).execute(pool).await?;
    Ok(())
}

/// What [`ensure`] changed about an existing account.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Ensured {
    pub(crate) created: bool,
    pub(crate) password_changed: bool,
}

/// Makes sure a declared user exists with the declared password and role: created if missing,
/// otherwise brought in line (the environment is the source of truth).
pub(crate) async fn ensure(
    pool: &SqlitePool,
    username: &str,
    password: &str,
    role: Role,
) -> Result<(i64, Ensured)> {
    validate(username, password).map_err(|e| Error::BadRequest(format!("user {username}: {e}")))?;
    let existing: Option<(i64, String, Role)> =
        sqlx::query_as("SELECT id, password_hash, role FROM users WHERE username = ?")
            .bind(username)
            .fetch_optional(pool)
            .await?;
    let Some((id, hash, current)) = existing else {
        let new = NewUser { username: username.to_owned(), password: password.to_owned(), role };
        let user = create_user(pool, &new)
            .await
            .map_err(|e| Error::BadRequest(format!("creating {username}: {e}")))?;
        tracing::info!(
            user.name = %username,
            role = role.as_str(),
            "created a user declared in the environment"
        );
        return Ok((user.id, Ensured { created: true, password_changed: false }));
    };
    let mut ensured = Ensured::default();
    if !verify_password(password, &hash) {
        set_password(pool, id, password).await?;
        ensured.password_changed = true;
        tracing::info!(user.name = %username, "updated a password from the environment");
    }
    if current != role {
        set_role(pool, id, role).await?;
        tracing::info!(user.name = %username, role = role.as_str(), "updated a role from the environment");
    }
    Ok((id, ensured))
}

/// Deletes every user not in `keep`; returns the ids removed.
pub(crate) async fn prune(pool: &SqlitePool, keep: &[String]) -> Result<Vec<i64>> {
    let all: Vec<(i64, String)> =
        sqlx::query_as("SELECT id, username FROM users").fetch_all(pool).await?;
    let mut removed = Vec::new();
    for (id, name) in all {
        if !keep.iter().any(|k| k.eq_ignore_ascii_case(&name)) {
            delete(pool, id).await?;
            tracing::info!(user.name = %name, "removed a user not declared in the environment");
            removed.push(id);
        }
    }
    Ok(removed)
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
    fn validates_usernames_and_passwords() {
        assert!(validate("bruce", "longenough").is_ok());
        assert!(validate("", "longenough").is_err());
        assert!(validate("has space", "longenough").is_err());
        assert!(validate("bruce", "short").is_err());
    }

    #[test]
    fn parses_roles() {
        assert_eq!("viewer".parse(), Ok(Role::Viewer));
        assert_eq!(" Admin ".parse(), Ok(Role::Admin));
        assert!("owner".parse::<Role>().is_err());
        assert_eq!(Role::default(), Role::Viewer);
    }
}
