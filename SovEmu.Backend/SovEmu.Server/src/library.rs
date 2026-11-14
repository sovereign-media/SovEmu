//! Scanning the ROM directory into the `games` table.

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::Serialize;
use sqlx::SqlitePool;

use crate::systems::{self, System};

/// How deep below a system folder to look (`nes/Europe/Game.nes` is depth 2).
const MAX_DEPTH: usize = 4;

/// One ROM found on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub system: &'static str,
    /// Relative to the ROM directory, with `/` separators.
    pub path: String,
    pub title: String,
    pub tags: String,
    pub size: i64,
    pub modified: i64,
}

#[derive(Debug, Clone, Copy, Default, Serialize, PartialEq, Eq)]
pub struct ScanReport {
    pub found: usize,
    pub added: usize,
    pub removed: usize,
}

/// Walks `root` and returns every file that belongs to a known system.
pub fn walk(root: &Path) -> Vec<Found> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else { return found };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            walk_dir(root, &path, systems::by_folder(&name), 1, &mut found);
        } else if let Some(f) = classify(root, &path, None) {
            found.push(f);
        }
    }
    found.sort_by(|a, b| (a.system, &a.title, &a.path).cmp(&(b.system, &b.title, &b.path)));
    found
}

fn walk_dir(
    root: &Path,
    dir: &Path,
    system: Option<&'static System>,
    depth: usize,
    found: &mut Vec<Found>,
) {
    if depth > MAX_DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        if path.is_dir() {
            walk_dir(root, &path, system, depth + 1, found);
        } else if let Some(f) = classify(root, &path, system) {
            found.push(f);
        }
    }
}

/// A file's system: its folder's, if the extension fits it; otherwise its extension's.
fn classify(root: &Path, path: &Path, folder: Option<&'static System>) -> Option<Found> {
    let ext = path.extension()?.to_string_lossy().to_ascii_lowercase();
    let system = match folder {
        Some(s) if s.extensions.contains(&ext.as_str()) => s,
        Some(_) => return None,
        None => systems::by_extension(&ext)?,
    };
    let meta = std::fs::metadata(path).ok()?;
    let relative: PathBuf = path.strip_prefix(root).ok()?.to_path_buf();
    let stem = path.file_stem()?.to_string_lossy();
    let (title, tags) = systems::parse_title(&stem);
    Some(Found {
        system: system.id,
        path: relative
            .components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/"),
        title,
        tags,
        size: meta.len() as i64,
        modified: meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs() as i64),
    })
}

/// Rescans `root` and brings the `games` table in line: new files added, changed ones updated
/// (keeping their ids, so play history and saves stay attached), missing ones removed.
pub async fn scan(pool: &SqlitePool, root: &Path) -> Result<ScanReport, sqlx::Error> {
    let root = root.to_owned();
    let found = tokio::task::spawn_blocking(move || walk(&root)).await.unwrap_or_default();
    let mut tx = pool.begin().await?;
    let before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM games").fetch_one(&mut *tx).await?;
    sqlx::query("CREATE TEMP TABLE IF NOT EXISTS seen (path TEXT PRIMARY KEY)")
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM seen").execute(&mut *tx).await?;
    for f in &found {
        sqlx::query(
            "INSERT INTO games (system, path, title, tags, size, modified) VALUES (?, ?, ?, ?, ?, ?)
             ON CONFLICT(path) DO UPDATE SET system = excluded.system, title = excluded.title,
               tags = excluded.tags, size = excluded.size, modified = excluded.modified",
        )
        .bind(f.system)
        .bind(&f.path)
        .bind(&f.title)
        .bind(&f.tags)
        .bind(f.size)
        .bind(f.modified)
        .execute(&mut *tx)
        .await?;
        sqlx::query("INSERT OR IGNORE INTO seen (path) VALUES (?)")
            .bind(&f.path)
            .execute(&mut *tx)
            .await?;
    }
    let removed = sqlx::query("DELETE FROM games WHERE path NOT IN (SELECT path FROM seen)")
        .execute(&mut *tx)
        .await?
        .rows_affected() as usize;
    let after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM games").fetch_one(&mut *tx).await?;
    tx.commit().await?;
    let added = (after - before + removed as i64).max(0) as usize;
    Ok(ScanReport { found: found.len(), added, removed })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn library() -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "sovemu-lib-{}-{}",
            std::process::id(),
            rand_suffix()
        ));
        for (path, bytes) in [
            ("nes/Super Game (USA).nes", 16usize),
            ("nes/Hacks/Other Game, The (Europe) [h].nes", 8),
            ("nes/readme.txt", 4),
            ("nes/wrong.sfc", 4),
            ("Genesis/Sonic Clone (World).md", 32),
            ("loose/Puzzle (Japan).gbc", 8),
            ("loose/mystery.bin", 8),
            (".hidden/game.nes", 8),
            ("psx/Disc.cue", 8),
        ] {
            let p = root.join(path);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, vec![0u8; bytes]).unwrap();
        }
        root
    }

    fn rand_suffix() -> u128 {
        std::time::SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
    }

    #[test]
    fn walks_folders_aliases_and_loose_files() {
        let root = library();
        let found = walk(&root);
        let summary: Vec<_> =
            found.iter().map(|f| (f.system, f.path.as_str(), f.title.as_str())).collect();
        assert_eq!(
            summary,
            [
                ("gbc", "loose/Puzzle (Japan).gbc", "Puzzle"),
                ("megadrive", "Genesis/Sonic Clone (World).md", "Sonic Clone"),
                ("nes", "nes/Super Game (USA).nes", "Super Game"),
                ("nes", "nes/Hacks/Other Game, The (Europe) [h].nes", "The Other Game"),
            ]
        );
        assert_eq!(found[3].tags, "Europe, h");
        assert_eq!(found[2].size, 16);
        std::fs::remove_dir_all(root).ok();
    }
}
