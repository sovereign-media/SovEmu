//! Release labels (`Game.release`), stored: which games share a title is worked out by
//! `sovemu_library::labels` over the whole library and kept in `games.release`, so listing games
//! only reads it.

use sovemu_library::labels::{Twin, labels};
use sqlx::SqlitePool;

/// Works out every game's label again and stores it (`games.release`): after a scan, after a
/// game-information lookup (titles change), and at startup. Listing games then only reads it.
pub(crate) async fn refresh(db: &SqlitePool) -> sqlx::Result<usize> {
    let rows: Vec<(i64, String, String, String, String, String)> = sqlx::query_as(
        "WITH t AS (
             SELECT g.id, g.system, COALESCE(m.title, g.title) AS title, g.title AS file_title,
                    g.tags, g.path, lower(COALESCE(m.title, g.title)) AS folded
             FROM games g LEFT JOIN game_metadata m ON m.game_id = g.id)
         SELECT id, system, title, file_title, tags, path FROM t
         WHERE (system, folded) IN (
             SELECT system, folded FROM t GROUP BY system, folded HAVING COUNT(*) > 1)",
    )
    .fetch_all(db)
    .await?;
    let twins: Vec<Twin> = rows
        .into_iter()
        .map(|(id, system, title, file_title, tags, path)| Twin {
            id,
            system,
            title,
            file_title,
            tags,
            path,
        })
        .collect();
    let labels = labels(&twins);
    let mut tx = crate::db::write_tx(db).await?;
    sqlx::query("UPDATE games SET release = NULL WHERE release IS NOT NULL")
        .execute(&mut *tx)
        .await?;
    for (id, label) in &labels {
        sqlx::query("UPDATE games SET release = ? WHERE id = ?")
            .bind(label)
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(labels.len())
}
