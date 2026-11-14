//! Scanning the ROM directory into the `games` table.

use std::collections::HashSet;
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
    /// Artwork found beside it, e.g. `"image,thumb,video"` (see [`MEDIA_KINDS`]).
    pub media: String,
}

/// The artwork kinds SovEmu shows, as named in Batocera/EmulationStation libraries:
/// `images/<rom stem>-<kind>.<png|jpg>` and `videos/<rom stem>-video.mp4`.
pub const MEDIA_KINDS: &[&str] = &["image", "thumb", "marquee", "video"];

/// Folders that hold artwork, not games; never scanned for ROMs.
const MEDIA_DIRS: &[&str] =
    &["images", "videos", "manuals", "media", "downloaded_images", "downloaded_videos"];

/// Where a game's artwork of `kind` would be, relative to the ROM's folder.
pub fn media_candidates(stem: &str, kind: &str) -> Vec<String> {
    if kind == "video" {
        vec![format!("videos/{stem}-video.mp4")]
    } else {
        vec![format!("images/{stem}-{kind}.png"), format!("images/{stem}-{kind}.jpg")]
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, PartialEq, Eq)]
pub struct ScanReport {
    pub found: usize,
    pub added: usize,
    pub removed: usize,
}

/// A ROM found while listing, before its size and date are read.
struct Candidate {
    path: PathBuf,
    system: &'static System,
    media: String,
}

/// Threads reading file sizes and dates: on a network library each is a round trip, so they
/// run side by side.
const STAT_THREADS: usize = 16;

/// Walks `root` and returns every file that belongs to a known system.
///
/// Two stages, both parallel, because on an SMB/NFS library the cost is round trips: list every
/// folder (one thread per top-level folder, no per-file requests), then read the size and date
/// of only the files that are ROMs.
pub fn walk(root: &Path) -> Vec<Found> {
    let Ok(entries) = std::fs::read_dir(root) else { return Vec::new() };
    let mut candidates = Vec::new();
    let mut folders = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        if is_dir(&entry) {
            folders.push((path, systems::by_folder(&name)));
        } else if let Some(system) = pick_system(&path, None) {
            candidates.push(Candidate { path, system, media: String::new() });
        }
    }
    std::thread::scope(|scope| {
        let handles: Vec<_> = folders
            .iter()
            .map(|(dir, system)| {
                scope.spawn(move || {
                    let mut found = Vec::new();
                    walk_dir(dir, *system, 1, &mut found);
                    found
                })
            })
            .collect();
        for handle in handles {
            candidates.extend(handle.join().unwrap_or_default());
        }
    });

    let chunk = candidates.len().div_ceil(STAT_THREADS).max(1);
    let mut found: Vec<Found> = std::thread::scope(|scope| {
        let handles: Vec<_> = candidates
            .chunks(chunk)
            .map(|part| {
                scope.spawn(move || {
                    part.iter().filter_map(|c| to_found(root, c)).collect::<Vec<_>>()
                })
            })
            .collect();
        handles.into_iter().flat_map(|h| h.join().unwrap_or_default()).collect()
    });
    found.sort_by(|a, b| (a.system, &a.title, &a.path).cmp(&(b.system, &b.title, &b.path)));
    found
}

fn walk_dir(dir: &Path, system: Option<&'static System>, depth: usize, found: &mut Vec<Candidate>) {
    if depth > MAX_DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    // List the artwork folders once per directory rather than probing per game.
    let mut media = HashSet::new();
    for sub in ["images", "videos"] {
        if let Ok(files) = std::fs::read_dir(dir.join(sub)) {
            media.extend(
                files.flatten().map(|f| format!("{sub}/{}", f.file_name().to_string_lossy())),
            );
        }
    }
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        if is_dir(&entry) {
            if !MEDIA_DIRS.contains(&name.to_ascii_lowercase().as_str()) {
                walk_dir(&path, system, depth + 1, found);
            }
        } else if let Some(sys) = pick_system(&path, system) {
            let stem =
                path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let media = MEDIA_KINDS
                .iter()
                .filter(|kind| media_candidates(&stem, kind).iter().any(|c| media.contains(c)))
                .copied()
                .collect::<Vec<_>>()
                .join(",");
            found.push(Candidate { path, system: sys, media });
        }
    }
}

/// Whether a directory entry is a directory, from the listing itself where the OS provides it:
/// a `stat` per entry is a network round trip on an SMB/NFS library, and artwork folders hold
/// thousands of files. Symlinks are followed.
fn is_dir(entry: &std::fs::DirEntry) -> bool {
    match entry.file_type() {
        Ok(t) if t.is_symlink() => entry.path().is_dir(),
        Ok(t) => t.is_dir(),
        Err(_) => entry.path().is_dir(),
    }
}

/// A file's system: its folder's, if the extension fits it; otherwise its extension's.
fn pick_system(path: &Path, folder: Option<&'static System>) -> Option<&'static System> {
    let ext = path.extension()?.to_string_lossy().to_ascii_lowercase();
    match folder {
        Some(s) if s.extensions.contains(&ext.as_str()) => Some(s),
        Some(_) => None,
        None => systems::by_extension(&ext),
    }
}

/// Reads a candidate's size and date and parses its title.
fn to_found(root: &Path, c: &Candidate) -> Option<Found> {
    let meta = std::fs::metadata(&c.path).ok()?;
    let relative: PathBuf = c.path.strip_prefix(root).ok()?.to_path_buf();
    let stem = c.path.file_stem()?.to_string_lossy();
    let (title, tags) = systems::parse_title(&stem);
    Some(Found {
        system: c.system.id,
        path: relative
            .components()
            .map(|p| p.as_os_str().to_string_lossy())
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
        media: c.media.clone(),
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
            "INSERT INTO games (system, path, title, tags, size, modified, media)
             VALUES (?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(path) DO UPDATE SET system = excluded.system, title = excluded.title,
               tags = excluded.tags, size = excluded.size, modified = excluded.modified,
               media = excluded.media",
        )
        .bind(f.system)
        .bind(&f.path)
        .bind(&f.title)
        .bind(&f.tags)
        .bind(f.size)
        .bind(f.modified)
        .bind(&f.media)
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
            ("nes/images/Super Game (USA)-image.png", 4),
            ("nes/images/Super Game (USA)-marquee.jpg", 4),
            ("nes/videos/Super Game (USA)-video.mp4", 4),
            ("nes/images/Stray.nes", 4),
            ("nes/Hacks/Other Game, The (Europe) [h].nes", 8),
            ("nes/readme.txt", 4),
            ("nes/wrong.sfc", 4),
            ("Genesis/Sonic Clone (World).md", 32),
            ("loose/Puzzle (Japan).gbc", 8),
            ("loose/mystery.bin", 8),
            (".hidden/game.nes", 8),
            ("n64/Game.z64", 8),
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
        assert_eq!(found[2].media, "image,marquee,video", "artwork beside the ROM");
        assert_eq!(found[3].media, "", "none for this one");
        assert_eq!(found[2].size, 16);
        std::fs::remove_dir_all(root).ok();
    }
}
