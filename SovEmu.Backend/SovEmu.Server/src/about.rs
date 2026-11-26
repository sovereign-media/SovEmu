//! What this server is: its version and build, and what it is made with and draws on (the
//! game-information services, the components bundled in the image), for the admin About page.
//! The cores and their licences are `sovemu_library::cores`, shown from there.

use std::path::PathBuf;
use std::sync::OnceLock;

use serde::Deserialize;

/// The build: what is compiled in, and the stamp the image writes beside it ([`stamp`]).
pub(crate) struct BuildInfo {
    pub(crate) version: &'static str,
    pub(crate) libretro_release: &'static str,
    /// SovEmu's own licence (SPDX), and where its source is: the AGPL asks that everyone using
    /// the server over a network be offered the source of the version they use.
    pub(crate) licence: &'static str,
    pub(crate) source: &'static str,
}

pub(crate) const BUILD: BuildInfo = BuildInfo {
    version: env!("CARGO_PKG_VERSION"),
    libretro_release: sovemu_library::cores::LIBRETRO_RELEASE,
    licence: env!("CARGO_PKG_LICENSE"),
    source: env!("CARGO_PKG_REPOSITORY"),
};

/// Which commit the image was built from, and when: `build.json`, which the image's runtime stage
/// writes beside the server binary. Never compiled in, so the release compile is the same for
/// every commit and the test and image builds share it. A local build has none.
#[derive(Debug, Default, Deserialize, PartialEq)]
pub(crate) struct Stamp {
    /// The commit it was built from.
    pub(crate) revision: Option<String>,
    /// The commit's time (ISO 8601).
    pub(crate) built: Option<String>,
}

impl Stamp {
    /// A stamp from `build.json`'s text; an empty field (a local image build passes none) is no
    /// value, and text that is not a stamp is none at all.
    fn parse(text: &str) -> Self {
        let stamp: Self = serde_json::from_str(text).unwrap_or_else(|e| {
            tracing::warn!(error = %e, "build.json is not a build stamp");
            Self::default()
        });
        let non_empty = |v: Option<String>| v.filter(|v| !v.trim().is_empty());
        Self { revision: non_empty(stamp.revision), built: non_empty(stamp.built) }
    }
}

/// This build's stamp, read once.
pub(crate) fn stamp() -> &'static Stamp {
    static STAMP: OnceLock<Stamp> = OnceLock::new();
    STAMP.get_or_init(|| {
        beside_exe("build.json")
            .and_then(|file| std::fs::read_to_string(file).ok())
            .map_or_else(Stamp::default, |text| Stamp::parse(&text))
    })
}

/// A file beside the server binary (`/opt/sovemu/bin` in the image), as the worker is found.
pub(crate) fn beside_exe(name: &str) -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    Some(exe.parent()?.join(name))
}

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
    fn the_build_names_the_libretro_release() {
        assert_eq!(BUILD.libretro_release, sovemu_library::cores::LIBRETRO_RELEASE);
    }

    #[test]
    fn a_stamp_is_read_from_build_json() {
        let stamp = Stamp::parse(r#"{"revision":"0e1bbf0","built":"2026-11-26T19:28:52Z"}"#);
        assert_eq!(stamp.revision.as_deref(), Some("0e1bbf0"));
        assert_eq!(stamp.built.as_deref(), Some("2026-11-26T19:28:52Z"));
    }

    #[test]
    fn an_empty_or_unreadable_stamp_is_none() {
        // A local `docker build` passes no commit: the runtime stage writes empty strings.
        assert_eq!(Stamp::parse(r#"{"revision":"","built":" "}"#), Stamp::default());
        assert_eq!(Stamp::parse("not json"), Stamp::default());
        assert_eq!(Stamp::parse("{}"), Stamp::default());
    }
}
