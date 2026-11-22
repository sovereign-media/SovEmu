//! Walking a ROM library: which files are games of which system, their parts (discs, disks,
//! tapes) folded into one game, the soft patches beside them and their artwork's file names.
//! Reads the disk only; the server stores what it finds.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

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
    /// What a multi-part game's [`Found::discs`] are (discs, disks, tapes…); none for one file.
    pub parts: Option<PartKind>,
    /// A multi-part game's part numbers that are not there: a gap in its numbering (discs 1
    /// and 3), or the parts before a lone later one (only disc 2).
    pub missing_parts: Vec<u32>,
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
pub(crate) const PATCH_EXTENSIONS: &[&str] = &["bps", "ups", "ips"];

/// Disc images and playlists: these patch formats are for cartridge ROMs, so none are offered.
const DISC_EXTENSIONS: &[&str] =
    &["chd", "cue", "gdi", "iso", "cso", "pbp", "m3u", "rvz", "gcz", "wbfs", "cdi", "ccd", "ciso"];

/// The patches for each game: listed with the files, matched to games afterwards; and the files
/// no system plays.
#[derive(Default)]
struct Listing {
    candidates: Vec<Candidate>,
    patches: Vec<PathBuf>,
    skipped: Vec<Skipped>,
}

/// What a walk found: the games, and the files in the library it could not place.
#[derive(Debug, Default)]
pub struct Walk {
    pub found: Vec<Found>,
    pub skipped: Vec<Skipped>,
}

/// A file in a system's folder (or a folder no system uses) that is not a game for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    /// Relative to the ROM directory, with `/` separators.
    pub path: String,
    /// The folder's system, if the folder is one's.
    pub system: Option<&'static str>,
    /// Why, in words.
    pub reason: String,
}

/// Files that sit beside games and are not meant as one: notes, pictures, lists, saves. A
/// library holds many, and they are not worth an admin's attention.
const NOT_GAMES: &[&str] = &[
    "txt", "nfo", "md", "pdf", "htm", "html", "xml", "json", "jpg", "jpeg", "png", "gif", "bmp",
    "webp", "mp4", "mkv", "srm", "sav", "state", "ini", "cfg", "db", "log", "sfv", "md5", "sha1",
    "url", "lnk", "ds_store",
];

fn worth_listing(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| !NOT_GAMES.contains(&e.to_string_lossy().to_ascii_lowercase().as_str()))
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

/// What a multi-part game's files are: the media it came on, one file each.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PartKind {
    /// Optical discs: `(Disc 2)`, `(CD2)`, `[Disc 2]`, `- Disc 2`.
    Disc,
    /// Floppy disks: `(Disk 1 of 2)`, `(Disk B)`.
    Disk,
    /// Cassettes: `(Tape 2 of 2 Side B)`.
    Tape,
    /// The sides of one disk or tape, alone: `(Side A)`.
    Side,
    /// Anything else split into numbered files: `(Part 1 of 3)`, `(File 2 of 2)`.
    Part,
}

impl PartKind {
    /// Stored in `games.parts`, and the API's name for it.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Disc => "disc",
            Self::Disk => "disk",
            Self::Tape => "tape",
            Self::Side => "side",
            Self::Part => "part",
        }
    }
}

/// A file's place in a multi-part set, from one of its tags, in the forms No-Intro, Redump and
/// TOSEC use and the frontends accept: `Disc 2`, `CD2`, `CD 2`, `Disk 1 of 2`, `Disk 06 of 13`,
/// `Disk B` (a letter counts from A), `Tape 2 of 2 Side B`, `Side A`, `Part 1 of 3`,
/// `File 2 of 2`. Part and File only with "of", which a title never uses. The order key sorts
/// by number, then side.
fn media_part(tag: &str) -> Option<(PartKind, u32)> {
    let lower = tag.trim().to_ascii_lowercase();
    // "cd2" is "cd 2".
    let spaced = match lower.strip_prefix("cd") {
        Some(rest) if rest.starts_with(|c: char| c.is_ascii_digit()) => format!("cd {rest}"),
        _ => lower,
    };
    let words: Vec<&str> = spaced.split_whitespace().collect();
    let (kind, rest) = match words.split_first()? {
        (&("disc" | "cd"), rest) => (PartKind::Disc, rest),
        (&"disk", rest) => (PartKind::Disk, rest),
        (&("tape" | "cassette"), rest) => (PartKind::Tape, rest),
        (&"side", rest) => (PartKind::Side, rest),
        (&("part" | "file"), rest) if rest.get(1) == Some(&"of") => (PartKind::Part, rest),
        _ => return None,
    };
    let (number, mut rest) = rest.split_first()?;
    let number = ordinal(number)?;
    if let [of, _total, tail @ ..] = rest
        && *of == "of"
    {
        rest = tail;
    }
    let side = match rest {
        [] => 0,
        [side, letter] if *side == "side" && kind != PartKind::Side => ordinal(letter)?,
        _ => return None,
    };
    Some((kind, number * 4 + side))
}

/// "2", "02" or a letter ("b" is 2).
fn ordinal(word: &str) -> Option<u32> {
    if let Ok(n) = word.parse::<u32>() {
        return (n < 1000).then_some(n);
    }
    let mut chars = word.chars();
    match (chars.next(), chars.next()) {
        (Some(c @ 'a'..='z'), None) => Some(c as u32 - 'a' as u32 + 1),
        _ => None,
    }
}

/// A file's place in its set, its tags and title without the part's: from a tag (and a side tag
/// right after it), else from the title's end (`Game - Disc 2`).
fn split_part(title: &str, tags: &[&str]) -> Option<(PartKind, u32, String, String)> {
    if let Some(at) = tags.iter().position(|t| media_part(t).is_some()) {
        let (kind, mut order) = media_part(tags[at])?;
        // TOSEC writes "(Disk 1 of 2)(Side A)": the side is part of the same place.
        if kind != PartKind::Side
            && order % 4 == 0
            && let Some((PartKind::Side, side)) = tags.get(at + 1).and_then(|t| media_part(t))
        {
            order += side / 4;
        }
        return Some((kind, order, title.to_owned(), tags[..at].join(", ")));
    }
    let (name, part) = title.rsplit_once(" - ")?;
    let (kind, order) = media_part(part)?;
    Some((kind, order, name.trim().to_owned(), tags.join(", ")))
}

/// The part numbers from 1 to the highest of `present` that are not in it.
fn missing(present: impl Iterator<Item = u32>) -> Vec<u32> {
    let present: std::collections::BTreeSet<u32> = present.collect();
    let last = present.last().copied().unwrap_or(0);
    (1..last).filter(|n| !present.contains(n)).collect()
}

/// What makes files one game: system, folder, title, the tags before the part's, and its kind.
type DiscGroup = (String, String, String, String, PartKind);

/// Folds a multi-part game's files into one entry: files in the same folder of the same system
/// whose names agree up to a part tag ([`media_part`]: discs, disks, tapes, sides, parts). Tags
/// after the part's name that part (`Command & Conquer (USA) (Disc 1) (GDI)`, `(Disc 2)
/// (NOD)`), so they are not part of the game's. A file with no part tag joins parts 2, 3… of
/// the same name as their first (`Final Fantasy VII (T).chd` beside `(T) (Disc 2)`), as some sets
/// name the first disc.
///
/// The entry is the first part's (its path, so its artwork and play history; with the part tags
/// dropped from its tags and title), sized as all of them, listing every part in order, with its
/// kind. A lone first part stays as it is.
pub fn group_discs(found: Vec<Found>) -> Vec<Found> {
    use std::collections::BTreeMap;
    let folder = |p: &str| p.rsplit_once('/').map_or("", |(dir, _)| dir).to_owned();
    let mut groups: BTreeMap<DiscGroup, Vec<(u32, Found)>> = BTreeMap::new();
    let mut untagged = Vec::new();
    for f in found {
        let tags: Vec<&str> = f.tags.split(", ").filter(|t| !t.is_empty()).collect();
        let Some((kind, order, title, rest)) = split_part(&f.title, &tags) else {
            untagged.push(f);
            continue;
        };
        groups
            .entry((f.system.to_owned(), folder(&f.path), title, rest, kind))
            .or_default()
            .push((order, f));
    }
    let mut out = Vec::new();
    for f in untagged {
        // The first part, untagged, of a set whose tagged parts start at the second.
        let joined = groups.iter_mut().find(|((system, dir, title, tags, _), parts)| {
            system == f.system
                && *dir == folder(&f.path)
                && *title == f.title
                && *tags == f.tags
                && parts.iter().all(|(order, _)| order / 4 >= 2)
        });
        match joined {
            Some((_, parts)) => parts.push((4, f)),
            None => out.push(f),
        }
    }
    for ((_, _, title, rest, kind), mut parts) in groups {
        parts.sort_by_key(|(n, f)| (*n, f.path.clone()));
        let missing_parts = missing(parts.iter().map(|(order, _)| order / 4));
        if parts.len() == 1 {
            out.push(Found { missing_parts, ..parts.remove(0).1 });
            continue;
        }
        let size = parts.iter().map(|(_, f)| f.size).sum();
        let modified = parts.iter().map(|(_, f)| f.modified).max().unwrap_or(0);
        let paths = parts.iter().map(|(_, f)| f.path.clone()).collect();
        let (_, first) = parts.remove(0);
        out.push(Found {
            title,
            tags: rest,
            size,
            modified,
            discs: paths,
            parts: Some(kind),
            missing_parts,
            ..first
        });
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

/// Walks `root` and returns every file that belongs to a known system, and the files it could
/// not place (see [`Skipped`]). An error when `root`
/// itself cannot be read (a share that went away): never an empty library, which a caller would
/// take as every game removed.
///
/// Two stages, both parallel, because on an SMB/NFS library the cost is round trips: list every
/// folder (one thread per top-level folder, no per-file requests), then read the size and date
/// of only the files that are ROMs.
pub fn walk(root: &Path) -> std::io::Result<Walk> {
    let entries = std::fs::read_dir(root).map_err(|e| unreadable(root, e))?;
    let mut candidates = Vec::new();
    let mut patches = Vec::new();
    let mut skipped = Vec::new();
    let mut folders = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| unreadable(root, e))?;
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
    std::thread::scope(|scope| -> std::io::Result<()> {
        let handles: Vec<_> = folders
            .iter()
            .map(|(dir, system)| {
                scope.spawn(move || {
                    let mut found = Listing::default();
                    walk_dir(dir, *system, 1, &mut found).map(|()| found)
                })
            })
            .collect();
        for handle in handles {
            let listing = handle.join().map_err(panicked)??;
            candidates.extend(listing.candidates);
            patches.extend(listing.patches);
            skipped.extend(listing.skipped);
        }
        Ok(())
    })?;
    attach_patches(&mut candidates, &patches);

    let chunk = candidates.len().div_ceil(STAT_THREADS).max(1);
    let mut found: Vec<Found> = std::thread::scope(|scope| -> std::io::Result<Vec<Found>> {
        let handles: Vec<_> = candidates
            .chunks(chunk)
            .map(|part| {
                scope.spawn(move || {
                    part.iter().filter_map(|c| to_found(root, c)).collect::<Vec<_>>()
                })
            })
            .collect();
        let mut found = Vec::new();
        for handle in handles {
            found.extend(handle.join().map_err(panicked)?);
        }
        Ok(found)
    })?;
    found.sort_by(|a, b| (a.system, &a.title, &a.path).cmp(&(b.system, &b.title, &b.path)));
    // Listed with full paths; given relative to the library, as games are.
    for s in &mut skipped {
        if let Ok(relative) = Path::new(&s.path).strip_prefix(root) {
            s.path = relative.to_string_lossy().replace('\\', "/");
        }
    }
    skipped.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(Walk { found, skipped })
}

/// Lists `dir` into `found`. A folder that cannot be read, or whose listing breaks off, is an
/// error, naming it: a scan that went on without it would remove its games, and with them
/// everyone's history, favourites and patch choices for them.
fn walk_dir(
    dir: &Path,
    system: Option<&'static System>,
    depth: usize,
    found: &mut Listing,
) -> std::io::Result<()> {
    if depth > MAX_DEPTH {
        return Ok(());
    }
    let entries = std::fs::read_dir(dir).map_err(|e| unreadable(dir, e))?;
    // List the artwork folders once per directory rather than probing per game.
    let mut media = HashSet::new();
    for sub in ["images", "videos"] {
        if let Ok(files) = std::fs::read_dir(dir.join(sub)) {
            media.extend(
                files.flatten().map(|f| format!("{sub}/{}", f.file_name().to_string_lossy())),
            );
        }
    }
    for entry in entries {
        let entry = entry.map_err(|e| unreadable(dir, e))?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        if is_dir(&entry) {
            if !MEDIA_DIRS.contains(&name.to_ascii_lowercase().as_str()) {
                walk_dir(&path, system, depth + 1, found)?;
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
        } else if worth_listing(&path) {
            let reason = match system {
                Some(sys) => format!("{} games are not .{} files", sys.name, extension(&path)),
                None => "in a folder no system uses, and no system plays its kind of file".into(),
            };
            let path = path.to_string_lossy().into_owned();
            found.skipped.push(Skipped { path, system: system.map(|s| s.id), reason });
        }
    }
    Ok(())
}

fn extension(path: &Path) -> String {
    path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default()
}

/// An error naming the folder it happened in.
fn unreadable(dir: &Path, e: std::io::Error) -> std::io::Error {
    std::io::Error::new(e.kind(), format!("reading {}: {e}", dir.display()))
}

/// A walk thread that panicked, as an error: its part of the library is unknown, not empty.
fn panicked(_: Box<dyn std::any::Any + Send>) -> std::io::Error {
    std::io::Error::other("a library walk thread panicked")
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
        parts: None,
        missing_parts: Vec::new(),
    })
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
    fn lists_the_files_no_system_plays_but_not_notes_and_pictures() {
        let root = std::env::temp_dir().join(format!("sovemu-skipped-{}", rand_suffix()));
        for path in [
            "snes/Game (USA).sfc",
            "snes/Game (USA).xyz",
            "snes/notes.txt",
            "snes/box.png",
            "snes/Game (USA).srm",
            "oddities/Thing.abc",
        ] {
            let p = root.join(path);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, [0u8; 4]).unwrap();
        }
        let walked = walk(&root).unwrap();
        std::fs::remove_dir_all(&root).ok();
        assert_eq!(walked.found.len(), 1);
        let skipped: Vec<(&str, Option<&str>)> =
            walked.skipped.iter().map(|s| (s.path.as_str(), s.system)).collect();
        assert_eq!(skipped, [("oddities/Thing.abc", None), ("snes/Game (USA).xyz", Some("snes"))]);
        assert!(
            walked.skipped[1].reason.ends_with("games are not .xyz files"),
            "{:?}",
            walked.skipped
        );
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
        let found = walk(&root).unwrap().found;
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
    fn reads_every_common_way_of_numbering_parts() {
        use PartKind::*;
        let order = |n: u32, side: u32| n * 4 + side;
        for (tag, want) in [
            // Redump and No-Intro.
            ("Disc 1", Some((Disc, order(1, 0)))),
            ("Disc 2", Some((Disc, order(2, 0)))),
            ("Disk 2", Some((Disk, order(2, 0)))),
            ("Disk A", Some((Disk, order(1, 0)))),
            ("Disk B", Some((Disk, order(2, 0)))),
            // TOSEC.
            ("Disk 1 of 2", Some((Disk, order(1, 0)))),
            ("Disk 06 of 13", Some((Disk, order(6, 0)))),
            ("Disc 1 of 6", Some((Disc, order(1, 0)))),
            ("Tape 2 of 2 Side B", Some((Tape, order(2, 2)))),
            ("Disk 1 Side A", Some((Disk, order(1, 1)))),
            ("Side A", Some((Side, order(1, 0)))),
            ("Side B", Some((Side, order(2, 0)))),
            ("Part 1 of 3", Some((Part, order(1, 0)))),
            ("File 2 of 2", Some((Part, order(2, 0)))),
            // Elsewhere.
            ("CD1", Some((Disc, order(1, 0)))),
            ("CD 2", Some((Disc, order(2, 0)))),
            ("disc 3", Some((Disc, order(3, 0)))),
            ("Cassette 1", Some((Tape, order(1, 0)))),
            // Not parts.
            ("Disco", None),
            ("Part 2", None),
            ("Disc Jockey", None),
            ("USA", None),
            ("Rev 1", None),
            ("Disk 1 Bonus", None),
        ] {
            assert_eq!(media_part(tag), want, "{tag}");
        }
    }

    #[test]
    fn a_set_with_a_part_missing_says_which() {
        let found = |path: &str| {
            let stem = Path::new(path).file_stem().unwrap().to_string_lossy().into_owned();
            let (title, tags) = systems::parse_title(&stem);
            Found {
                system: "psx",
                path: path.into(),
                title,
                tags,
                size: 1,
                modified: 0,
                media: String::new(),
                discs: Vec::new(),
                patches: Vec::new(),
                parts: None,
                missing_parts: Vec::new(),
            }
        };
        let grouped = group_discs(vec![
            found("psx/Epic (USA) (Disc 1).chd"),
            found("psx/Epic (USA) (Disc 3).chd"),
            found("psx/Whole (USA) (Disc 1).chd"),
            found("psx/Whole (USA) (Disc 2).chd"),
            found("psx/Late (USA) (Disc 2).chd"),
        ]);
        let missing: Vec<(&str, &[u32])> =
            grouped.iter().map(|f| (f.title.as_str(), f.missing_parts.as_slice())).collect();
        assert_eq!(missing, [("Epic", &[2][..]), ("Late", &[1][..]), ("Whole", &[][..])]);
    }

    #[test]
    fn groups_disks_tapes_and_titled_discs_too() {
        let found = |path: &str| {
            let stem = Path::new(path).file_stem().unwrap().to_string_lossy().into_owned();
            let (title, tags) = systems::parse_title(&stem);
            Found {
                system: "nes",
                path: path.into(),
                title,
                tags,
                size: 1,
                modified: 0,
                media: String::new(),
                discs: Vec::new(),
                patches: Vec::new(),
                parts: None,
                missing_parts: Vec::new(),
            }
        };
        let games = group_discs(vec![
            // TOSEC, sides as their own tag.
            found("nes/Saga (1990)(Maker)(Disk 1 of 2)(Side B).fds"),
            found("nes/Saga (1990)(Maker)(Disk 1 of 2)(Side A).fds"),
            found("nes/Saga (1990)(Maker)(Disk 2 of 2)(Side A).fds"),
            // A disc number at the end of the title, and brackets.
            found("nes/Quest - Disc 2.chd"),
            found("nes/Quest - Disc 1.chd"),
            found("nes/Epic [Disc 2].chd"),
            found("nes/Epic [Disc 1].chd"),
        ]);
        let summary: Vec<_> = games
            .iter()
            .map(|g| (g.title.as_str(), g.parts.map(PartKind::as_str), g.discs.len()))
            .collect();
        assert_eq!(
            summary,
            [("Epic", Some("disc"), 2), ("Quest", Some("disc"), 2), ("Saga", Some("disk"), 3)]
        );
        let saga = games.iter().find(|g| g.title == "Saga").unwrap();
        assert_eq!(
            saga.discs,
            [
                "nes/Saga (1990)(Maker)(Disk 1 of 2)(Side A).fds",
                "nes/Saga (1990)(Maker)(Disk 1 of 2)(Side B).fds",
                "nes/Saga (1990)(Maker)(Disk 2 of 2)(Side A).fds",
            ],
            "by disk, then side"
        );
        assert_eq!(saga.tags, "1990, Maker");
    }

    #[test]
    fn folds_a_multi_disc_game_into_one() {
        let root = library();
        let psx: Vec<_> = group_discs(walk(&root).unwrap().found)
            .into_iter()
            .filter(|f| f.system == "psx")
            .collect();
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
        assert_eq!(media_part("Disc 2 of 3"), Some((PartKind::Disc, 8)));
        assert_eq!(media_part("Disco"), None);
    }

    #[test]
    fn walks_folders_aliases_and_loose_files() {
        let root = library();
        // The multi-disc files are the next test's.
        let found: Vec<_> =
            walk(&root).unwrap().found.into_iter().filter(|f| f.system != "psx").collect();
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

    #[test]
    fn a_library_that_cannot_be_read_is_an_error_not_an_empty_library() {
        let missing =
            std::env::temp_dir().join(format!("sovemu-no-library-{}", std::process::id()));
        let err = walk(&missing).unwrap_err();
        assert!(err.to_string().contains("sovemu-no-library"), "names the folder: {err}");
    }

    #[cfg(unix)]
    #[test]
    fn an_unreadable_system_folder_fails_the_walk() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("sovemu-locked-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("snes")).unwrap();
        std::fs::write(root.join("snes/Game.sfc"), [0u8; 16]).unwrap();
        std::fs::set_permissions(root.join("snes"), std::fs::Permissions::from_mode(0o000))
            .unwrap();
        // Root reads anything: the folder cannot be made unreadable for it (the container).
        if std::fs::read_dir(root.join("snes")).is_err() {
            let err = walk(&root).unwrap_err();
            assert!(err.to_string().contains("snes"), "{err}");
        }
        std::fs::set_permissions(root.join("snes"), std::fs::Permissions::from_mode(0o755))
            .unwrap();
        std::fs::remove_dir_all(root).ok();
    }
}
