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
    let non_commercial = matches!(licence.as_bytes(), b"Non-commercial");
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
    fn non_commercial_cores_are_marked() {
        let marked: Vec<&str> = CORES.iter().filter(|c| c.non_commercial).map(|c| c.name).collect();
        assert_eq!(marked, ["snes9x", "genesis_plus_gx", "fbneo"]);
        assert_eq!(licence("mgba").map(|c| c.licence), Some("MPLv2.0"));
        assert_eq!(licence("nope"), None);
    }
}
