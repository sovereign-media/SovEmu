//! Decorations (bezels): artwork the clients draw around a game's picture, filling the bars a
//! 4:3 game leaves on a 16:9 screen. The server only finds and serves them; the client draws
//! them, so the stream and its encoder are untouched.
//!
//! Sets use Batocera's layout, so its decoration packs work as they are:
//!
//! ```text
//! <dir>/<set>/default.png                     every game, unless something closer exists
//! <dir>/<set>/systems/<system>.png            a system's games
//! <dir>/<set>/games/<system>/<rom stem>.png   one game
//! <dir>/<set>/games/<rom stem>.png            a game of that name on any system
//! ```
//!
//! Each `.png` has a `.info` beside it (JSON): the image's `width` and `height`, and the
//! `top`/`left`/`bottom`/`right` margins of the window the game shows through. Without one,
//! the window is the largest centred 4:3 area.
//!
//! Sets come from several folders, merged by name: those in `SOVEMU_DECORATIONS_DIRS` first
//! (a decorations folder on the NAS, say), then `<data>/decorations`, then the built-in folder.
//! For the same file, the first folder wins. SovEmu ships one set of its own, `sovemu`
//! ([`write_builtin`]): frames drawn by this code, and the ESRB's rating icons
//! (`ratings/esrb/<rating>.png`, [`Decorations::esrb_badge`]), which the clients show on a game's
//! panel. An admin replaces any of them by putting a file at the same path in an earlier folder
//! (`<data>/decorations/sovemu/ratings/esrb/t.png`). No other third-party artwork is shipped, and
//! there is no downloader.

use std::collections::{BTreeMap, HashMap};
use std::io::Write as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The built-in set's name.
pub const BUILTIN: &str = "sovemu";
/// Turns decorations off (`SOVEMU_DECORATION=none`, or per system).
pub const NONE: &str = "none";

/// Bumped when the built-in artwork changes, so a cached copy is drawn again.
const BUILTIN_VERSION: &str = "3";

/// The ESRB's rating icons (the 2013 set from Wikimedia Commons, where they are public domain as
/// simple text logos; trademarks of the ESA), rendered to PNG by the web client's
/// `npm run esrb-badges`, by [`Esrb::file_stem`](crate::info::Esrb::file_stem).
const ESRB_BADGES: [(&str, &[u8]); 7] = [
    ("ec", include_bytes!("../assets/ratings/esrb/ec.png")),
    ("e", include_bytes!("../assets/ratings/esrb/e.png")),
    ("e10", include_bytes!("../assets/ratings/esrb/e10.png")),
    ("t", include_bytes!("../assets/ratings/esrb/t.png")),
    ("m", include_bytes!("../assets/ratings/esrb/m.png")),
    ("ao", include_bytes!("../assets/ratings/esrb/ao.png")),
    ("rp", include_bytes!("../assets/ratings/esrb/rp.png")),
];

/// Where a set keeps the ESRB's rating icons.
const ESRB_DIR: &str = "ratings/esrb";

/// Where a decoration's window is, in its image's pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct Window {
    pub width: u32,
    pub height: u32,
    #[serde(default)]
    pub top: u32,
    #[serde(default)]
    pub left: u32,
    #[serde(default)]
    pub bottom: u32,
    #[serde(default)]
    pub right: u32,
}

impl Window {
    /// The largest centred 4:3 window in an image this size (a `.info` was missing).
    fn centred(width: u32, height: u32) -> Self {
        let inner = (height * 4 / 3).min(width);
        let side = (width - inner) / 2;
        Self { width, height, top: 0, left: side, bottom: 0, right: width - inner - side }
    }
}

/// The decoration for a game: which file, and its window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decoration {
    pub set: String,
    /// Within the set, with `/` separators (`systems/snes.png`).
    pub path: String,
    pub window: Window,
}

impl Decoration {
    /// Its URL under `/media/decorations/`.
    pub fn url(&self) -> String {
        let segments: Vec<String> =
            std::iter::once(self.set.as_str()).chain(self.path.split('/')).map(escape).collect();
        format!("/media/decorations/{}", segments.join("/"))
    }
}

/// Escapes a URL path segment (ROM names have spaces, brackets, ampersands…).
fn escape(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len());
    for byte in segment.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// What a decoration set holds ([`Decorations::summary`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetSummary {
    /// It has `default.png`, for every game nothing closer covers.
    pub has_default: bool,
    /// The systems with an image of their own (`systems/<id>.png`), by id.
    pub systems: Vec<String>,
    /// Images for single games (`games/…`).
    pub games: usize,
}

/// Every set's files, from all the folders, merged.
#[derive(Debug, Default)]
pub struct Decorations {
    /// Set → path within it (`systems/snes.png`) → the file on disk.
    sets: BTreeMap<String, HashMap<String, PathBuf>>,
}

impl Decorations {
    /// Lists `dirs`, first wins for the same set and file.
    pub fn load(dirs: &[PathBuf]) -> Self {
        let mut sets: BTreeMap<String, HashMap<String, PathBuf>> = BTreeMap::new();
        for dir in dirs {
            let Ok(entries) = std::fs::read_dir(dir) else { continue };
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with('.') || !entry.path().is_dir() {
                    continue;
                }
                let files = sets.entry(name).or_default();
                list(&entry.path(), "", files, 0);
            }
        }
        Self { sets }
    }

    /// The sets found, by name.
    pub fn sets(&self) -> Vec<String> {
        self.sets.keys().cloned().collect()
    }

    /// What a set holds: a default image, which systems have their own, and how many games.
    /// None for no such set.
    pub fn summary(&self, set: &str) -> Option<SetSummary> {
        let files = self.sets.get(set)?;
        let mut systems: Vec<String> = files
            .keys()
            .filter_map(|p| p.strip_prefix("systems/")?.strip_suffix(".png"))
            .filter(|s| !s.contains('/'))
            .map(str::to_owned)
            .collect();
        systems.sort();
        Some(SetSummary {
            has_default: files.contains_key("default.png"),
            systems,
            games: files.keys().filter(|p| p.starts_with("games/") && p.ends_with(".png")).count(),
        })
    }

    /// The URL (under `/media/decorations/`) of an ESRB rating's icon: the built-in set's
    /// `ratings/esrb/<rating>.png`, from whichever folder has it first. None if no folder does.
    pub fn esrb_badge(&self, rating: crate::info::Esrb) -> Option<String> {
        let path = format!("{ESRB_DIR}/{}.png", rating.file_stem());
        self.file(BUILTIN, &path)?;
        Some(format!(
            "/media/decorations/{}/{}",
            escape(BUILTIN),
            path.split('/').map(escape).collect::<Vec<_>>().join("/")
        ))
    }

    /// A file of a set, by its path within it.
    pub fn file(&self, set: &str, path: &str) -> Option<&Path> {
        self.sets.get(set)?.get(path).map(PathBuf::as_path)
    }

    /// The decoration for a game in `set`: its own, else its name's on any system, else its
    /// system's, else the set's default. None for no such set (or `none`).
    pub fn for_game(&self, set: &str, system: &str, rom_stem: &str) -> Option<Decoration> {
        let files = self.sets.get(set)?;
        let candidates = [
            format!("games/{system}/{rom_stem}.png"),
            format!("games/{rom_stem}.png"),
            format!("systems/{system}.png"),
            "default.png".to_owned(),
        ];
        // The closest image whose window is known (from its .info, else its size).
        candidates.into_iter().find_map(|path| {
            let image = files.get(&path)?;
            let info = files.get(&format!("{}.info", path.trim_end_matches(".png")));
            let window = info
                .and_then(|f| std::fs::read(f).ok())
                .and_then(|bytes| serde_json::from_slice::<Window>(&bytes).ok())
                .or_else(|| png_size(image).map(|(w, h)| Window::centred(w, h)))?;
            Some(Decoration { set: set.to_owned(), path, window })
        })
    }
}

/// Collects a set's `.png` and `.info` files (games may sit two folders deep).
fn list(dir: &Path, prefix: &str, files: &mut HashMap<String, PathBuf>, depth: usize) {
    if depth > 2 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        let key = format!("{prefix}{name}");
        if path.is_dir() {
            list(&path, &format!("{key}/"), files, depth + 1);
        } else if name.ends_with(".png") || name.ends_with(".info") {
            // The first folder listed wins.
            files.entry(key).or_insert(path);
        }
    }
}

/// A PNG's size, from its header.
fn png_size(path: &Path) -> Option<(u32, u32)> {
    use std::io::Read as _;
    let mut header = [0u8; 24];
    std::fs::File::open(path).ok()?.read_exact(&mut header).ok()?;
    if &header[..8] != b"\x89PNG\r\n\x1a\n" || &header[12..16] != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes(header[16..20].try_into().ok()?);
    let height = u32::from_be_bytes(header[20..24].try_into().ok()?);
    Some((width, height))
}

// --- The built-in set ----------------------------------------------------------------------

/// The built-in set's image size (Batocera's, which the clients scale to the screen).
const WIDTH: u32 = 1920;
const HEIGHT: u32 = 1080;

/// A system's screen shape, as width:height: the window its frame leaves.
fn aspect(system: &str) -> (u32, u32) {
    match system {
        "gb" | "gbc" | "gamegear" => (10, 9),
        "gba" => (3, 2),
        // 160×152 screens.
        "ngp" | "ngpc" => (20, 19),
        // 224×144.
        "wswan" | "wswanc" => (14, 9),
        // 160×102.
        "lynx" => (80, 51),
        // 384×224.
        "virtualboy" => (12, 7),
        // A portrait tube.
        "vectrex" => (4, 5),
        _ => (4, 3),
    }
}

/// Writes the built-in set (`sovemu`) into `dir`: a frame per system, in its colour, around a
/// window of its screen's shape, and a default. Skipped when `dir` already holds this version.
pub fn write_builtin(dir: &Path) -> std::io::Result<()> {
    let set = dir.join(BUILTIN);
    let marker = set.join(".version");
    if std::fs::read_to_string(&marker).is_ok_and(|v| v.trim() == BUILTIN_VERSION) {
        return Ok(());
    }
    std::fs::create_dir_all(set.join("systems"))?;
    for system in crate::systems::SYSTEMS {
        write_frame(&set.join("systems").join(system.id), system.color, aspect(system.id))?;
    }
    write_frame(&set.join("default"), "#4169e1", (4, 3))?;
    let ratings = set.join(ESRB_DIR);
    std::fs::create_dir_all(&ratings)?;
    for (stem, png) in ESRB_BADGES {
        std::fs::write(ratings.join(format!("{stem}.png")), png)?;
    }
    std::fs::write(marker, BUILTIN_VERSION)
}

/// Draws one frame: `<stem>.png` and `<stem>.info`.
fn write_frame(stem: &Path, colour: &str, (aw, ah): (u32, u32)) -> std::io::Result<()> {
    let inner = (HEIGHT * aw / ah).min(WIDTH);
    let left = (WIDTH - inner) / 2;
    let window = Window {
        width: WIDTH,
        height: HEIGHT,
        top: 0,
        left,
        bottom: 0,
        right: WIDTH - inner - left,
    };
    let pixels = draw(window, parse_colour(colour));
    let file = std::fs::File::create(stem.with_extension("png"))?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), WIDTH, HEIGHT);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::High);
    let mut writer = encoder.write_header().map_err(std::io::Error::other)?;
    writer.write_image_data(&pixels).map_err(std::io::Error::other)?;
    writer.finish().map_err(std::io::Error::other)?;
    let mut info = std::fs::File::create(stem.with_extension("info"))?;
    info.write_all(&serde_json::to_vec_pretty(&window).map_err(std::io::Error::other)?)?;
    Ok(())
}

fn parse_colour(hex: &str) -> [f32; 3] {
    let hex = hex.trim_start_matches('#');
    let channel = |i: usize| {
        u8::from_str_radix(hex.get(i..i + 2).unwrap_or("80"), 16).unwrap_or(128) as f32 / 255.0
    };
    [channel(0), channel(2), channel(4)]
}

/// The frame's pixels (RGBA): a dark panel either side of the window, lit by the system's
/// colour from its outer edge, with a fine vertical grain and a lit bevel along the window,
/// whose edge casts a soft shadow onto the panel. The window itself is transparent.
fn draw(window: Window, colour: [f32; 3]) -> Vec<u8> {
    let (w, h) = (window.width as usize, window.height as usize);
    let (left, right) = (window.left as usize, w - window.right as usize);
    let mut pixels = vec![0u8; w * h * 4];
    let base = [0.043, 0.055, 0.078];
    for y in 0..h {
        let fy = y as f32 / h as f32;
        for x in (0..left).chain(right..w) {
            // Distance from the window's edge, and from the screen's.
            let (from_window, from_edge, panel) =
                if x < left { (left - x, x, left) } else { (x - right + 1, w - 1 - x, w - right) };
            let t = from_edge as f32 / panel.max(1) as f32;
            // The colour, strongest at the outer edge and towards the top.
            let glow = (1.0 - t).powf(2.2) * (0.55 - 0.35 * fy).max(0.0);
            // A soft shadow where the panel meets the picture.
            let shadow = (-(from_window as f32) / 26.0).exp() * 0.65;
            // Fine vertical grain, like brushed plastic.
            let grain = if x % 3 == 0 { 0.012 } else { 0.0 };
            // Darker towards the bottom.
            let shade = 1.0 - 0.35 * fy;
            let mut rgb = [0f32; 3];
            for (c, value) in rgb.iter_mut().enumerate() {
                let lit = base[c] * shade + colour[c] * glow * 0.6 + grain;
                *value = lit * (1.0 - shadow);
            }
            // A thin lit bevel right at the window's edge.
            if from_window <= 2 {
                for (c, value) in rgb.iter_mut().enumerate() {
                    *value = (*value + 0.10 + colour[c] * 0.12).min(1.0);
                }
            }
            let i = (y * w + x) * 4;
            pixels[i] = (rgb[0].clamp(0.0, 1.0) * 255.0) as u8;
            pixels[i + 1] = (rgb[1].clamp(0.0, 1.0) * 255.0) as u8;
            pixels[i + 2] = (rgb[2].clamp(0.0, 1.0) * 255.0) as u8;
            pixels[i + 3] = 255;
        }
    }
    pixels
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("sovemu-decorations-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn touch(path: PathBuf, bytes: &[u8]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }

    #[test]
    fn the_esrb_icons_are_built_in_and_an_earlier_folder_replaces_them() {
        use crate::info::Esrb;
        for rating in Esrb::ALL {
            let (_, png) =
                ESRB_BADGES.iter().find(|(stem, _)| *stem == rating.file_stem()).unwrap();
            assert_eq!(&png[1..4], b"PNG", "{rating:?}'s icon is a PNG");
        }
        let (builtin, own) = (dir("esrb-builtin"), dir("esrb-own"));
        // The icons as write_builtin writes them (without drawing every frame).
        for (stem, png) in ESRB_BADGES {
            touch(builtin.join(format!("sovemu/{ESRB_DIR}/{stem}.png")), png);
        }
        touch(own.join("sovemu/ratings/esrb/t.png"), b"\x89PNG mine");
        let sets = Decorations::load(&[own.clone(), builtin.clone()]);
        assert_eq!(
            sets.esrb_badge(Esrb::Everyone10Plus).as_deref(),
            Some("/media/decorations/sovemu/ratings/esrb/e10.png")
        );
        let teen = sets.file(BUILTIN, "ratings/esrb/t.png").unwrap();
        assert!(teen.starts_with(&own), "the admin's own icon wins: {teen:?}");
        assert_eq!(Decorations::load(std::slice::from_ref(&own)).esrb_badge(Esrb::Mature), None);
        std::fs::remove_dir_all(&builtin).ok();
        std::fs::remove_dir_all(&own).ok();
    }

    #[test]
    fn a_builtin_frame_is_shaped_to_its_systems_screen() {
        let out = dir("frame");
        std::fs::create_dir_all(out.join("sovemu/systems")).unwrap();
        // One frame (drawing them all is slow in a debug build).
        write_frame(&out.join("sovemu/systems/gba"), "#5a2dc8", aspect("gba")).unwrap();
        let set = Decorations::load(std::slice::from_ref(&out));
        assert_eq!(set.sets(), [BUILTIN]);
        let gba = set.for_game(BUILTIN, "gba", "Any").unwrap();
        assert_eq!(gba.path, "systems/gba.png");
        assert_eq!(
            gba.window,
            Window { width: 1920, height: 1080, top: 0, left: 150, bottom: 0, right: 150 },
            "a 3:2 window"
        );
        assert_eq!(png_size(&out.join("sovemu/systems/gba.png")), Some((1920, 1080)));
        assert_eq!(aspect("snes"), (4, 3));
        assert_eq!(aspect("gbc"), (10, 9));
        // The set is drawn once per version: with the marker there, nothing is redrawn.
        std::fs::write(out.join("sovemu/.version"), BUILTIN_VERSION).unwrap();
        write_builtin(&out).unwrap();
        assert!(!out.join("sovemu/systems/snes.png").exists());
        std::fs::remove_dir_all(&out).ok();
    }

    #[test]
    fn a_game_gets_its_own_then_its_systems_then_the_default() {
        let user = dir("user");
        let builtin = dir("base");
        let info = br#"{"width":1920,"height":1080,"top":0,"left":300,"bottom":0,"right":300}"#;
        touch(user.join("arcade/games/snes/Mother 2 (Japan).png"), b"");
        touch(user.join("arcade/games/snes/Mother 2 (Japan).info"), info);
        touch(user.join("arcade/systems/snes.png"), b"");
        touch(user.join("arcade/systems/snes.info"), info);
        // The same set in a later folder: its files only fill gaps.
        touch(builtin.join("arcade/systems/snes.png"), b"other");
        touch(builtin.join("arcade/default.png"), b"");
        touch(builtin.join("arcade/default.info"), info);
        let set = Decorations::load(&[user.clone(), builtin.clone()]);
        assert_eq!(
            set.summary("arcade"),
            Some(SetSummary { has_default: true, systems: vec!["snes".into()], games: 1 })
        );
        assert_eq!(set.summary("missing"), None);
        let own = set.for_game("arcade", "snes", "Mother 2 (Japan)").unwrap();
        assert_eq!(own.path, "games/snes/Mother 2 (Japan).png");
        assert_eq!(own.url(), "/media/decorations/arcade/games/snes/Mother%202%20%28Japan%29.png");
        assert_eq!(set.for_game("arcade", "snes", "Other").unwrap().path, "systems/snes.png");
        assert_eq!(
            set.file("arcade", "systems/snes.png"),
            Some(user.join("arcade/systems/snes.png").as_path()),
            "the first folder wins"
        );
        assert_eq!(set.for_game("arcade", "nes", "Other").unwrap().path, "default.png");
        assert!(set.for_game("missing", "snes", "Other").is_none());
        std::fs::remove_dir_all(&user).ok();
        std::fs::remove_dir_all(&builtin).ok();
    }

    #[test]
    fn without_an_info_the_window_is_the_centred_four_by_three() {
        assert_eq!(
            Window::centred(1920, 1080),
            Window { width: 1920, height: 1080, top: 0, left: 240, bottom: 0, right: 240 }
        );
        assert_eq!(escape("A&B (C).png"), "A%26B%20%28C%29.png");
    }
}
