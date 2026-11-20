//! Background jobs: what the server does on its own, on intervals set in the environment.
//!
//! - Library: rescans (`SOVEMU_SCAN_INTERVAL_MINUTES`) and, in `mirror` mode, syncs
//!   (`SOVEMU_MIRROR_INTERVAL_MINUTES`).
//! - Maintenance (`SOVEMU_MAINTENANCE_INTERVAL_MINUTES`): expired sign-ins deleted, the query
//!   planner's statistics refreshed (`PRAGMA optimize`) and the write-ahead log folded back into
//!   the database, so neither grows without end.
//!
//! The first run of each is one interval after startup (startup does its own scan).

use std::time::Duration;

use sqlx::SqlitePool;

use crate::{AppState, LibraryMode, admin};

/// Starts every background job.
pub(crate) fn spawn(state: &AppState) {
    spawn_library(state);
    spawn_maintenance(state);
}

fn every(minutes: u64) -> Option<Duration> {
    (minutes > 0).then(|| Duration::from_secs(minutes * 60))
}

/// Rescans (and in `mirror` mode syncs) on the configured intervals, so the library keeps up
/// with the share on its own.
fn spawn_library(state: &AppState) {
    if let Some(period) = every(state.config.scan_interval_minutes) {
        let state = state.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(period);
            tick.tick().await;
            loop {
                tick.tick().await;
                admin::scan_and_log(&state).await;
                admin::after_scan(&state).await;
            }
        });
    }
    if state.config.library_mode == LibraryMode::Mirror
        && let Some(period) = every(state.config.mirror_interval_minutes)
    {
        let state = state.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(period);
            tick.tick().await;
            loop {
                tick.tick().await;
                admin::sync_then_scan(state.clone()).await;
                admin::after_scan(&state).await;
            }
        });
    }
}

fn spawn_maintenance(state: &AppState) {
    let Some(period) = every(state.config.maintenance_interval_minutes) else { return };
    let db = state.db.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(period);
        tick.tick().await;
        loop {
            tick.tick().await;
            if let Err(e) = maintain(&db).await {
                tracing::warn!(error = %e, "database maintenance");
            }
        }
    });
}

/// One round of upkeep. Returns how many expired sign-ins it deleted.
pub(crate) async fn maintain(db: &SqlitePool) -> sqlx::Result<u64> {
    let expired = sqlx::query("DELETE FROM auth_sessions WHERE expires_at <= datetime('now')")
        .execute(db)
        .await?
        .rows_affected();
    let expired = expired + crate::oidc::tokens::expire(db).await?;
    sqlx::query("PRAGMA optimize").execute(db).await?;
    // PASSIVE: never waits on a reader or writer; what it cannot fold back now waits for the
    // next round.
    sqlx::query("PRAGMA wal_checkpoint(PASSIVE)").execute(db).await?;
    tracing::debug!(expired_sessions = expired, "database maintenance");
    Ok(expired)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn maintenance_deletes_only_expired_sign_ins() {
        let dir = std::env::temp_dir().join(format!("sovemu-jobs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = crate::db::open(&dir.join("jobs.db")).await.unwrap();
        sqlx::query("INSERT INTO users (username, password_hash) VALUES ('a', 'x')")
            .execute(&db)
            .await
            .unwrap();
        for (token, expires) in [("old", "-1 day"), ("live", "+1 day")] {
            sqlx::query(
                "INSERT INTO auth_sessions (token_hash, user_id, expires_at)
                 VALUES (?, 1, datetime('now', ?))",
            )
            .bind(token)
            .bind(expires)
            .execute(&db)
            .await
            .unwrap();
        }
        // Two apps' sign-ins: one whose only token has expired, one still current.
        for (grant, token, expires) in [(1, "old-app", "-1 minute"), (2, "live-app", "+1 hour")] {
            sqlx::query(
                "INSERT INTO oauth_grants (id, client_id, user_id, scope, auth_time)
                 VALUES (?, 'sovemu-native', 1, '', 0)",
            )
            .bind(grant)
            .execute(&db)
            .await
            .unwrap();
            sqlx::query(
                "INSERT INTO oauth_tokens (token_hash, grant_id, kind, expires_at)
                 VALUES (?, ?, 'access', datetime('now', ?))",
            )
            .bind(token)
            .bind(grant)
            .bind(expires)
            .execute(&db)
            .await
            .unwrap();
        }
        assert_eq!(maintain(&db).await.unwrap(), 3, "a session, a token and its sign-in");
        let left: Vec<String> = sqlx::query_scalar("SELECT token_hash FROM auth_sessions")
            .fetch_all(&db)
            .await
            .unwrap();
        assert_eq!(left, ["live"]);
        let grants: Vec<i64> =
            sqlx::query_scalar("SELECT id FROM oauth_grants").fetch_all(&db).await.unwrap();
        assert_eq!(grants, [2]);
        assert_eq!(maintain(&db).await.unwrap(), 0, "nothing left to delete");
    }
}
