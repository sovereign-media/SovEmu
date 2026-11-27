//! Games needing attention, for the admin page of that name: games without game information,
//! games waiting to be looked up again, games without box art or a video, downloads that gave
//! up (and why), possible duplicates (the same title and tags twice in a system), multi-part games
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
    /// Queued to be looked up again (`game_metadata.status = 'refresh'`): it has information,
    /// but a lookup is owed, after a source stopped before reaching it. The lookup after each
    /// scan asks it.
    AwaitingLookup,
    /// No box art: none beside it in the library, and none downloaded.
    NoBoxArt,
    /// No gameplay video downloaded (only while videos are downloaded at all): none found, not
    /// downloaded yet, or failing.
    NoVideo,
    /// A picture or video whose download failed
    /// [`MAX_ATTEMPTS`](crate::metadata::artwork::MAX_ATTEMPTS) times, and is no longer tried
    /// (until `retryArtworkDownloads`).
    DownloadGaveUp,
    /// The same title with the same tags in two or more files of one system.
    Duplicate,
    /// A multi-part game with a part missing.
    MissingParts,
    /// A file the last scan could not place as a game.
    Unrecognised,
}

impl Kind {
    pub(crate) const ALL: [Self; 8] = [
        Self::NoInformation,
        Self::AwaitingLookup,
        Self::NoBoxArt,
        Self::NoVideo,
        Self::DownloadGaveUp,
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
            Self::AwaitingLookup => {
                "SELECT g.id AS game_id, g.system, g.title, g.path,
                        'last looked up ' || m.fetched_at AS detail
                 FROM games g JOIN game_metadata m ON m.game_id = g.id
                 WHERE m.status = 'refresh' AND (?1 IS NULL OR g.system = ?1)
                 ORDER BY g.system, g.title LIMIT ?2"
            }
            Self::NoBoxArt => {
                "SELECT g.id AS game_id, g.system, g.title, g.path, '' AS detail
                 FROM games g LEFT JOIN game_metadata m ON m.game_id = g.id
                 WHERE m.box_art IS NULL AND (?1 IS NULL OR g.system = ?1)
                 ORDER BY g.system, g.title LIMIT ?2"
            }
            // The attempts are artwork::MAX_ATTEMPTS (a test keeps them alike).
            Self::NoVideo => {
                "SELECT g.id AS game_id, g.system, g.title, g.path,
                        CASE WHEN m.video_url IS NULL THEN 'none found'
                             WHEN m.video_attempts >= 3
                               THEN 'gave up: ' || COALESCE(m.video_error, 'failed')
                             WHEN m.video_error IS NOT NULL
                               THEN 'failing: ' || m.video_error
                             ELSE 'not downloaded yet' END AS detail
                 FROM games g LEFT JOIN game_metadata m ON m.game_id = g.id
                 WHERE m.video IS NULL AND (?1 IS NULL OR g.system = ?1)
                 ORDER BY g.system, g.title LIMIT ?2"
            }
            Self::DownloadGaveUp => {
                "SELECT g.id AS game_id, g.system, g.title, g.path,
                        RTRIM(
                          CASE WHEN m.box_art IS NULL AND m.box_art_url IS NOT NULL
                                 AND m.box_art_attempts >= 3
                               THEN 'box art: ' || COALESCE(m.box_art_error, 'failed') || char(10)
                               ELSE '' END
                          || CASE WHEN m.screenshot IS NULL AND m.screenshot_url IS NOT NULL
                                   AND m.screenshot_attempts >= 3
                                 THEN 'screenshot: ' || COALESCE(m.screenshot_error, 'failed')
                                      || char(10)
                                 ELSE '' END
                          || CASE WHEN m.marquee IS NULL AND m.marquee_url IS NOT NULL
                                   AND m.marquee_attempts >= 3
                                 THEN 'logo: ' || COALESCE(m.marquee_error, 'failed') || char(10)
                                 ELSE '' END
                          || CASE WHEN m.video IS NULL AND m.video_url IS NOT NULL
                                   AND m.video_attempts >= 3
                                 THEN 'video: ' || COALESCE(m.video_error, 'failed') || char(10)
                                 ELSE '' END,
                          char(10)) AS detail
                 FROM games g JOIN game_metadata m ON m.game_id = g.id
                 WHERE ((m.box_art IS NULL AND m.box_art_url IS NOT NULL
                         AND m.box_art_attempts >= 3)
                     OR (m.screenshot IS NULL AND m.screenshot_url IS NOT NULL
                         AND m.screenshot_attempts >= 3)
                     OR (m.marquee IS NULL AND m.marquee_url IS NOT NULL
                         AND m.marquee_attempts >= 3)
                     OR (m.video IS NULL AND m.video_url IS NOT NULL AND m.video_attempts >= 3))
                   AND (?1 IS NULL OR g.system = ?1)
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

impl Kind {
    /// Whether it is listed: games without a video only while videos are downloaded
    /// (`SovEmu__Metadata__Videos`), or every game would be.
    fn listed(self, videos: bool) -> bool {
        self != Self::NoVideo || videos
    }
}

/// The games (or files) of one kind, one system's or all, at most `limit`. `videos`: whether
/// gameplay videos are downloaded.
pub(crate) async fn items(
    db: &SqlitePool,
    kind: Kind,
    system: Option<&str>,
    limit: u32,
    videos: bool,
) -> sqlx::Result<Vec<Item>> {
    if !kind.listed(videos) {
        return Ok(Vec::new());
    }
    sqlx::query_as(kind.sql()).bind(system).bind(limit).fetch_all(db).await
}

/// How many there are of each kind, per system (empty for a folder no system uses).
pub(crate) async fn counts(
    db: &SqlitePool,
    videos: bool,
) -> sqlx::Result<Vec<(Kind, String, i64)>> {
    let mut out = Vec::new();
    for kind in Kind::ALL.into_iter().filter(|k| k.listed(videos)) {
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

    /// A database of its own for each test (`name`), as `cargo test` runs them in one process.
    async fn db(name: &str) -> SqlitePool {
        let dir =
            std::env::temp_dir().join(format!("sovemu-attention-{name}-{}", std::process::id()));
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
        let db = db("kinds").await;
        let found = game(&db, "psx/A.chd", "A", "USA", "").await;
        game(&db, "psx/B (USA).chd", "B", "USA", "").await;
        game(&db, "psx/B (USA).cue", "B", "USA", "").await;
        let unknown = game(&db, "psx/C (Disc 1).chd", "C", "", "2").await;
        let queued = game(&db, "psx/D.chd", "D", "", "").await;
        for (id, status) in [(found, "found"), (unknown, "not_found"), (queued, "refresh")] {
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
        let of = |kind| items(&db, kind, Some("psx"), 50, true);
        assert_eq!(titles(of(Kind::NoInformation).await.unwrap()), ["B", "B", "C"]);
        let waiting = of(Kind::AwaitingLookup).await.unwrap();
        assert_eq!(titles(waiting.clone()), ["D"], "queued, not without information");
        assert!(waiting[0].detail.starts_with("last looked up "), "{}", waiting[0].detail);
        assert_eq!(titles(of(Kind::NoBoxArt).await.unwrap()), ["B", "B"]);
        assert_eq!(titles(of(Kind::Duplicate).await.unwrap()), ["B", "B"]);
        let missing = of(Kind::MissingParts).await.unwrap();
        assert_eq!((missing[0].title.as_str(), missing[0].detail.as_str()), ("C", "disc 2"));
        let skipped = of(Kind::Unrecognised).await.unwrap();
        assert_eq!((skipped[0].game_id, skipped[0].path.as_str()), (None, "psx/x.abc"));
        assert!(items(&db, Kind::NoInformation, Some("nes"), 50, true).await.unwrap().is_empty());
        assert_eq!(
            items(&db, Kind::NoInformation, None, 1, true).await.unwrap().len(),
            1,
            "the limit"
        );

        let counts = counts(&db, true).await.unwrap();
        assert!(counts.contains(&(Kind::NoInformation, "psx".into(), 3)), "{counts:?}");
        assert!(counts.contains(&(Kind::Unrecognised, "psx".into(), 1)));
    }

    #[test]
    fn gave_up_is_the_download_jobs_attempts() {
        assert_eq!(crate::metadata::artwork::MAX_ATTEMPTS, 3, "the attention queries say 3");
    }

    #[tokio::test]
    async fn videos_and_downloads_that_gave_up_say_why() {
        let db = db("downloads").await;
        let none = game(&db, "psx/A.chd", "A", "", "").await;
        let waiting = game(&db, "psx/B.chd", "B", "", "").await;
        let failing = game(&db, "psx/C.chd", "C", "", "").await;
        let gave_up = game(&db, "psx/D.chd", "D", "", "").await;
        let has = game(&db, "psx/E.chd", "E", "", "").await;
        for (id, url, attempts, error, file) in [
            (none, None, 0, None, None),
            (waiting, Some("v"), 0, None, None),
            (failing, Some("v"), 1, Some("refused (HTTP 430): quota"), None),
            (gave_up, Some("v"), 3, Some("not an MP4 video: over quota"), None),
            (has, Some("v"), 0, None, Some("5/video.mp4")),
        ] {
            sqlx::query(
                "INSERT INTO game_metadata (game_id, status, video_url, video_attempts,
                   video_error, video) VALUES (?, 'found', ?, ?, ?, ?)",
            )
            .bind(id)
            .bind(url)
            .bind(attempts)
            .bind(error)
            .bind(file)
            .execute(&db)
            .await
            .unwrap();
        }
        // D's box art gave up too; A's has attempts left.
        sqlx::query(
            "UPDATE game_metadata SET box_art_url = 'b', box_art_attempts = 3,
               box_art_error = 'no answer (timed out)' WHERE game_id = ?",
        )
        .bind(gave_up)
        .execute(&db)
        .await
        .unwrap();
        sqlx::query(
            "UPDATE game_metadata SET box_art_url = 'b', box_art_attempts = 2 WHERE game_id = ?",
        )
        .bind(none)
        .execute(&db)
        .await
        .unwrap();

        let of = |kind, videos| items(&db, kind, Some("psx"), 50, videos);
        let details =
            |items: Vec<Item>| items.into_iter().map(|i| (i.title, i.detail)).collect::<Vec<_>>();
        assert_eq!(
            details(of(Kind::NoVideo, true).await.unwrap()),
            [
                ("A".into(), "none found".into()),
                ("B".into(), "not downloaded yet".into()),
                ("C".into(), "failing: refused (HTTP 430): quota".into()),
                ("D".into(), "gave up: not an MP4 video: over quota".into()),
            ] as [(String, String); 4]
        );
        assert!(of(Kind::NoVideo, false).await.unwrap().is_empty(), "videos off: not listed");
        assert_eq!(
            details(of(Kind::DownloadGaveUp, true).await.unwrap()),
            [(
                "D".into(),
                "box art: no answer (timed out)\nvideo: not an MP4 video: over quota".into()
            )] as [(String, String); 1]
        );
        let kinds =
            |c: Vec<(Kind, String, i64)>| c.into_iter().map(|(k, ..)| k).collect::<Vec<_>>();
        assert!(kinds(counts(&db, true).await.unwrap()).contains(&Kind::NoVideo));
        assert!(!kinds(counts(&db, false).await.unwrap()).contains(&Kind::NoVideo));
    }
}
