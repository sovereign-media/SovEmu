//! The data folder's layout (`SovEmu__Paths__Data`, `/sov-data` in the image), in one place. It holds
//! everything SovEmu writes and nothing it ships: the image's own files are in `/opt/sovemu`,
//! whose `artwork/` has the same layout as the one here, so a file here replaces the built-in one
//! at the same path (`sovemu_library::layout`).
//!
//! ```text
//! <data>/
//!   sovemu.db             the database (SQLite, WAL)
//!   server.json           the asset and token signing keys, if not set, and the OAuth clients'
//!                         ids (sovemu_assets)
//!   artwork/              every image (see sovemu_library::layout):
//!     backgrounds/          backgrounds of your own (WebP, PNG or JPEG), by file name
//!     systems/              system photos of your own, `<system id>.webp`
//!     decorations/          decoration sets of your own (Batocera's layout)
//!     ratings/esrb/         ESRB icons of your own, `<rating>.png`
//!     games/<game id>/      downloaded box art, screenshot, logo and video
//!   saves/<user>/<system>/<game>.srm            battery saves
//!   saves/<user>/<system>/states/<game>/        save states and their screenshots
//!   bios/                 BIOS files in any layout, unless SovEmu__Paths__Bios points elsewhere
//!   metadata/fbneo/       FBNeo's DATs, by commit
//!   backups/              backups, unless SovEmu__Backups__Directory points elsewhere
//!   restored.json         the backup last restored (SovEmu__Backups__RestoreFrom), so it is restored once
//!   before-restore-<time>/  what a restore replaced, kept
//!   library/              mode `mirror`: the local copy of the library
//!   roms/                 the library when SovEmu__Paths__Library is not set (development)
//!   cache/                made again as needed:
//!     games/                mode `cache`: games copied from the library as they are played
//!     discs/                generated .m3u playlists for multi-disc games
//!     artwork/games/<id>/   artwork copies at the widths clients asked for (WebP)
//!     builtin-artwork/      the built-in artwork, when the image does not hold it (development)
//! ```
//!
//! A new file or folder in the data directory gets its method here.

use std::path::{Path, PathBuf};

use sovemu_library::layout::ArtworkDir;

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

    /// The library when `SovEmu__Paths__Library` is not set.
    pub fn roms(&self) -> PathBuf {
        self.root.join("roms")
    }

    /// Mode `mirror`'s local copy of the library.
    pub fn mirror(&self) -> PathBuf {
        self.root.join("library")
    }

    /// BIOS files when `SovEmu__Paths__Bios` is not set.
    pub fn bios(&self) -> PathBuf {
        self.root.join("bios")
    }

    /// A user's battery saves for a system; states are in `states/<game>/` below it.
    pub fn saves(&self, user_id: i64, system: &str) -> PathBuf {
        self.user_saves(user_id).join(system)
    }

    /// One user's saves, every system's (a folder named by the user's id).
    pub fn user_saves(&self, user_id: i64) -> PathBuf {
        self.all_saves().join(user_id.to_string())
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

    /// The play cache's copies of the library's own artwork, from before artwork came only from
    /// ScreenScraper: removed at startup.
    pub fn removed_cache_artwork(&self) -> PathBuf {
        self.cache_games().join("media")
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

    /// The built-in artwork when the image does not hold it (`SovEmu__Paths__BuiltInArtwork` unset):
    /// written here at startup, as the image writes `/opt/sovemu/artwork` at build.
    pub fn builtin_artwork(&self) -> PathBuf {
        self.cache().join("builtin-artwork")
    }

    /// Every image of the admin's own, and the game art SovEmu downloads.
    pub fn artwork(&self) -> ArtworkDir {
        ArtworkDir::new(self.root.join("artwork"))
    }

    /// Folders from before images had one of their own, and where each moved to: moved at
    /// startup (`metadata::tidy`). The downloaded game art (`metadata/media`) is moved file by
    /// file, renamed into a folder per game.
    pub fn moved_artwork(&self) -> [(PathBuf, PathBuf); 3] {
        let art = self.artwork();
        [
            (self.root.join("decorations"), art.decorations()),
            (self.root.join("backgrounds"), art.backgrounds()),
            (self.root.join("systems"), art.systems()),
        ]
    }

    /// Where downloaded game art was kept, `<game id>-<box|shot|marquee|video>.<ext>`, before
    /// `artwork/games/<game id>/`: moved at startup.
    pub fn moved_game_art(&self) -> PathBuf {
        self.metadata().join("media")
    }

    /// Caches from before the layout changed, deleted at startup: the built-in decorations
    /// (now in [`Self::builtin_artwork`]) and the artwork copies (now in
    /// [`Self::artwork_widths`]).
    pub fn removed_caches(&self) -> [PathBuf; 2] {
        [self.cache().join("decorations"), self.cache().join("artwork-widths")]
    }

    /// Game information: the services' downloads.
    pub fn metadata(&self) -> PathBuf {
        self.root.join("metadata")
    }

    /// Where LaunchBox's database dump was kept before LaunchBox stopped being a source; deleted
    /// at startup if it is still there.
    pub fn removed_launchbox(&self) -> PathBuf {
        self.metadata().join("launchbox")
    }

    /// FBNeo's DAT files (arcade names), one folder per pinned commit.
    pub fn fbneo_dats(&self) -> PathBuf {
        self.metadata().join("fbneo")
    }

    /// Game art downloaded by the game-information lookup, a folder per game.
    pub fn game_artwork(&self) -> PathBuf {
        self.artwork().games()
    }

    /// Artwork copies at the widths clients asked for (lossy WebP), made from the originals on
    /// first request and kept, as `games/<id>/<kind>/w<width>.webp`; cleared with the cache.
    pub fn artwork_widths(&self) -> PathBuf {
        self.cache().join("artwork")
    }

    /// Backups, when `SovEmu__Backups__Directory` is not set (`backup`).
    pub fn backups(&self) -> PathBuf {
        self.root.join("backups")
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
        assert_eq!(p.game_artwork(), Path::new("/data/artwork/games"));
        assert_eq!(p.artwork().esrb(), Path::new("/data/artwork/ratings/esrb"));
        assert_eq!(p.artwork().decorations(), Path::new("/data/artwork/decorations"));
        assert_eq!(p.artwork_widths(), Path::new("/data/cache/artwork"));
        assert_eq!(p.saves(3, "snes"), Path::new("/data/saves/3/snes"));
        assert_eq!(p.disc_playlists(), Path::new("/data/cache/discs"));
        assert_eq!(p.removed_launchbox(), Path::new("/data/metadata/launchbox"));
        assert_eq!(p.mirror(), Path::new("/data/library"));
        assert_eq!(p.cache_games(), Path::new("/data/cache/games"));
        assert_eq!(p.removed_cache_artwork(), Path::new("/data/cache/games/media"));
    }

    #[test]
    fn the_game_cache_holds_nothing_else() {
        // The cache evicts every file under its folder: nothing else may live there.
        let p = DataPaths::new("/data");
        for other in [p.disc_playlists(), p.builtin_artwork(), p.database(), p.metadata()] {
            assert!(
                !other.starts_with(p.cache_games()),
                "{} is in the game cache",
                other.display()
            );
        }
    }
}
