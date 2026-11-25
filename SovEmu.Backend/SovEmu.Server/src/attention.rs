//! Games needing attention, for the admin page of that name: games without game information or
//! box art, possible duplicates (the same title and tags twice in a system), multi-part games
//! with a part missing, and the files the last scan could not place.
//!
//! Each kind is one query over the library as it stands, one system's or all; the library scan
//! keeps the two things only it can see (`games.missing_parts`, `scan_skipped`). This is an
//! admin page read now and then, so it reads the games table rather than keeping counts.

use sqlx::SqlitePool;

/// What needs attention.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// No game information: never looked up, or looked up and not found.
    NoInformation,
    /// No box art: none beside it in the library, and none downloaded.
    NoBoxArt,
    /// The same title with the same tags in two or more files of one system.
    Duplicate,
    /// A multi-part game with a part missing.
    MissingParts,
    /// A file the last scan could not place as a game.
    Unrecognised,
}

impl Kind {
    pub(crate) const ALL: [Self; 5] = [
        Self::NoInformation,
        Self::NoBoxArt,
        Self::Duplicate,
        Self::MissingParts,
        Self::Unrecognised,
    ];

    /// Its games (or files), as `(game id, system, title, path, detail)`: the query takes the
    /// system (or NULL for all) and a limit.
    fn sql(self) -> &'static str {
        match self {
            Self::NoInformation => {
                "SELECT g.id AS game_id, g.system, g.title, g.path,
                        CASE WHEN m.game_id IS NULL THEN 'never looked up' ELSE 'not found' END AS detail
                 FROM games g LEFT JOIN game_metadata m ON m.game_id = g.id
                 WHERE (m.game_id IS NULL OR m.status = 'not_found') AND (?1 IS NULL OR g.system = ?1)
                 ORDER BY g.system, g.title LIMIT ?2"
            }
            Self::NoBoxArt => {
                "SELECT g.id AS game_id, g.system, g.title, g.path, '' AS detail
                 FROM games g LEFT JOIN game_metadata m ON m.game_id = g.id
                 WHERE m.box_art IS NULL AND (?1 IS NULL OR g.system = ?1)
                 ORDER BY g.system, g.title LIMIT ?2"
            }
            Self::Duplicate => {
                "SELECT g.id AS game_id, g.system, g.title, g.path, g.tags AS detail
                 FROM games g
                 WHERE (?1 IS NULL OR g.system = ?1) AND EXISTS (
                   SELECT 1 FROM games d WHERE d.system = g.system AND d.title = g.title
                     COLLATE NOCASE AND d.tags = g.tags AND d.id <> g.id)
                 ORDER BY g.system, g.title, g.path LIMIT ?2"
            }
            Self::MissingParts => {
                "SELECT g.id AS game_id, g.system, g.title, g.path,
                        COALESCE(g.parts, 'disc') || ' ' || g.missing_parts AS detail
                 FROM games g
                 WHERE g.missing_parts <> '' AND (?1 IS NULL OR g.system = ?1)
                 ORDER BY g.system, g.title LIMIT ?2"
            }
            Self::Unrecognised => {
                "SELECT NULL AS game_id, COALESCE(system, '') AS system, '' AS title, path,
                        reason AS detail
                 FROM scan_skipped WHERE (?1 IS NULL OR system = ?1) ORDER BY path LIMIT ?2"
            }
        }
    }
}

/// One game (or file) needing attention.
#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub(crate) struct Item {
    /// None for a file that is not a game.
    pub(crate) game_id: Option<i64>,
    /// Empty for a file in a folder no system uses.
    pub(crate) system: String,
    pub(crate) title: String,
    /// Relative to the library.
    pub(crate) path: String,
    /// More: why it has no information, the tags it shares, the parts missing, why a file was
    /// skipped.
    pub(crate) detail: String,
}

/// The games (or files) of one kind, one system's or all, at most `limit`.
pub(crate) async fn items(
    db: &SqlitePool,
    kind: Kind,
    system: Option<&str>,
    limit: u32,
) -> sqlx::Result<Vec<Item>> {
    sqlx::query_as(kind.sql()).bind(system).bind(limit).fetch_all(db).await
}

/// How many there are of each kind, per system (empty for a folder no system uses).
pub(crate) async fn counts(db: &SqlitePool) -> sqlx::Result<Vec<(Kind, String, i64)>> {
    let mut out = Vec::new();
    for kind in Kind::ALL {
        let per_system: Vec<(String, i64)> = sqlx::query_as(&format!(
            "SELECT system, COUNT(*) FROM ({}) GROUP BY system ORDER BY system",
            kind.sql()
        ))
        .bind(None::<&str>)
        .bind(-1)
        .fetch_all(db)
        .await?;
        out.extend(per_system.into_iter().map(|(system, n)| (kind, system, n)));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn db() -> SqlitePool {
        let dir = std::env::temp_dir().join(format!("sovemu-attention-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        crate::db::open(&dir.join("attention.db")).await.unwrap()
    }

    async fn game(db: &SqlitePool, path: &str, title: &str, tags: &str, missing: &str) -> i64 {
        sqlx::query_scalar(
            "INSERT INTO games (system, path, title, tags, size, modified, missing_parts)
             VALUES ('psx', ?, ?, ?, 1, 0, ?) RETURNING id",
        )
        .bind(path)
        .bind(title)
        .bind(tags)
        .bind(missing)
        .fetch_one(db)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn each_kind_finds_its_games_and_counts_them_per_system() {
        let db = db().await;
        let found = game(&db, "psx/A.chd", "A", "USA", "").await;
        game(&db, "psx/B (USA).chd", "B", "USA", "").await;
        game(&db, "psx/B (USA).cue", "B", "USA", "").await;
        let unknown = game(&db, "psx/C (Disc 1).chd", "C", "", "2").await;
        for (id, status) in [(found, "found"), (unknown, "not_found")] {
            sqlx::query(
                "INSERT INTO game_metadata (game_id, status, box_art) VALUES (?, ?, 'box.webp')",
            )
            .bind(id)
            .bind(status)
            .execute(&db)
            .await
            .unwrap();
        }
        sqlx::query(
            "INSERT INTO scan_skipped (path, system, reason) VALUES ('psx/x.abc', 'psx', 'no')",
        )
        .execute(&db)
        .await
        .unwrap();

        let titles = |items: Vec<Item>| items.into_iter().map(|i| i.title).collect::<Vec<_>>();
        let of = |kind| items(&db, kind, Some("psx"), 50);
        assert_eq!(titles(of(Kind::NoInformation).await.unwrap()), ["B", "B", "C"]);
        assert_eq!(titles(of(Kind::NoBoxArt).await.unwrap()), ["B", "B"]);
        assert_eq!(titles(of(Kind::Duplicate).await.unwrap()), ["B", "B"]);
        let missing = of(Kind::MissingParts).await.unwrap();
        assert_eq!((missing[0].title.as_str(), missing[0].detail.as_str()), ("C", "disc 2"));
        let skipped = of(Kind::Unrecognised).await.unwrap();
        assert_eq!((skipped[0].game_id, skipped[0].path.as_str()), (None, "psx/x.abc"));
        assert!(items(&db, Kind::NoInformation, Some("nes"), 50).await.unwrap().is_empty());
        assert_eq!(items(&db, Kind::NoInformation, None, 1).await.unwrap().len(), 1, "the limit");

        let counts = counts(&db).await.unwrap();
        assert!(counts.contains(&(Kind::NoInformation, "psx".into(), 3)), "{counts:?}");
        assert!(counts.contains(&(Kind::Unrecognised, "psx".into(), 1)));
    }
}
