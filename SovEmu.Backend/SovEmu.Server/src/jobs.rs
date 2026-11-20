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
    let (db, identity) = (state.db.clone(), state.identity.clone());
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(period);
        tick.tick().await;
        loop {
            tick.tick().await;
            if let Err(e) = maintain(&db, &identity).await {
                tracing::warn!(error = %e, "database maintenance");
            }
        }
    });
}

/// One round of upkeep: expired sign-ins, codes and tokens deleted (`sovemu_identity`), the query
/// planner's statistics refreshed, the write-ahead log folded back. How many records expired.
pub(crate) async fn maintain(
    db: &SqlitePool,
    identity: &sovemu_identity::Identity,
) -> anyhow::Result<u64> {
    let expired = identity.maintain().await?;
    sqlx::query("PRAGMA optimize").execute(db).await?;
    // PASSIVE: never waits on a reader or writer; what it cannot fold back now waits for the
    // next round.
    sqlx::query("PRAGMA wal_checkpoint(PASSIVE)").execute(db).await?;
    tracing::debug!(expired, "database maintenance");
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
        let config = sovemu_identity::Config {
            public_urls: vec!["http://localhost:8080".into()],
            token_secret: "test".into(),
            access_token_minutes: 60,
            session_days: 30,
            secure_cookies: false,
            password_sign_in: true,
            max_failures: 5,
            max_failures_per_ip: 20,
            lockout: std::time::Duration::from_secs(300),
            refresh_retry: std::time::Duration::ZERO,
        };
        let identity = sovemu_identity::Identity::new(db.clone(), config).await.unwrap();
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
        assert_eq!(maintain(&db, &identity).await.unwrap(), 1);
        let left: Vec<String> = sqlx::query_scalar("SELECT token_hash FROM auth_sessions")
            .fetch_all(&db)
            .await
            .unwrap();
        assert_eq!(left, ["live"]);
        assert_eq!(maintain(&db, &identity).await.unwrap(), 0, "nothing left to delete");
        std::fs::remove_dir_all(&dir).ok();
    }
}
