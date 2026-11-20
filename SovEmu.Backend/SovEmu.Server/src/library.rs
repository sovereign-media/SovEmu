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
    /// A multi-disc game's discs in order (disc 1's is [`Found::path`]); empty for one disc.
    pub discs: Vec<String>,
    /// Soft patches found with it (see [`PATCH_EXTENSIONS`]).
    pub patches: Vec<FoundPatch>,
}

/// A soft patch for a game: beside it with its name (`Game.bps`, applied by default, as
/// RetroArch does; named "BPS patch"), or in a folder named as it (`Game/English v1.1.bps`,
/// named after its file, to choose).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundPatch {
    /// Relative to the ROM directory, with `/` separators.
    pub path: String,
    pub name: String,
    pub is_default: bool,
}

/// Patch formats, in the order one is preferred when a game has several beside it.
pub const PATCH_EXTENSIONS: &[&str] = &["bps", "ups", "ips"];

/// Disc images and playlists: these patch formats are for cartridge ROMs, so none are offered.
const DISC_EXTENSIONS: &[&str] =
    &["chd", "cue", "gdi", "iso", "cso", "pbp", "m3u", "rvz", "gcz", "wbfs", "cdi", "ccd", "ciso"];

/// The patches for each game: listed with the files, matched to games afterwards.
#[derive(Default)]
struct Listing {
    candidates: Vec<Candidate>,
    patches: Vec<PathBuf>,
}

fn patch_extension(path: &Path) -> Option<String> {
    let ext = path.extension()?.to_string_lossy().to_ascii_lowercase();
    PATCH_EXTENSIONS.contains(&ext.as_str()).then_some(ext)
}

/// Gives each candidate its patches: those beside it with its name, and those in a folder
/// named as it.
fn attach_patches(candidates: &mut [Candidate], patches: &[PathBuf]) {
    use std::collections::HashMap;
    let mut beside: HashMap<(PathBuf, String), Vec<&PathBuf>> = HashMap::new();
    let mut folders: HashMap<PathBuf, Vec<&PathBuf>> = HashMap::new();
    for p in patches {
        let (Some(dir), Some(stem)) = (p.parent(), p.file_stem()) else { continue };
        beside.entry((dir.to_owned(), stem.to_string_lossy().into_owned())).or_default().push(p);
        folders.entry(dir.to_owned()).or_default().push(p);
    }
    for c in candidates {
        let ext = c.path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase());
        if ext.is_some_and(|e| DISC_EXTENSIONS.contains(&e.as_str())) {
            continue;
        }
        let (Some(dir), Some(stem)) = (c.path.parent(), c.path.file_stem()) else { continue };
        let stem = stem.to_string_lossy().into_owned();
        let mut own: Vec<(PathBuf, bool)> = beside
            .get(&(dir.to_owned(), stem.clone()))
            .into_iter()
            .flatten()
            .map(|p| ((*p).clone(), true))
            .collect();
        // Preferred first: BPS, then UPS, then IPS.
        own.sort_by_key(|(p, _)| {
            patch_extension(p).and_then(|e| PATCH_EXTENSIONS.iter().position(|x| *x == e))
        });
        let mut chosen: Vec<(PathBuf, bool)> = folders
            .get(&dir.join(&stem))
            .into_iter()
            .flatten()
            .map(|p| ((*p).clone(), false))
            .collect();
        chosen.sort();
        own.extend(chosen);
        c.patches = own;
    }
}

/// The disc number in a tag such as `Disc 2` (or `Disc 2 of 3`).
fn disc_number(tag: &str) -> Option<u32> {
    let rest = tag.strip_prefix("Disc ")?;
    rest.split_whitespace().next()?.parse().ok()
}

/// What makes files one game: system, folder, title and the tags before the disc's.
type DiscGroup = (String, String, String, String);

/// Folds a multi-disc game's files into one entry: files in the same folder of the same system
/// whose names agree up to a `(Disc N)` tag. Tags after the disc's name that disc
/// (`Command & Conquer (USA) (Disc 1) (GDI)`, `(Disc 2) (NOD)`), so they are not part of the
/// game's. A file with no disc tag joins discs 2, 3… of the same name as their disc 1 (`Final
/// Fantasy VII (T).chd` beside `(T) (Disc 2)`), as some sets name the first disc.
///
/// The entry is disc 1's (its path, so its artwork and play history; with the disc tags dropped
/// from its tags), sized as all of them, listing every disc in order. A lone `(Disc 1)` stays as
/// it is.
pub fn group_discs(found: Vec<Found>) -> Vec<Found> {
    use std::collections::BTreeMap;
    let folder = |p: &str| p.rsplit_once('/').map_or("", |(dir, _)| dir).to_owned();
    let mut groups: BTreeMap<DiscGroup, Vec<(u32, Found)>> = BTreeMap::new();
    let mut untagged = Vec::new();
    for f in found {
        let tags: Vec<&str> = f.tags.split(", ").filter(|t| !t.is_empty()).collect();
        let Some(at) = tags.iter().position(|t| disc_number(t).is_some()) else {
            untagged.push(f);
            continue;
        };
        let disc = disc_number(tags[at]).unwrap_or(1);
        let rest = tags[..at].join(", ");
        groups
            .entry((f.system.to_owned(), folder(&f.path), f.title.clone(), rest))
            .or_default()
            .push((disc, f));
    }
    let mut out = Vec::new();
    for f in untagged {
        let key = (f.system.to_owned(), folder(&f.path), f.title.clone(), f.tags.clone());
        match groups.get_mut(&key) {
            Some(discs) if discs.iter().all(|(n, _)| *n >= 2) => discs.push((1, f)),
            _ => out.push(f),
        }
    }
    for ((_, _, _, rest), mut discs) in groups {
        discs.sort_by_key(|(n, f)| (*n, f.path.clone()));
        if discs.len() == 1 {
            out.push(discs.remove(0).1);
            continue;
        }
        let size = discs.iter().map(|(_, f)| f.size).sum();
        let modified = discs.iter().map(|(_, f)| f.modified).max().unwrap_or(0);
        let paths = discs.iter().map(|(_, f)| f.path.clone()).collect();
        let (_, first) = discs.remove(0);
        out.push(Found { tags: rest, size, modified, discs: paths, ..first });
    }
    out.sort_by(|a, b| (a.system, &a.title, &a.path).cmp(&(b.system, &b.title, &b.path)));
    out
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
    /// Its patches, and whether each is beside it (named as it).
    patches: Vec<(PathBuf, bool)>,
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
    let mut patches = Vec::new();
    let mut folders = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        if is_dir(&entry) {
            let system = systems::by_folder(&name);
            if system.is_none() && systems::is_unsupported_folder(&name) {
                continue;
            }
            folders.push((path, system));
        } else if let Some(system) = pick_system(&path, None) {
            candidates.push(Candidate { path, system, media: String::new(), patches: Vec::new() });
        }
    }
    std::thread::scope(|scope| {
        let handles: Vec<_> = folders
            .iter()
            .map(|(dir, system)| {
                scope.spawn(move || {
                    let mut found = Listing::default();
                    walk_dir(dir, *system, 1, &mut found);
                    found
                })
            })
            .collect();
        for handle in handles {
            let listing = handle.join().unwrap_or_default();
            candidates.extend(listing.candidates);
            patches.extend(listing.patches);
        }
    });
    attach_patches(&mut candidates, &patches);

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

fn walk_dir(dir: &Path, system: Option<&'static System>, depth: usize, found: &mut Listing) {
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
        } else if patch_extension(&path).is_some() {
            found.patches.push(path);
        } else if let Some(sys) = pick_system(&path, system) {
            let stem =
                path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let media = MEDIA_KINDS
                .iter()
                .filter(|kind| media_candidates(&stem, kind).iter().any(|c| media.contains(c)))
                .copied()
                .collect::<Vec<_>>()
                .join(",");
            found.candidates.push(Candidate { path, system: sys, media, patches: Vec::new() });
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
    let relative_to_root = |p: &Path| -> Option<String> {
        Some(
            p.strip_prefix(root)
                .ok()?
                .components()
                .map(|part| part.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/"),
        )
    };
    let mut first_beside = true;
    let patches = c
        .patches
        .iter()
        .filter_map(|(p, beside)| {
            let ext = patch_extension(p)?.to_uppercase();
            let name = if *beside {
                format!("{ext} patch")
            } else {
                p.file_stem()?.to_string_lossy().into_owned()
            };
            let is_default = *beside && std::mem::replace(&mut first_beside, false);
            Some(FoundPatch { path: relative_to_root(p)?, name, is_default })
        })
        .collect();
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
        discs: Vec::new(),
        patches,
    })
}

/// Rescans `root` and brings the `games` table in line: new files added, changed ones updated
/// (keeping their ids, so play history and saves stay attached), missing ones removed.
pub async fn scan(pool: &SqlitePool, root: &Path) -> Result<ScanReport, sqlx::Error> {
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
            ("nds/Game.nds", 8),
            // Another console's discs: a .chd would otherwise be taken for PlayStation.
            ("saturn/Spy Game (USA).chd", 8),
            ("SegaCD/Disc Game (USA).cue", 8),
            ("psx/Epic (USA) (Disc 2).chd", 20),
            ("psx/Epic (USA) (Disc 1).chd", 10),
            ("psx/Epic (USA) (Disc 3).chd", 30),
            ("psx/Epic (Europe) (Disc 1).chd", 10),
            // Discs named after their number belong together; the name is the disc's.
            ("psx/War (USA) (Disc 1) (Allies).chd", 10),
            ("psx/War (USA) (Disc 2) (Soviet).chd", 10),
            // Disc 1 without a disc tag (as some translations name it).
            ("psx/Saga (T).chd", 10),
            ("psx/Saga (T) (Disc 2).chd", 10),
            ("psx/Saga (T) (Disc 3).chd", 10),
            // An untagged release beside another release's discs is its own game.
            ("psx/Epic (Japan).chd", 10),
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
    fn finds_patches_beside_a_game_and_in_its_folder() {
        let root = std::env::temp_dir().join(format!("sovemu-patches-{}", rand_suffix()));
        for path in [
            "snes/Mother (Japan).sfc",
            "snes/Mother (Japan).ips",
            "snes/Mother (Japan).bps",
            "snes/Mother (Japan)/English v1.1.bps",
            "snes/Mother (Japan)/Hard Mode.ips",
            "snes/Other (USA).sfc",
            "snes/notes.txt",
            // Disc images get no patches.
            "psx/Disc (USA).chd",
            "psx/Disc (USA).ips",
        ] {
            let p = root.join(path);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, [0u8; 4]).unwrap();
        }
        let found = walk(&root);
        std::fs::remove_dir_all(&root).ok();
        let paths: Vec<_> = found.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(
            paths,
            ["psx/Disc (USA).chd", "snes/Mother (Japan).sfc", "snes/Other (USA).sfc"],
            "patches are not games"
        );
        let mother = &found[1];
        let summary: Vec<_> = mother
            .patches
            .iter()
            .map(|p| (p.path.as_str(), p.name.as_str(), p.is_default))
            .collect();
        assert_eq!(
            summary,
            [
                ("snes/Mother (Japan).bps", "BPS patch", true),
                ("snes/Mother (Japan).ips", "IPS patch", false),
                ("snes/Mother (Japan)/English v1.1.bps", "English v1.1", false),
                ("snes/Mother (Japan)/Hard Mode.ips", "Hard Mode", false),
            ],
            "beside it first (BPS preferred, applied by default), then its folder's"
        );
        assert!(found[0].patches.is_empty(), "no patches for a disc image");
        assert!(found[2].patches.is_empty());
    }

    #[test]
    fn folds_a_multi_disc_game_into_one() {
        let root = library();
        let psx: Vec<_> =
            group_discs(walk(&root)).into_iter().filter(|f| f.system == "psx").collect();
        std::fs::remove_dir_all(&root).ok();
        let paths: Vec<_> = psx.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "psx/Epic (Europe) (Disc 1).chd",
                "psx/Epic (Japan).chd",
                "psx/Epic (USA) (Disc 1).chd",
                "psx/Saga (T).chd",
                "psx/War (USA) (Disc 1) (Allies).chd",
            ],
            "the USA discs are one game; the lone European disc and the Japanese release stay"
        );
        let usa = psx.iter().find(|f| f.tags == "USA").expect("the USA game, its disc tag dropped");
        assert_eq!(usa.path, "psx/Epic (USA) (Disc 1).chd");
        assert_eq!(
            usa.discs,
            [
                "psx/Epic (USA) (Disc 1).chd",
                "psx/Epic (USA) (Disc 2).chd",
                "psx/Epic (USA) (Disc 3).chd"
            ]
        );
        assert_eq!(usa.size, 60);
        let europe = psx.iter().find(|f| f.tags.starts_with("Europe")).unwrap();
        assert!(europe.discs.is_empty());
        let war = psx.iter().find(|f| f.title == "War").unwrap();
        assert_eq!(war.tags, "USA", "the discs' names are not the game's tags");
        assert_eq!(war.discs.len(), 2);
        let saga = psx.iter().find(|f| f.title == "Saga").unwrap();
        assert_eq!(
            saga.discs,
            ["psx/Saga (T).chd", "psx/Saga (T) (Disc 2).chd", "psx/Saga (T) (Disc 3).chd"]
        );
        assert_eq!(saga.tags, "T");
        assert_eq!(disc_number("Disc 2 of 3"), Some(2));
        assert_eq!(disc_number("Disco"), None);
    }

    #[test]
    fn walks_folders_aliases_and_loose_files() {
        let root = library();
        // The multi-disc files are the next test's.
        let found: Vec<_> = walk(&root).into_iter().filter(|f| f.system != "psx").collect();
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
