//! What this server is: its version and build, and what it is made with and draws on (the
//! game-information services, the components bundled in the image), for the admin About page.
//! The cores and their licences are `sovemu_library::cores`, shown from there.

/// The build, as the image's build stage passes it (`SOVEMU_REVISION`, `SOVEMU_BUILT`); a
/// local build has neither.
pub(crate) struct BuildInfo {
    pub(crate) version: &'static str,
    /// The commit it was built from.
    pub(crate) revision: Option<&'static str>,
    /// The commit's time (ISO 8601).
    pub(crate) built: Option<&'static str>,
    pub(crate) libretro_release: &'static str,
    /// SovEmu's own licence (SPDX), and where its source is: the AGPL asks that everyone using
    /// the server over a network be offered the source of the version they use.
    pub(crate) licence: &'static str,
    pub(crate) source: &'static str,
}

const fn non_empty(value: Option<&'static str>) -> Option<&'static str> {
    match value {
        Some(v) if !v.is_empty() => Some(v),
        _ => None,
    }
}

pub(crate) const BUILD: BuildInfo = BuildInfo {
    version: env!("CARGO_PKG_VERSION"),
    revision: non_empty(option_env!("SOVEMU_REVISION")),
    built: non_empty(option_env!("SOVEMU_BUILT")),
    libretro_release: sovemu_library::cores::LIBRETRO_RELEASE,
    licence: env!("CARGO_PKG_LICENSE"),
    source: env!("CARGO_PKG_REPOSITORY"),
};

/// A service SovEmu looks games up in.
pub(crate) struct Service {
    pub(crate) name: &'static str,
    /// What SovEmu takes from it.
    pub(crate) provides: &'static str,
    pub(crate) url: &'static str,
    /// The data's licence, where the service states one.
    pub(crate) licence: Option<&'static str>,
    pub(crate) licence_url: Option<&'static str>,
    /// What its terms ask of an app, or that they state nothing.
    pub(crate) terms: &'static str,
}

/// The game-information services, as their own pages state their terms (checked 2026-11-22).
pub(crate) const SERVICES: &[Service] = &[
    Service {
        name: "ScreenScraper",
        provides: "Game information, box art and media, with your own developer account",
        url: "https://www.screenscraper.fr",
        licence: Some("CC BY-NC-SA 4.0"),
        licence_url: Some("https://creativecommons.org/licenses/by-nc-sa/4.0/"),
        terms: "Its API may be built only into entirely free applications; its content is \
                licensed CC BY-NC-SA 4.0, drawing on Wikipedia, GameFAQs, MobyGames, GameTDB \
                and others.",
    },
    Service {
        name: "FinalBurn Neo",
        provides: "Arcade game names and years, from its DAT files at a pinned commit",
        url: "https://neo-source.com",
        licence: Some("FinalBurn Neo licence (non-commercial)"),
        licence_url: Some("https://github.com/libretro/FBNeo/blob/master/src/license.txt"),
        terms: "No selling, renting or profiting from it, and no asking for donations for a \
                project that uses its source.",
    },
];

/// A component the image ships besides the cores.
pub(crate) struct Component {
    pub(crate) name: &'static str,
    pub(crate) what: &'static str,
    /// SPDX, where there is one.
    pub(crate) licence: &'static str,
    pub(crate) url: &'static str,
    /// The commit it is taken from, when pinned (`scripts/cores.list`).
    pub(crate) commit: Option<&'static str>,
}

/// Dolphin's `Data/Sys` and PCSX2's resources, at the commits `scripts/cores.list` pins.
const DOLPHIN_SYS_COMMIT: &str = "0863ee3700978e826b0d95b72d4742e537a7325b";
const PCSX2_RESOURCES_COMMIT: &str = "9691a4f995862436c3d25ce686ac3bd02065ad62";

pub(crate) const COMPONENTS: &[Component] = &[
    Component {
        name: "Mesa",
        what: "OpenGL and EGL: software rendering, and AMD and Intel GPUs",
        licence: "MIT",
        url: "https://www.mesa3d.org",
        commit: None,
    },
    Component {
        name: "libvpx",
        what: "VP8 video encoding",
        licence: "BSD-3-Clause",
        url: "https://www.webmproject.org",
        commit: None,
    },
    Component {
        name: "Opus",
        what: "Audio encoding",
        licence: "BSD-3-Clause",
        url: "https://opus-codec.org",
        commit: None,
    },
    Component {
        name: "FFmpeg (libavcodec, Debian's build)",
        what: "H.264 video encoding: VA-API, NVENC or x264",
        licence: "GPL-2.0-or-later",
        url: "https://ffmpeg.org",
        commit: None,
    },
    Component {
        name: "x264",
        what: "H.264 encoding in software",
        licence: "GPL-2.0-or-later",
        url: "https://www.videolan.org/developers/x264.html",
        commit: None,
    },
    Component {
        name: "Dolphin Sys",
        what: "Dolphin's fonts and game settings (GameCube)",
        licence: "GPL-2.0-or-later",
        url: "https://dolphin-emu.org",
        commit: Some(DOLPHIN_SYS_COMMIT),
    },
    Component {
        name: "PCSX2 game database",
        what: "PCSX2's per-game fixes (PlayStation 2)",
        licence: "GPL-3.0-or-later",
        url: "https://pcsx2.net",
        commit: Some(PCSX2_RESOURCES_COMMIT),
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    /// The image's list, whose `data` lines pin the components' commits.
    const CORES_LIST: &str = include_str!("../../../scripts/cores.list");

    #[test]
    fn pinned_components_are_the_commits_the_image_installs() {
        let pinned: Vec<&str> = CORES_LIST
            .lines()
            .filter_map(|l| l.trim().strip_prefix("data "))
            .filter_map(|l| l.split_whitespace().nth(1))
            .collect();
        let ours: Vec<&str> = COMPONENTS.iter().filter_map(|c| c.commit).collect();
        assert_eq!(pinned, ours, "scripts/cores.list moved a data commit: update about.rs");
    }

    #[test]
    fn a_local_build_has_no_revision() {
        assert!(BUILD.revision.is_none_or(|r| !r.is_empty()));
        assert_eq!(BUILD.libretro_release, sovemu_library::cores::LIBRETRO_RELEASE);
    }
}
