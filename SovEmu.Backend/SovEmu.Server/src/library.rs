//! Scanning the ROM directory into the `games` table. What is on disk comes from
//! `sovemu_library::scan`; this keeps the table in step with it.

use std::path::Path;

use serde::Serialize;
use sovemu_library::scan::{PartKind, group_discs, walk};
use sqlx::SqlitePool;

#[derive(Debug, Clone, Copy, Default, Serialize, PartialEq, Eq)]
pub(crate) struct ScanReport {
    pub found: usize,
    pub added: usize,
    pub removed: usize,
}

/// Rescans `root` and brings the `games` table in line: new files added, changed ones updated
/// (keeping their ids, so play history and saves stay attached), missing ones removed.
pub(crate) async fn scan(pool: &SqlitePool, root: &Path) -> Result<ScanReport, sqlx::Error> {
    let root = root.to_owned();
    let found =
        tokio::task::spawn_blocking(move || group_discs(walk(&root))).await.unwrap_or_default();
    let mut tx = pool.begin().await?;
    let before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM games").fetch_one(&mut *tx).await?;
    sqlx::query("CREATE TEMP TABLE IF NOT EXISTS seen (path TEXT PRIMARY KEY)")
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM seen").execute(&mut *tx).await?;
    sqlx::query("CREATE TEMP TABLE IF NOT EXISTS seen_patches (path TEXT PRIMARY KEY)")
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM seen_patches").execute(&mut *tx).await?;
    for f in &found {
        sqlx::query(
            "INSERT INTO games (system, path, title, tags, size, modified, media, parts)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(path) DO UPDATE SET system = excluded.system, title = excluded.title,
               tags = excluded.tags, size = excluded.size, modified = excluded.modified,
               media = excluded.media, parts = excluded.parts",
        )
        .bind(f.system)
        .bind(&f.path)
        .bind(&f.title)
        .bind(&f.tags)
        .bind(f.size)
        .bind(f.modified)
        .bind(&f.media)
        .bind(f.parts.map(PartKind::as_str))
        .execute(&mut *tx)
        .await?;
        sqlx::query("INSERT OR IGNORE INTO seen (path) VALUES (?)")
            .bind(&f.path)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM game_discs WHERE game_id = (SELECT id FROM games WHERE path = ?)")
            .bind(&f.path)
            .execute(&mut *tx)
            .await?;
        for (i, disc) in f.discs.iter().enumerate() {
            sqlx::query(
                "INSERT INTO game_discs (game_id, disc, path)
                 SELECT id, ?, ? FROM games WHERE path = ?",
            )
            .bind(i as i64 + 1)
            .bind(disc)
            .bind(&f.path)
            .execute(&mut *tx)
            .await?;
        }
        // Patches keep their ids across scans (by path), so players' choices stay put.
        for patch in &f.patches {
            sqlx::query(
                "INSERT INTO game_patches (game_id, path, name, is_default)
                 SELECT id, ?, ?, ? FROM games WHERE path = ?
                 ON CONFLICT(path) DO UPDATE SET game_id = excluded.game_id,
                   name = excluded.name, is_default = excluded.is_default",
            )
            .bind(&patch.path)
            .bind(&patch.name)
            .bind(patch.is_default)
            .bind(&f.path)
            .execute(&mut *tx)
            .await?;
            sqlx::query("INSERT OR IGNORE INTO seen_patches (path) VALUES (?)")
                .bind(&patch.path)
                .execute(&mut *tx)
                .await?;
        }
    }
    sqlx::query("DELETE FROM game_patches WHERE path NOT IN (SELECT path FROM seen_patches)")
        .execute(&mut *tx)
        .await?;
    let removed = sqlx::query("DELETE FROM games WHERE path NOT IN (SELECT path FROM seen)")
        .execute(&mut *tx)
        .await?
        .rows_affected() as usize;
    let after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM games").fetch_one(&mut *tx).await?;
    tx.commit().await?;
    // Titles came and went: which games share one may have changed.
    crate::releases::refresh(pool).await?;
    let added = (after - before + removed as i64).max(0) as usize;
    Ok(ScanReport { found: found.len(), added, removed })
}
