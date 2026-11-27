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
    /// Core options SovEmu sets for it only while its games render on a hardware GPU, an
    /// integrated one included: the enhancements (texture filtering, shaders compiled in the
    /// background) that a GPU draws easily and Mesa's software renderer, or a hypervisor's
    /// adapter, would draw too slowly to play. They apply on top of `options`. Never a higher
    /// internal resolution: every one of its pixels is encoded, sent and decoded, and at 2× a
    /// Fire TV could not decode the stream at 60 fps (18–26 ms a frame) while the server's read
    /// back and encode outgrew the frame (sovereign-media/SovEmu#296).
    #[serde(skip)]
    pub gpu_options: &'static [(&'static str, &'static str)],
    /// A 3D system: its cores render with OpenGL, on a GPU or in software (`SovEmu__Rendering__Gpu`).
    pub needs_gpu: bool,
}

impl System {
    /// The shape of its games' boxes, width over height, as most of a library has them (the US
    /// and European releases): the system's game cards take it, so box art fills them. Other
    /// regions' boxes (a Super Famicom's, portrait) are fitted inside.
    pub fn box_aspect(&self) -> f64 {
        match self.id {
            // Landscape boxes.
            "snes" | "n64" | "virtualboy" => 1.4,
            // CD jewel cases, square.
            "psx" | "segacd" | "pcenginecd" | "dreamcast" => 1.0,
            // Handhelds' small boxes, nearly square.
            "gb" | "gbc" | "gba" | "ngp" | "ngpc" | "wswan" | "wswanc" => 0.9,
            // DVD cases.
            "ps2" | "gamecube" => 0.7,
            // Cartridge boxes and clamshells, portrait; arcade flyers too.
            _ => 0.72,
        }
    }

    /// Whether its games can run ahead (`SovEmu__Systems__<id>__RunAhead`): each frame shown
    /// then costs several emulated frames and a save state, which only the systems that render
    /// in software can afford.
    pub fn run_ahead(&self) -> bool {
        !self.needs_gpu
    }

    /// The frames its games run ahead when neither the environment nor an admin says:
    /// [`DEFAULT_RUN_AHEAD`] where it can run ahead, else 0.
    pub fn default_run_ahead(&self) -> u32 {
        if self.run_ahead() { DEFAULT_RUN_AHEAD } else { 0 }
    }

    /// How many times its games' pictures are scaled up by repeating pixels before they are
    /// encoded, when neither `SovEmu__Systems__<id>__Prescale` nor
    /// `SovEmu__Streaming__Video__Prescale` says. 2 on the systems measured to gain from it
    /// (sovereign-media/SovEmu#263 and #266: with each console pixel a 2×2 block, 4:2:0 keeps its
    /// colour), else 1. The original Game Boy has no colour to keep. PlayStation games stay at 1:
    /// its 2D test suite gains as these do, but its games are mostly 3D, which was not measured.
    pub fn default_prescale(&self) -> u32 {
        match self.id {
            "nes" | "snes" | "gbc" | "gba" | "megadrive" | "pcengine" => 2,
            _ => 1,
        }
    }
}

/// What a session's worker may use when neither the environment nor an admin says
/// (`SovEmu__Systems__<id>__Limits__*`, `SovEmu__Sessions__Limits__*`; 0 is no limit): several
/// times what each system's sessions were measured to use, in sovereign-media/SovEmu#282
/// (docs/research/worker-limits-2026-11.md), so only a runaway core reaches them.
impl System {
    /// Memory held (resident and swapped out), MiB. The 2D systems and the PlayStation used
    /// 80–145 MiB with rewind's history full; the Dreamcast 300 MiB and a GameCube booting 460
    /// MiB, under Mesa's software renderer. The N64 and Dreamcast have a few MiB of RAM, so 2 GiB
    /// is several times their games and still stops the N64 core that ran away to 2.4 GiB; the
    /// GameCube and PlayStation 2 (unmeasured: no free BIOS or game) get 4 GiB.
    pub fn default_memory_mb(&self) -> u64 {
        match self.id {
            "n64" | "dreamcast" => 2048,
            _ if self.needs_gpu => 4096,
            _ => 1024,
        }
    }

    /// Threads: 12–16 measured on the 2D systems; 33–49 on the 3D ones on 12 CPUs, where
    /// Mesa's software renderer starts one per CPU, so a bigger host needs more.
    pub fn default_threads(&self) -> u32 {
        if self.needs_gpu { 256 } else { 64 }
    }

    /// CPU time, in CPUs: the 2D systems used at most 0.7 of one; the 3D ones none, since
    /// without a GPU they render on every CPU there is.
    pub fn default_cpus(&self) -> f64 {
        if self.needs_gpu { 0.0 } else { 2.0 }
    }
}

/// Run-ahead's default on the systems that support it. One frame hides most of the games' own
/// lag, and the worker runs fewer if a core cannot keep up (or none if it cannot save its state),
/// so it is safe to have on. Two would hide more in some games but skips ahead visibly in others.
const DEFAULT_RUN_AHEAD: u32 = 1;

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
        // Every sprite on a line is drawn, past the console's limit (8 here, 16 on a PC Engine,
        // 20 on a Mega Drive), so busy scenes stop flickering. The NES, Sega (Genesis Plus GX)
        // and PC Engine systems all set it. The few games that used the limit to hide a sprite
        // (a doorway, a title screen) show it for a moment; turn it off for the system to undo.
        // Sound resampled at the core's high quality, not its default low, for very little CPU.
        options: &[("fceumm_nospritelimit", "enabled"), ("fceumm_sndquality", "High")],
        gpu_options: &[],
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
        gpu_options: &[],
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
        gpu_options: &[],
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
        gpu_options: &[],
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
        // Blends only the pixels that change every other frame: some games flicker a sprite to
        // make it look transparent, as the console's slow LCD showed it. Unlike blending every
        // frame, motion does not smear.
        options: &[("mgba_interframe_blending", "mix_smart")],
        gpu_options: &[],
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
        options: &[("genesis_plus_gx_no_sprite_limit", "enabled")],
        gpu_options: &[],
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
        options: &[("genesis_plus_gx_no_sprite_limit", "enabled")],
        gpu_options: &[],
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
        // Gaussian sound interpolation, as the console's own SPU does, for next to no CPU.
        options: &[
            ("pcsx_rearmed_cd_readahead", "256"),
            ("pcsx_rearmed_spu_interpolation", "gaussian"),
        ],
        gpu_options: &[],
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
        gpu_options: &[],
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
        gpu_options: &[],
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
        options: &[("genesis_plus_gx_no_sprite_limit", "enabled")],
        gpu_options: &[],
        needs_gpu: false,
    },
    System {
        id: "sg1000",
        name: "SG-1000",
        short: "SG",
        manufacturer: "Sega",
        year: 1983,
        color: "#b33939",
        folders: &["sg1000", "sg-1000", "sega - sg-1000"],
        extensions: &["sg"],
        cores: &["genesis_plus_gx"],
        options: &[("genesis_plus_gx_no_sprite_limit", "enabled")],
        gpu_options: &[],
        needs_gpu: false,
    },
    System {
        id: "segacd",
        name: "Mega-CD / Sega CD",
        short: "MCD",
        manufacturer: "Sega",
        year: 1991,
        color: "#34495e",
        folders: &["segacd", "megacd", "sega - mega-cd - sega cd", "sega - mega cd - sega cd"],
        extensions: &["chd", "cue", "m3u", "iso"],
        cores: &["genesis_plus_gx"],
        // Backup RAM per game in the player's save folder, not one file per BIOS region.
        options: &[
            ("genesis_plus_gx_system_bram", "per game"),
            ("genesis_plus_gx_no_sprite_limit", "enabled"),
        ],
        gpu_options: &[],
        needs_gpu: false,
    },
    System {
        id: "sega32x",
        name: "32X",
        short: "32X",
        manufacturer: "Sega",
        year: 1994,
        color: "#c0392b",
        folders: &["sega32x", "32x", "sega - 32x"],
        extensions: &["32x"],
        cores: &["picodrive"],
        options: &[],
        gpu_options: &[],
        needs_gpu: false,
    },
    System {
        id: "pcengine",
        name: "PC Engine / TurboGrafx-16",
        short: "PCE",
        manufacturer: "NEC",
        year: 1987,
        color: "#e84393",
        folders: &["pcengine", "tg16", "turbografx16", "nec - pc engine - turbografx 16"],
        extensions: &["pce"],
        cores: &["mednafen_pce_fast"],
        options: &[("pce_fast_nospritelimit", "enabled")],
        gpu_options: &[],
        needs_gpu: false,
    },
    System {
        id: "pcenginecd",
        name: "PC Engine CD / TurboGrafx-CD",
        short: "PCECD",
        manufacturer: "NEC",
        year: 1988,
        color: "#a29bfe",
        folders: &["pcenginecd", "tg16cd", "turbografxcd", "nec - pc engine cd - turbografx-cd"],
        extensions: &["chd", "cue", "ccd", "toc", "m3u"],
        cores: &["mednafen_pce_fast"],
        options: &[("pce_fast_nospritelimit", "enabled")],
        gpu_options: &[],
        needs_gpu: false,
    },
    System {
        id: "atari2600",
        name: "Atari 2600",
        short: "2600",
        manufacturer: "Atari",
        year: 1977,
        color: "#8e5b3c",
        folders: &["atari2600", "2600", "atari - 2600"],
        extensions: &["a26", "bin"],
        cores: &["stella"],
        options: &[],
        gpu_options: &[],
        needs_gpu: false,
    },
    System {
        id: "atari7800",
        name: "Atari 7800",
        short: "7800",
        manufacturer: "Atari",
        year: 1986,
        color: "#d35400",
        folders: &["atari7800", "7800", "atari - 7800"],
        extensions: &["a78", "bin"],
        cores: &["prosystem"],
        options: &[],
        gpu_options: &[],
        needs_gpu: false,
    },
    System {
        id: "lynx",
        name: "Lynx",
        short: "LYNX",
        manufacturer: "Atari",
        year: 1989,
        color: "#f39c12",
        folders: &["lynx", "atarilynx", "atari - lynx"],
        extensions: &["lnx", "lyx"],
        cores: &["handy"],
        options: &[],
        gpu_options: &[],
        needs_gpu: false,
    },
    System {
        id: "ngp",
        name: "Neo Geo Pocket",
        short: "NGP",
        manufacturer: "SNK",
        year: 1998,
        color: "#7f8c8d",
        folders: &["ngp", "neogeopocket", "snk - neo geo pocket"],
        extensions: &["ngp"],
        cores: &["mednafen_ngp"],
        options: &[],
        gpu_options: &[],
        needs_gpu: false,
    },
    System {
        id: "ngpc",
        name: "Neo Geo Pocket Color",
        short: "NGPC",
        manufacturer: "SNK",
        year: 1999,
        color: "#0097e6",
        folders: &["ngpc", "neogeopocketcolor", "snk - neo geo pocket color"],
        extensions: &["ngc", "ngpc", "npc"],
        cores: &["mednafen_ngp"],
        options: &[],
        gpu_options: &[],
        needs_gpu: false,
    },
    System {
        id: "wswan",
        name: "WonderSwan",
        short: "WS",
        manufacturer: "Bandai",
        year: 1999,
        color: "#95a5a6",
        folders: &["wswan", "wonderswan", "bandai - wonderswan"],
        extensions: &["ws"],
        cores: &["mednafen_wswan"],
        options: &[],
        gpu_options: &[],
        needs_gpu: false,
    },
    System {
        id: "wswanc",
        name: "WonderSwan Color",
        short: "WSC",
        manufacturer: "Bandai",
        year: 2000,
        color: "#00a8ff",
        folders: &["wswanc", "wonderswancolor", "bandai - wonderswan color"],
        extensions: &["wsc", "pc2"],
        cores: &["mednafen_wswan"],
        options: &[],
        gpu_options: &[],
        needs_gpu: false,
    },
    System {
        id: "virtualboy",
        name: "Virtual Boy",
        short: "VB",
        manufacturer: "Nintendo",
        year: 1995,
        color: "#e74c3c",
        folders: &["virtualboy", "vb", "nintendo - virtual boy"],
        extensions: &["vb", "vboy"],
        cores: &["mednafen_vb"],
        options: &[],
        gpu_options: &[],
        needs_gpu: false,
    },
    System {
        id: "intellivision",
        name: "Intellivision",
        short: "INTV",
        manufacturer: "Mattel",
        year: 1979,
        color: "#a0522d",
        folders: &["intellivision", "intv", "mattel - intellivision"],
        extensions: &["int", "bin", "rom"],
        cores: &["freeintv"],
        options: &[],
        gpu_options: &[],
        needs_gpu: false,
    },
    System {
        id: "colecovision",
        name: "ColecoVision",
        short: "CV",
        manufacturer: "Coleco",
        year: 1982,
        color: "#16a085",
        folders: &["colecovision", "coleco", "coleco - colecovision"],
        extensions: &["col", "cv", "bin", "rom"],
        cores: &["gearcoleco"],
        options: &[],
        gpu_options: &[],
        needs_gpu: false,
    },
    System {
        id: "vectrex",
        name: "Vectrex",
        short: "VEC",
        manufacturer: "GCE",
        year: 1982,
        color: "#5f27cd",
        folders: &["vectrex", "gce - vectrex"],
        extensions: &["vec", "bin"],
        cores: &["vecx"],
        options: &[],
        gpu_options: &[],
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
        gpu_options: &[],
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
        // The console's own 640×480: at 2× a stream falls to 49 fps on the server alone.
        gpu_options: &[],
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
        // The console's own resolution, with 4× anisotropic filtering (`2`), which sharpens
        // textures without adding a pixel to the stream, and shaders compiled in the background,
        // drawn with ubershaders meanwhile (`2`: "Async (UberShaders)") rather than stalling the
        // frame for each new effect, which a stream shows as a dropped frame. Ubershaders are
        // heavy, so not on the software renderer.
        gpu_options: &[("dolphin_max_anisotropy", "2"), ("dolphin_shader_compilation_mode", "2")],
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
        // The console's own resolution (the core's default `pcsx2_upscale_multiplier`).
        gpu_options: &[],
        needs_gpu: true,
    },
];

pub fn by_id(id: &str) -> Option<&'static System> {
    SYSTEMS.iter().find(|s| s.id == id)
}

/// Batocera/Recalbox folders of systems SovEmu does not play. The scanner skips them rather
/// than guess from extensions: a Saturn or Neo Geo CD disc is a `.chd` or `.cue` too, and the
/// PlayStation core cannot run it.
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
    "pcfx",
    "ps3",
    "ps4",
    "psp",
    "psvita",
    "saturn",
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
    fn every_system_has_limits_and_3d_ones_more_memory() {
        // Several times what was measured (#282): 145 MiB on the 2D systems, 460 on the 3D.
        for s in SYSTEMS {
            let (memory, threads) = (s.default_memory_mb(), s.default_threads());
            assert!(memory >= 1024 && threads >= 64, "{}: {memory} MiB, {threads} threads", s.id);
            if s.needs_gpu {
                assert!(memory > 1024 && threads > 64, "{}: a 3D system gets more", s.id);
                assert_eq!(s.default_cpus(), 0.0, "{}: renders on every CPU there is", s.id);
            } else {
                assert!(s.default_cpus() >= 1.0, "{}", s.id);
            }
        }
        assert_eq!(by_id("nes").unwrap().default_memory_mb(), 1024);
        assert_eq!(by_id("gamecube").unwrap().default_memory_mb(), 4096);
    }

    #[test]
    fn every_system_has_a_box_shape() {
        for s in SYSTEMS {
            let a = s.box_aspect();
            assert!((0.5..=1.6).contains(&a), "{}: {a}", s.id);
        }
        assert!(by_id("snes").unwrap().box_aspect() > 1.0, "SNES boxes are landscape");
        assert!(by_id("nes").unwrap().box_aspect() < 1.0, "NES boxes are portrait");
    }

    #[test]
    fn the_3d_systems_stream_at_the_consoles_own_resolution() {
        let scales =
            ["reicast_internal_resolution", "dolphin_efb_scale", "pcsx2_upscale_multiplier"];
        for s in SYSTEMS {
            for (key, _) in s.options.iter().chain(s.gpu_options) {
                assert!(!scales.contains(key), "{}: {key} adds pixels to every frame", s.id);
            }
        }
        let gamecube = by_id("gamecube").unwrap();
        assert!(gamecube.gpu_options.contains(&("dolphin_max_anisotropy", "2")));
    }

    #[test]
    fn gpu_options_are_only_for_3d_systems_and_never_repeat_an_option() {
        for s in SYSTEMS {
            assert!(s.gpu_options.is_empty() || s.needs_gpu, "{}: draws on the CPU", s.id);
            for (key, _) in s.gpu_options {
                assert!(!s.options.iter().any(|(k, _)| k == key), "{}: {key} twice", s.id);
            }
        }
    }

    #[test]
    fn every_core_that_can_draw_past_the_sprite_limit_does() {
        let limits: &[(&str, &str)] = &[
            ("fceumm", "fceumm_nospritelimit"),
            ("genesis_plus_gx", "genesis_plus_gx_no_sprite_limit"),
            ("mednafen_pce_fast", "pce_fast_nospritelimit"),
        ];
        let mut seen = 0;
        for s in SYSTEMS {
            let Some((_, key)) = limits.iter().find(|(core, _)| s.cores.first() == Some(core))
            else {
                continue;
            };
            assert!(s.options.contains(&(*key, "enabled")), "{}: {key}", s.id);
            seen += 1;
        }
        assert_eq!(seen, 8, "NES, five Sega systems and both PC Engines");
    }

    #[test]
    fn the_measured_colour_systems_prescale_by_default() {
        let prescale = |id| by_id(id).unwrap().default_prescale();
        for id in ["nes", "snes", "gbc", "gba", "megadrive", "pcengine"] {
            assert_eq!(prescale(id), 2, "{id}");
        }
        assert_eq!(prescale("gb"), 1, "four greys: no colour to keep");
        assert_eq!(prescale("psx"), 1, "3D games not measured");
    }

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
        assert_eq!(by_folder("segacd").unwrap().id, "segacd");
        assert_eq!(by_folder("MegaCD").unwrap().id, "segacd");
        assert_eq!(by_folder("tg16").unwrap().id, "pcengine");
        assert_eq!(by_folder("wonderswancolor").unwrap().id, "wswanc");
        assert!(!is_unsupported_folder("segacd"), "Sega CD is supported now");
    }

    #[test]
    fn extensions_only_when_unambiguous() {
        assert_eq!(by_extension("SFC").unwrap().id, "snes");
        assert_eq!(by_extension("gbc").unwrap().id, "gbc");
        assert!(by_extension("bin").is_none());
        assert!(by_extension("zip").is_none(), "arcade and Neo Geo both claim .zip");
        assert!(by_extension("chd").is_none(), "PS1, PS2 and Dreamcast discs are all .chd");
        assert_eq!(by_extension("z64").unwrap().id, "n64");
        assert_eq!(by_extension("a26").unwrap().id, "atari2600");
        assert_eq!(by_extension("pce").unwrap().id, "pcengine");
        assert_eq!(by_extension("ngc").unwrap().id, "ngpc");
        assert!(by_extension("rom").is_none(), "Intellivision and ColecoVision both claim .rom");
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
