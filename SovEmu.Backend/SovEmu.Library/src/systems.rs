//! The consoles SovEmu knows: how to recognise their games and which core plays them.
//!
//! A ROM library is laid out one folder per system. Folder names follow Recalbox/Batocera
//! (`nes/`, `snes/`, `megadrive/` …) with common aliases (`genesis/`, `Nintendo - NES/` …);
//! files outside a recognised folder are placed by extension when the extension is unambiguous.

use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize)]
pub struct System {
    /// Stable id, also the Recalbox folder name.
    pub id: &'static str,
    pub name: &'static str,
    /// A short label for tight spaces (the carousel's badge).
    pub short: &'static str,
    pub manufacturer: &'static str,
    pub year: u16,
    /// The UI accent colour.
    pub color: &'static str,
    #[serde(skip)]
    pub folders: &'static [&'static str],
    #[serde(skip)]
    pub extensions: &'static [&'static str],
    /// Cores that can play it, best first, as `<name>_libretro.<so|dylib>` stems.
    #[serde(skip)]
    pub cores: &'static [&'static str],
    /// Core options SovEmu sets for it (`key`, `value`).
    #[serde(skip)]
    pub options: &'static [(&'static str, &'static str)],
    /// A 3D system: its cores render with OpenGL, on a GPU or in software (`SOVEMU_GPU`).
    pub needs_gpu: bool,
}

pub const SYSTEMS: &[System] = &[
    System {
        id: "nes",
        name: "Nintendo Entertainment System",
        short: "NES",
        manufacturer: "Nintendo",
        year: 1983,
        color: "#c0392b",
        folders: &["nes", "famicom", "nintendo - nintendo entertainment system", "nintendo - nes"],
        extensions: &["nes", "unf", "unif", "fds"],
        cores: &["fceumm"],
        options: &[],
        needs_gpu: false,
    },
    System {
        id: "snes",
        name: "Super Nintendo",
        short: "SNES",
        manufacturer: "Nintendo",
        year: 1990,
        color: "#6c5ce7",
        folders: &["snes", "sfc", "superfamicom", "nintendo - super nintendo entertainment system"],
        extensions: &["sfc", "smc", "swc", "fig", "bs", "st"],
        cores: &["snes9x"],
        options: &[],
        needs_gpu: false,
    },
    System {
        id: "gb",
        name: "Game Boy",
        short: "GB",
        manufacturer: "Nintendo",
        year: 1989,
        color: "#7f8c3a",
        folders: &["gb", "gameboy", "nintendo - game boy"],
        extensions: &["gb", "dmg"],
        cores: &["gambatte", "mgba"],
        options: &[],
        needs_gpu: false,
    },
    System {
        id: "gbc",
        name: "Game Boy Color",
        short: "GBC",
        manufacturer: "Nintendo",
        year: 1998,
        color: "#16a085",
        folders: &["gbc", "gameboycolor", "nintendo - game boy color"],
        extensions: &["gbc"],
        cores: &["gambatte", "mgba"],
        options: &[],
        needs_gpu: false,
    },
    System {
        id: "gba",
        name: "Game Boy Advance",
        short: "GBA",
        manufacturer: "Nintendo",
        year: 2001,
        color: "#4834d4",
        folders: &["gba", "gameboyadvance", "nintendo - game boy advance"],
        extensions: &["gba"],
        cores: &["mgba"],
        options: &[],
        needs_gpu: false,
    },
    System {
        id: "megadrive",
        name: "Mega Drive / Genesis",
        short: "MD",
        manufacturer: "Sega",
        year: 1988,
        color: "#2d3436",
        folders: &["megadrive", "genesis", "md", "sega - mega drive - genesis"],
        extensions: &["md", "gen", "smd"],
        cores: &["genesis_plus_gx"],
        options: &[],
        needs_gpu: false,
    },
    System {
        id: "mastersystem",
        name: "Master System",
        short: "SMS",
        manufacturer: "Sega",
        year: 1985,
        color: "#d63031",
        folders: &["mastersystem", "sms", "sega - master system - mark iii"],
        extensions: &["sms"],
        cores: &["genesis_plus_gx"],
        options: &[],
        needs_gpu: false,
    },
    System {
        id: "psx",
        name: "PlayStation",
        short: "PS1",
        manufacturer: "Sony",
        year: 1994,
        color: "#636e72",
        folders: &["psx", "playstation", "ps1", "sony - playstation"],
        // Not .bin/.iso: other systems use them too, and a .cue/.m3u names its tracks anyway.
        extensions: &["chd", "cue", "pbp", "m3u", "ccd"],
        cores: &["pcsx_rearmed"],
        // Read 256 sectors (~600 KB) ahead on the core's CD thread, instead of 12: a library on
        // a network share answers each read in tens to hundreds of milliseconds.
        options: &[("pcsx_rearmed_cd_readahead", "256")],
        needs_gpu: false,
    },
    System {
        id: "fbneo",
        name: "Arcade",
        short: "ARC",
        manufacturer: "FinalBurn Neo",
        year: 1978,
        color: "#e17055",
        folders: &["fbneo", "arcade", "fba", "mame"],
        extensions: &["zip", "7z"],
        cores: &["fbneo"],
        options: &[],
        needs_gpu: false,
    },
    System {
        id: "neogeo",
        name: "Neo Geo",
        short: "NEO",
        manufacturer: "SNK",
        year: 1990,
        color: "#b8860b",
        folders: &["neogeo", "snk - neo geo"],
        extensions: &["zip", "7z"],
        cores: &["fbneo"],
        options: &[],
        needs_gpu: false,
    },
    System {
        id: "gamegear",
        name: "Game Gear",
        short: "GG",
        manufacturer: "Sega",
        year: 1990,
        color: "#0984e3",
        folders: &["gamegear", "gg", "sega - game gear"],
        extensions: &["gg"],
        cores: &["genesis_plus_gx"],
        options: &[],
        needs_gpu: false,
    },
    System {
        id: "n64",
        name: "Nintendo 64",
        short: "N64",
        manufacturer: "Nintendo",
        year: 1996,
        color: "#27ae60",
        folders: &["n64", "nintendo64", "nintendo 64", "nintendo - nintendo 64"],
        extensions: &["z64", "n64", "v64"],
        cores: &["mupen64plus_next"],
        options: &[],
        needs_gpu: true,
    },
    System {
        id: "dreamcast",
        name: "Dreamcast",
        short: "DC",
        manufacturer: "Sega",
        year: 1998,
        color: "#e67e22",
        folders: &["dreamcast", "dc", "sega - dreamcast"],
        extensions: &["chd", "cdi", "gdi", "cue", "m3u"],
        cores: &["flycast"],
        // Render on the thread that runs the core: SovEmu's GL context is current there only.
        // Memory cards (VMUs) per game in the player's save folder: by default Flycast writes
        // them into the BIOS folder, which is shared, and on a NAS read-only.
        options: &[
            ("reicast_threaded_rendering", "disabled"),
            ("reicast_per_content_vmus", "All VMUs"),
        ],
        needs_gpu: true,
    },
    System {
        id: "gamecube",
        name: "GameCube",
        short: "GC",
        manufacturer: "Nintendo",
        year: 2001,
        color: "#6c5ce7",
        folders: &["gamecube", "gc", "ngc", "nintendo - gamecube"],
        extensions: &["iso", "gcm", "gcz", "rvz", "ciso"],
        cores: &["dolphin"],
        options: &[],
        needs_gpu: true,
    },
    System {
        id: "ps2",
        name: "PlayStation 2",
        short: "PS2",
        manufacturer: "Sony",
        year: 2000,
        color: "#2d3436",
        folders: &["ps2", "playstation2", "playstation 2", "sony - playstation 2"],
        extensions: &["chd", "iso", "cso", "gz"],
        cores: &["pcsx2"],
        // Memory cards per game in the player's save folder, not shared under the BIOS folder.
        options: &[("pcsx2_shared_memory_cards", "disabled")],
        needs_gpu: true,
    },
];

pub fn by_id(id: &str) -> Option<&'static System> {
    SYSTEMS.iter().find(|s| s.id == id)
}

/// Batocera/Recalbox folders of systems SovEmu does not play. The scanner skips them rather
/// than guess from extensions: a Saturn, Sega CD or Neo Geo CD disc is a `.chd` or `.cue`
/// too, and the PlayStation core cannot run it.
pub(crate) const UNSUPPORTED_FOLDERS: &[&str] = &[
    "3do",
    "3ds",
    "64dd",
    "amigacd32",
    "atomiswave",
    "cdi",
    "jaguar",
    "jaguarcd",
    "model2",
    "model3",
    "naomi",
    "naomi2",
    "nds",
    "neogeocd",
    "pcenginecd",
    "pcfx",
    "ps3",
    "ps4",
    "psp",
    "psvita",
    "saturn",
    "segacd",
    "sega32x",
    "switch",
    "triforce",
    "wii",
    "wiiu",
    "xbox",
    "xbox360",
];

/// Whether a top-level library folder belongs to a system SovEmu does not play.
pub(crate) fn is_unsupported_folder(folder: &str) -> bool {
    UNSUPPORTED_FOLDERS.contains(&folder.trim().to_ascii_lowercase().as_str())
}

/// The system a top-level library folder belongs to.
pub(crate) fn by_folder(folder: &str) -> Option<&'static System> {
    let folder = folder.trim().to_ascii_lowercase();
    SYSTEMS.iter().find(|s| s.folders.contains(&folder.as_str()))
}

/// The system an extension belongs to, if exactly one system claims it.
pub(crate) fn by_extension(ext: &str) -> Option<&'static System> {
    let ext = ext.to_ascii_lowercase();
    let mut matches = SYSTEMS.iter().filter(|s| s.extensions.contains(&ext.as_str()));
    let first = matches.next()?;
    matches.next().is_none().then_some(first)
}

/// A display title and its tags from a ROM file name, No-Intro style:
/// `"Legend of Zelda, The (USA) (Rev 1) [!].nes"` → `("The Legend of Zelda", "USA, Rev 1, !")`.
pub(crate) fn parse_title(stem: &str) -> (String, String) {
    let mut title = String::new();
    let mut tags = Vec::new();
    let mut depth = 0;
    let mut tag = String::new();
    for c in stem.chars() {
        match c {
            '(' | '[' => {
                depth += 1;
                if depth == 1 {
                    tag.clear();
                    continue;
                }
            }
            ')' | ']' if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    let t = tag.trim();
                    if !t.is_empty() {
                        tags.push(t.to_owned());
                    }
                    continue;
                }
            }
            _ => {}
        }
        if depth > 0 { tag.push(c) } else { title.push(c) }
    }
    let mut title = title.split_whitespace().collect::<Vec<_>>().join(" ");
    for article in ["The", "A", "An"] {
        if let Some(rest) = title.strip_suffix(&format!(", {article}")) {
            title = format!("{article} {rest}");
            break;
        }
        // "Legend of Zelda, The - A Link to the Past"
        let infix = format!(", {article} - ");
        if let Some(pos) = title.find(&infix) {
            title = format!("{article} {} - {}", &title[..pos], &title[pos + infix.len()..]);
            break;
        }
    }
    if title.is_empty() {
        title = stem.trim().to_owned();
    }
    (title, tags.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_and_aliases() {
        assert_eq!(by_folder("nes").unwrap().id, "nes");
        assert_eq!(by_folder("Genesis").unwrap().id, "megadrive");
        assert_eq!(by_folder("Nintendo - Game Boy Advance").unwrap().id, "gba");
        assert_eq!(by_folder("psx").unwrap().id, "psx");
        assert_eq!(by_folder("arcade").unwrap().id, "fbneo");
        assert_eq!(by_folder("n64").unwrap().id, "n64");
        assert!(by_folder("n64").unwrap().needs_gpu);
        assert!(!by_folder("snes").unwrap().needs_gpu);
        assert!(by_folder("saturn").is_none());
        assert!(is_unsupported_folder("Saturn"));
        assert!(!is_unsupported_folder("ps2"), "PS2 is supported now");
    }

    #[test]
    fn extensions_only_when_unambiguous() {
        assert_eq!(by_extension("SFC").unwrap().id, "snes");
        assert_eq!(by_extension("gbc").unwrap().id, "gbc");
        assert!(by_extension("bin").is_none());
        assert!(by_extension("zip").is_none(), "arcade and Neo Geo both claim .zip");
        assert!(by_extension("chd").is_none(), "PS1, PS2 and Dreamcast discs are all .chd");
        assert_eq!(by_extension("z64").unwrap().id, "n64");
    }

    #[test]
    fn ids_are_unique_and_every_system_has_a_core() {
        for (i, s) in SYSTEMS.iter().enumerate() {
            assert!(SYSTEMS[i + 1..].iter().all(|o| o.id != s.id), "{}", s.id);
            assert!(!s.cores.is_empty(), "{}", s.id);
            assert!(s.folders.contains(&s.id), "{} folder", s.id);
        }
    }

    #[test]
    fn titles() {
        assert_eq!(
            parse_title("Legend of Zelda, The (USA) (Rev 1) [!]"),
            ("The Legend of Zelda".into(), "USA, Rev 1, !".into())
        );
        assert_eq!(
            parse_title("Legend of Zelda, The - A Link to the Past (USA)"),
            ("The Legend of Zelda - A Link to the Past".into(), "USA".into())
        );
        assert_eq!(parse_title("uCity"), ("uCity".into(), String::new()));
        assert_eq!(
            parse_title("Game (Europe) (En,Fr,De)"),
            ("Game".into(), "Europe, En,Fr,De".into())
        );
        assert_eq!(parse_title("(Proto)"), ("(Proto)".into(), "Proto".into()));
    }
}
