//! The data folder's layout (`SOVEMU_DATA_DIR`, `/data` in the image), in one place:
//!
//! ```text
//! <data>/
//!   sovemu.db             the database (SQLite, WAL)
//!   server.json           the asset signing key, if not set (sovemu_assets::load_key)
//!   roms/                 the library, unless SOVEMU_ROMS_DIR points elsewhere
//!   library/              mode `mirror`: the local copy of the library
//!   bios/<system>/        BIOS files, unless SOVEMU_BIOS_DIR points elsewhere
//!   saves/<user>/<system>/<game>.srm            battery saves
//!   saves/<user>/<system>/states/<game>/        save states and their screenshots
//!   cache/                mode `cache`: games copied from the library as they are played
//!   cache/discs/          generated .m3u playlists for multi-disc games
//!   cache/decorations/    the built-in decoration set, unless the image holds it
//!   decorations/          decoration sets of your own (Batocera's layout)
//!   metadata/             game information: LaunchBox's dump, FBNeo's DATs, downloaded art
//! ```
//!
//! A new file or folder in the data directory gets its method here.

use std::path::{Path, PathBuf};

/// The data folder, and where each thing lives in it.
#[derive(Debug, Clone)]
pub struct DataPaths {
    root: PathBuf,
}

impl DataPaths {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn database(&self) -> PathBuf {
        self.root.join("sovemu.db")
    }

    /// The library when `SOVEMU_ROMS_DIR` is not set.
    pub fn roms(&self) -> PathBuf {
        self.root.join("roms")
    }

    /// Mode `mirror`'s local copy of the library.
    pub fn mirror(&self) -> PathBuf {
        self.root.join("library")
    }

    /// BIOS files when `SOVEMU_BIOS_DIR` is not set.
    pub fn bios(&self) -> PathBuf {
        self.root.join("bios")
    }

    /// A user's battery saves for a system; states are in `states/<game>/` below it.
    pub fn saves(&self, user_id: i64, system: &str) -> PathBuf {
        self.root.join("saves").join(user_id.to_string()).join(system)
    }

    /// Mode `cache`'s copies of games.
    pub fn cache(&self) -> PathBuf {
        self.root.join("cache")
    }

    /// Playlists generated for multi-disc games.
    pub fn disc_playlists(&self) -> PathBuf {
        self.cache().join("discs")
    }

    /// The built-in decoration set when the image does not hold it.
    pub fn builtin_decorations(&self) -> PathBuf {
        self.cache().join("decorations")
    }

    /// Decoration sets of the admin's own.
    pub fn decorations(&self) -> PathBuf {
        self.root.join("decorations")
    }

    /// Game information: the services' downloads and box art.
    pub fn metadata(&self) -> PathBuf {
        self.root.join("metadata")
    }

    /// LaunchBox's database dump.
    pub fn launchbox(&self) -> PathBuf {
        self.metadata().join("launchbox")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_layout_is_as_documented() {
        let p = DataPaths::new("/data");
        assert_eq!(p.database(), Path::new("/data/sovemu.db"));
        assert_eq!(p.saves(3, "snes"), Path::new("/data/saves/3/snes"));
        assert_eq!(p.disc_playlists(), Path::new("/data/cache/discs"));
        assert_eq!(p.launchbox(), Path::new("/data/metadata/launchbox"));
        assert_eq!(p.mirror(), Path::new("/data/library"));
    }
}
