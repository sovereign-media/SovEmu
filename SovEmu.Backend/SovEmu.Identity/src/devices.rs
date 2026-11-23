//! What each app sign-in calls its device, by refresh family (`sign_in_devices`): the app names
//! it once signed in, and the admin's lists of sign-ins show it. Beside it, what a sign-in's
//! device says it can decode (`sign_in_capabilities`): a document the server writes and reads,
//! kept here as text. SovEmu's own tables beside oauth-as's records, which it never changes.

use std::collections::HashMap;

use sqlx::SqlitePool;

use crate::error::{Error, Result};

/// Longest device name kept.
const MAX_NAME: usize = 80;

/// A device name as kept: whitespace and control characters folded to single spaces, 1 to
/// [`MAX_NAME`] characters.
pub(crate) fn clean(name: &str) -> Result<String> {
    let name = name
        .split(|c: char| c.is_whitespace() || c.is_control())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if name.is_empty() || name.chars().count() > MAX_NAME {
        return Err(Error::BadRequest(format!("a device's name has 1 to {MAX_NAME} characters")));
    }
    Ok(name)
}

/// Names the sign-in `family_id`, replacing any name it had.
pub(crate) async fn name(db: &SqlitePool, family_id: &str, name: &str) -> Result<()> {
    sqlx::query(
        "INSERT INTO sign_in_devices (family_id, name) VALUES (?, ?)
         ON CONFLICT (family_id) DO UPDATE SET name = excluded.name, named_at = datetime('now')",
    )
    .bind(family_id)
    .bind(name)
    .execute(db)
    .await
    .map_err(Error::internal)?;
    Ok(())
}

/// Every sign-in's device name, by family.
pub(crate) async fn names(db: &SqlitePool) -> Result<HashMap<String, String>> {
    let rows: Vec<(String, String)> = sqlx::query_as("SELECT family_id, name FROM sign_in_devices")
        .fetch_all(db)
        .await
        .map_err(Error::internal)?;
    Ok(rows.into_iter().collect())
}

/// Keeps the sign-in `family_id`'s capabilities document, replacing any it had.
pub(crate) async fn keep_capabilities(
    db: &SqlitePool,
    family_id: &str,
    document: &str,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO sign_in_capabilities (family_id, document) VALUES (?, ?)
         ON CONFLICT (family_id) DO UPDATE
           SET document = excluded.document, reported_at = datetime('now')",
    )
    .bind(family_id)
    .bind(document)
    .execute(db)
    .await
    .map_err(Error::internal)?;
    Ok(())
}

/// Every sign-in's capabilities document, by family.
pub(crate) async fn capabilities(db: &SqlitePool) -> Result<HashMap<String, String>> {
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT family_id, document FROM sign_in_capabilities")
            .fetch_all(db)
            .await
            .map_err(Error::internal)?;
    Ok(rows.into_iter().collect())
}

/// Forgets the names and capabilities of sign-ins that have ended (no refresh token is left
/// in their family). Returns how many names went.
pub(crate) async fn prune(db: &SqlitePool) -> Result<u64> {
    sqlx::query(
        "DELETE FROM sign_in_capabilities WHERE family_id NOT IN
           (SELECT family_id FROM oauth_refresh_tokens)",
    )
    .execute(db)
    .await
    .map_err(Error::internal)?;
    Ok(sqlx::query(
        "DELETE FROM sign_in_devices WHERE family_id NOT IN
           (SELECT family_id FROM oauth_refresh_tokens)",
    )
    .execute(db)
    .await
    .map_err(Error::internal)?
    .rows_affected())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_folded_and_bounded() {
        assert_eq!(clean("  SovEmu for\tAndroid\n1.4 ").unwrap(), "SovEmu for Android 1.4");
        assert!(clean(" \u{7} ").is_err());
        assert!(clean(&"x".repeat(MAX_NAME + 1)).is_err());
        assert_eq!(clean(&"é".repeat(MAX_NAME)).unwrap().chars().count(), MAX_NAME);
    }
}
