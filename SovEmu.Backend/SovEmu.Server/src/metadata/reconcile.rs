//! The game information and artwork kept for the library, reconciled with the library and with
//! what is on disk ([`Job::Reconcile`](crate::jobs::Job::Reconcile)), after every scan and when
//! an admin asks:
//!
//! - a game no longer in the library loses its information (the scan deletes its row, and the
//!   row's `ON DELETE CASCADE` takes the information; any left over goes here), its downloaded
//!   pictures and video (its folder, `artwork/games/<id>/`), and its copies at each width
//!   (`cache/artwork/games/<id>/`);
//! - a downloaded file that has gone (deleted by hand, a volume restored without it) is forgotten,
//!   so the download job fetches it again;
//! - games with no information yet are counted; the lookup that follows asks for them, and ends
//!   by downloading what is missing.
//!
//! Only SovEmu's own files are deleted: numbered game folders, and in them
//! `<box|shot|marquee>.<png|jpg|webp>` and `video.mp4`.
//! A file no row names but whose game still exists may be a download being recorded, so it is
//! only deleted while no downloads run.

use std::collections::HashSet;
use std::path::PathBuf;

use crate::AppState;

/// What one run did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Report {
    /// Information of games no longer in the library, deleted.
    pub(crate) rows: u64,
    /// Downloaded files no game refers to, deleted.
    pub(crate) files: u64,
    /// Games whose copies at each width were deleted with them.
    pub(crate) copies: u64,
    /// Downloaded files that had gone, to be downloaded again.
    pub(crate) missing: u64,
    /// Games with no information yet.
    pub(crate) unknown: u64,
}

impl std::fmt::Display for Report {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} removed games' information, {} files and {} games' resized copies deleted; \
             {} missing files to download again; {} games without information",
            self.rows, self.files, self.copies, self.missing, self.unknown
        )
    }
}

/// The columns that name a downloaded file, and the statement that forgets one (its attempts
/// start again) so the download job fetches it again.
const FILE_COLUMNS: [(&str, &str); 4] = [
    (
        "box_art",
        "UPDATE game_metadata SET box_art = NULL, box_art_attempts = 0
         WHERE game_id = ? AND box_art = ?",
    ),
    (
        "screenshot",
        "UPDATE game_metadata SET screenshot = NULL, screenshot_attempts = 0
         WHERE game_id = ? AND screenshot = ?",
    ),
    (
        "marquee",
        "UPDATE game_metadata SET marquee = NULL, marquee_attempts = 0
         WHERE game_id = ? AND marquee = ?",
    ),
    (
        "video",
        "UPDATE game_metadata SET video = NULL, video_attempts = 0
         WHERE game_id = ? AND video = ?",
    ),
];

type FileRow = (i64, Option<String>, Option<String>, Option<String>, Option<String>);

/// One run (see the module's notes).
pub(crate) async fn reconcile(state: &AppState) -> anyhow::Result<Report> {
    let db = &state.db;
    let media = state.metadata.media_dir.clone();
    let mut report = Report {
        rows: sqlx::query("DELETE FROM game_metadata WHERE game_id NOT IN (SELECT id FROM games)")
            .execute(db)
            .await?
            .rows_affected(),
        ..Report::default()
    };

    // Files the table names that are not on disk.
    let rows: Vec<FileRow> = sqlx::query_as(
        "SELECT game_id, box_art, screenshot, marquee, video FROM game_metadata
         WHERE box_art IS NOT NULL OR screenshot IS NOT NULL OR marquee IS NOT NULL
           OR video IS NOT NULL",
    )
    .fetch_all(db)
    .await?;
    let named: Vec<(i64, usize, String)> = rows
        .into_iter()
        .flat_map(|(id, a, b, c, d)| {
            [a, b, c, d].into_iter().enumerate().filter_map(move |(i, f)| Some((id, i, f?)))
        })
        .collect();
    let dir = media.clone();
    let (named, missing) = tokio::task::spawn_blocking(move || {
        named.into_iter().partition::<Vec<_>, _>(|(_, _, name)| dir.join(name).is_file())
    })
    .await?;
    for (id, column, name) in &missing {
        report.missing += sqlx::query(FILE_COLUMNS[*column].1)
            .bind(id)
            .bind(name)
            .execute(db)
            .await?
            .rows_affected();
    }
    if report.missing > 0 {
        tracing::info!(files = report.missing, "downloaded files gone: downloading them again");
    }

    // Files on disk no row names.
    let games: HashSet<i64> =
        sqlx::query_scalar("SELECT id FROM games").fetch_all(db).await?.into_iter().collect();
    let kept: HashSet<String> = named.into_iter().map(|(_, _, name)| name).collect();
    let downloading = state.metadata.artwork_status().running;
    // A folder per game: a removed game's goes whole, a kept game's loses what no row names.
    if let Ok(mut folders) = tokio::fs::read_dir(&media).await {
        while let Some(folder) = folders.next_entry().await? {
            let Ok(id) = folder.file_name().to_string_lossy().parse::<i64>() else { continue };
            if !folder.file_type().await?.is_dir() {
                continue;
            }
            let Ok(mut files) = tokio::fs::read_dir(folder.path()).await else { continue };
            let removed_game = !games.contains(&id);
            let mut left = 0;
            while let Some(file) = files.next_entry().await? {
                let name = file.file_name().to_string_lossy().into_owned();
                if !super::is_game_file(&name) || kept.contains(&format!("{id}/{name}")) {
                    left += 1;
                    continue;
                }
                if (removed_game || !downloading)
                    && tokio::fs::remove_file(file.path()).await.is_ok()
                {
                    report.files += 1;
                } else {
                    left += 1;
                }
            }
            if left == 0 {
                let _ = tokio::fs::remove_dir(folder.path()).await;
            }
        }
    }

    // Copies at each width of games no longer in the library.
    let widths: PathBuf = state.config.paths().artwork_widths().join("games");
    if let Ok(mut entries) = tokio::fs::read_dir(&widths).await {
        while let Some(entry) = entries.next_entry().await? {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Ok(id) = name.parse::<i64>() else { continue };
            if !games.contains(&id) && tokio::fs::remove_dir_all(entry.path()).await.is_ok() {
                report.copies += 1;
            }
        }
    }

    report.unknown = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM games g LEFT JOIN game_metadata m ON m.game_id = g.id
         WHERE m.game_id IS NULL",
    )
    .fetch_one(db)
    .await? as u64;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_file_column_has_its_statement() {
        for (column, sql) in FILE_COLUMNS {
            assert!(sql.contains(&format!("SET {column} = NULL, {column}_attempts = 0")), "{sql}");
        }
    }
}
