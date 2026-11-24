//! The cores SovEmu ships, and their licences: one table, read by the admin Cores and About
//! pages. The image's cores come from one libretro stable release (`scripts/cores.list`); a
//! test keeps this table and that list in step, so moving to a new release, or adding a core,
//! means checking the licence here too (as it means bumping `FBNEO_COMMIT`).
//!
//! Licences are as libretro's core info files (`libretro-core-info`, `<core>_libretro.info`)
//! give them.

/// The libretro stable release the image's cores come from (`release` in `scripts/cores.list`).
pub const LIBRETRO_RELEASE: &str = "1.22.2";

/// A shipped core.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreLicence {
    /// The core's file name without `_libretro.so`.
    pub name: &'static str,
    /// What the core calls itself.
    pub label: &'static str,
    /// As its core info file states it (`GPLv2`, `Non-commercial`, …).
    pub licence: &'static str,
    /// Its licence forbids commercial use: fine for SovEmu, which is not sold.
    pub non_commercial: bool,
    /// Its source: libretro's repository for it.
    pub source: &'static str,
}

const fn core(
    name: &'static str,
    label: &'static str,
    licence: &'static str,
    source: &'static str,
) -> CoreLicence {
    // The old MAME licence (PicoDrive's) forbids commercial use too.
    let non_commercial = matches!(licence.as_bytes(), b"Non-commercial" | b"MAME");
    CoreLicence { name, label, licence, non_commercial, source }
}

/// Every core in `scripts/cores.list`, in its order.
pub const CORES: &[CoreLicence] = &[
    core("fceumm", "FCEUmm", "GPLv2", "https://github.com/libretro/libretro-fceumm"),
    core("snes9x", "Snes9x", "Non-commercial", "https://github.com/libretro/snes9x"),
    core("mgba", "mGBA", "MPLv2.0", "https://github.com/libretro/mgba"),
    core("gambatte", "Gambatte", "GPLv2", "https://github.com/libretro/gambatte-libretro"),
    core(
        "genesis_plus_gx",
        "Genesis Plus GX",
        "Non-commercial",
        "https://github.com/libretro/Genesis-Plus-GX",
    ),
    core("pcsx_rearmed", "PCSX-ReARMed", "GPLv2", "https://github.com/libretro/pcsx_rearmed"),
    core("fbneo", "FinalBurn Neo", "Non-commercial", "https://github.com/libretro/FBNeo"),
    core("picodrive", "PicoDrive", "MAME", "https://github.com/libretro/picodrive"),
    core(
        "mednafen_pce_fast",
        "Beetle PCE Fast",
        "GPLv2",
        "https://github.com/libretro/beetle-pce-fast-libretro",
    ),
    core("stella", "Stella", "GPLv2", "https://github.com/stella-emu/stella"),
    core("prosystem", "ProSystem", "GPLv2", "https://github.com/libretro/prosystem-libretro"),
    core("handy", "Handy", "Zlib", "https://github.com/libretro/libretro-handy"),
    core(
        "mednafen_ngp",
        "Beetle NeoPop",
        "GPLv2",
        "https://github.com/libretro/beetle-ngp-libretro",
    ),
    core(
        "mednafen_wswan",
        "Beetle WonderSwan",
        "GPLv2",
        "https://github.com/libretro/beetle-wswan-libretro",
    ),
    core("mednafen_vb", "Beetle VB", "GPLv2", "https://github.com/libretro/beetle-vb-libretro"),
    core("freeintv", "FreeIntv", "GPLv2+", "https://github.com/libretro/FreeIntv"),
    core("gearcoleco", "Gearcoleco", "GPLv3", "https://github.com/drhelius/Gearcoleco"),
    core("vecx", "vecx", "GPLv3", "https://github.com/libretro/libretro-vecx"),
    core(
        "mupen64plus_next",
        "Mupen64Plus-Next",
        "GPLv2",
        "https://github.com/libretro/mupen64plus-libretro-nx",
    ),
    core("flycast", "Flycast", "GPLv2", "https://github.com/libretro/flycast"),
    core("dolphin", "Dolphin", "GPLv2+", "https://github.com/libretro/dolphin"),
    core("pcsx2", "LRPS2", "GPLv3", "https://github.com/libretro/ps2"),
];

/// A shipped core's licence, by name (`snes9x`).
pub fn licence(name: &str) -> Option<&'static CoreLicence> {
    CORES.iter().find(|c| c.name == name)
}

/// What a shipped core's worker needs in its environment, beyond SovEmu's allow-list, by core
/// name (`mupen64plus_next`): workarounds for bugs in this release's builds of the core, each
/// naming the bug. Moving to a new release means checking whether each is still needed.
pub fn worker_env(name: &str) -> &'static [(&'static str, &'static str)] {
    match name {
        // Release 1.22.2's Mupen64Plus-Next (680e033): GLideN64 draws only its clear colour, a
        // black picture with the sound playing, when Mesa offers framebuffer fetch (iris,
        // llvmpipe). Fixed upstream after that build (libretro/mupen64plus-libretro-nx#620,
        // #610); hiding the extensions from Mesa puts the picture back.
        "mupen64plus_next" => &[(
            "MESA_EXTENSION_OVERRIDE",
            "-GL_EXT_shader_framebuffer_fetch -GL_EXT_shader_framebuffer_fetch_non_coherent",
        )],
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The image's list. The container's test stage copies it to the same relative place.
    const CORES_LIST: &str = include_str!("../../../scripts/cores.list");

    #[test]
    fn every_shipped_core_has_a_licence_and_nothing_else_does() {
        let listed: Vec<&str> = CORES_LIST
            .lines()
            .filter_map(|l| l.trim().strip_prefix("core "))
            .map(str::trim)
            .collect();
        let known: Vec<&str> = CORES.iter().map(|c| c.name).collect();
        assert_eq!(listed, known, "scripts/cores.list and CORES differ: check the licences");
    }

    #[test]
    fn the_release_is_the_one_the_image_installs() {
        let release = CORES_LIST
            .lines()
            .find_map(|l| l.trim().strip_prefix("release "))
            .and_then(|r| r.split_whitespace().next());
        assert_eq!(release, Some(LIBRETRO_RELEASE));
    }

    #[test]
    fn workarounds_are_for_shipped_cores() {
        assert!(licence("mupen64plus_next").is_some(), "a workaround for a core not shipped");
        let n64 = worker_env("mupen64plus_next");
        assert!(n64.iter().any(|(k, v)| *k == "MESA_EXTENSION_OVERRIDE"
            && v.contains("-GL_EXT_shader_framebuffer_fetch")));
        assert!(worker_env("snes9x").is_empty());
    }

    #[test]
    fn non_commercial_cores_are_marked() {
        let marked: Vec<&str> = CORES.iter().filter(|c| c.non_commercial).map(|c| c.name).collect();
        assert_eq!(marked, ["snes9x", "genesis_plus_gx", "fbneo", "picodrive"]);
        assert_eq!(licence("mgba").map(|c| c.licence), Some("MPLv2.0"));
        assert_eq!(licence("nope"), None);
    }
}
