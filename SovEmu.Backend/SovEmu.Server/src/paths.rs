//! The data folder's layout (`SOVEMU_DATA_DIR`, `/data` in the image), in one place:
//!
//! ```text
//! <data>/
//!   sovemu.db             the database (SQLite, WAL)
//!   server.json           the asset and token signing keys, if not set (sovemu_assets)
//!   roms/                 the library, unless SOVEMU_ROMS_DIR points elsewhere
//!   library/              mode `mirror`: the local copy of the library
//!   bios/<system>/        BIOS files, unless SOVEMU_BIOS_DIR points elsewhere
//!   saves/<user>/<system>/<game>.srm            battery saves
//!   saves/<user>/<system>/states/<game>/        save states and their screenshots
//!   cache/games/          mode `cache`: games and art copied from the library as they are used
//!   cache/discs/          generated .m3u playlists for multi-disc games
//!   cache/decorations/    the built-in decoration set, unless the image holds it
//!   decorations/          decoration sets of your own (Batocera's layout)
//!   metadata/             game information:
//!   metadata/launchbox/   LaunchBox's database dump
//!   metadata/fbneo/       FBNeo's DATs, by commit
//!   metadata/media/       downloaded box art
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
        self.all_saves().join(user_id.to_string()).join(system)
    }

    /// What the server makes or copies for itself, and can make again: the folders below.
    pub fn cache(&self) -> PathBuf {
        self.root.join("cache")
    }

    /// Mode `cache`'s copies of games and their art. Its own folder, since the cache evicts
    /// everything in it: never put anything else there.
    pub fn cache_games(&self) -> PathBuf {
        self.cache().join("games")
    }

    /// Where copies were kept before they had a folder of their own (`cache/roms`,
    /// `cache/media`), moved into [`Self::cache_games`] at startup.
    pub fn legacy_cache_kinds(&self) -> [PathBuf; 2] {
        [self.cache().join("roms"), self.cache().join("media")]
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

    /// FBNeo's DAT files (arcade names), one folder per pinned commit.
    pub fn fbneo_dats(&self) -> PathBuf {
        self.metadata().join("fbneo")
    }

    /// Box art downloaded by the game-information lookup.
    pub fn artwork(&self) -> PathBuf {
        self.metadata().join("media")
    }

    /// The saves of every user, one folder per user id.
    pub fn all_saves(&self) -> PathBuf {
        self.root.join("saves")
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
        assert_eq!(p.cache_games(), Path::new("/data/cache/games"));
    }

    #[test]
    fn the_game_cache_holds_nothing_else() {
        // The cache evicts every file under its folder: nothing else may live there.
        let p = DataPaths::new("/data");
        for other in [p.disc_playlists(), p.builtin_decorations(), p.database(), p.metadata()] {
            assert!(
                !other.starts_with(p.cache_games()),
                "{} is in the game cache",
                other.display()
            );
        }
    }
}
