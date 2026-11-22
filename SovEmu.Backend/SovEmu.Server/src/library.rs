//! Scanning the ROM directory into the `games` table. What is on disk comes from
//! `sovemu_library::scan`; this keeps the table in step with it.

use std::path::Path;

use anyhow::Context as _;
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
///
/// Removing a game removes everyone's history, favourites, collection entries, cheats and patch
/// choices for it, so a library that cannot be read is an error, never an empty library. And
/// one that reads as empty while games are known is refused: an unmounted share is an empty
/// folder.
pub(crate) async fn scan(pool: &SqlitePool, root: &Path) -> anyhow::Result<ScanReport> {
    let dir = root.to_owned();
    let walked = tokio::task::spawn_blocking(move || walk(&dir))
        .await
        .context("the library walk stopped")?
        .with_context(|| format!("reading the library at {}", root.display()))?;
    let found = group_discs(walked.found);
    let mut tx = crate::db::write_tx(pool).await?;
    // What the walk could not place, for the admin: this scan's, replacing the last one's.
    sqlx::query("DELETE FROM scan_skipped").execute(&mut *tx).await?;
    for s in &walked.skipped {
        sqlx::query("INSERT OR IGNORE INTO scan_skipped (path, system, reason) VALUES (?, ?, ?)")
            .bind(&s.path)
            .bind(s.system)
            .bind(&s.reason)
            .execute(&mut *tx)
            .await?;
    }
    let before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM games").fetch_one(&mut *tx).await?;
    if found.is_empty() && before > 0 {
        anyhow::bail!(
            "the library at {} has no games, but {before} are known: not removing them all \
             (is the share mounted?)",
            root.display()
        );
    }
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
            "INSERT INTO games
               (system, path, title, tags, size, modified, media, parts, missing_parts)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(path) DO UPDATE SET system = excluded.system, title = excluded.title,
               tags = excluded.tags, size = excluded.size, modified = excluded.modified,
               media = excluded.media, parts = excluded.parts,
               missing_parts = excluded.missing_parts",
        )
        .bind(f.system)
        .bind(&f.path)
        .bind(&f.title)
        .bind(&f.tags)
        .bind(f.size)
        .bind(f.modified)
        .bind(&f.media)
        .bind(f.parts.map(PartKind::as_str))
        .bind(f.missing_parts.iter().map(u32::to_string).collect::<Vec<_>>().join(","))
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

/// Games per system, counted when the library changes (after each scan, and at startup), not on
/// each request that lists the systems.
#[derive(Debug, Default)]
pub(crate) struct GameCounts(
    std::sync::RwLock<std::sync::Arc<std::collections::BTreeMap<String, i64>>>,
);

impl GameCounts {
    pub(crate) async fn load(pool: &SqlitePool) -> sqlx::Result<Self> {
        let counts = Self::default();
        counts.refresh(pool).await?;
        Ok(counts)
    }

    /// Counts again.
    pub(crate) async fn refresh(&self, pool: &SqlitePool) -> sqlx::Result<()> {
        let rows: Vec<(String, i64)> =
            sqlx::query_as("SELECT system, COUNT(*) FROM games GROUP BY system")
                .fetch_all(pool)
                .await?;
        *self.0.write().unwrap_or_else(|e| e.into_inner()) =
            std::sync::Arc::new(rows.into_iter().collect());
        Ok(())
    }

    /// Each system's games, as last counted.
    pub(crate) fn get(&self) -> std::sync::Arc<std::collections::BTreeMap<String, i64>> {
        self.0.read().unwrap_or_else(|e| e.into_inner()).clone()
    }
}
