//! The artwork folder's layout, the same in the data folder (`<data>/artwork`, the admin's own
//! and what SovEmu downloads) and in the built-in one (`/opt/sovemu/artwork` in the image, what
//! SovEmu ships), so an image's path in one names its place in the other: a file in the data
//! folder replaces the built-in one at the same path.
//!
//! ```text
//! artwork/
//!   backgrounds/<id>.webp|png|jpg       behind every screen
//!   systems/<system>.webp|png|jpg       each system's photo
//!   decorations/<set>/…                 bezels (Batocera's layout; the built-in set is `sovemu`)
//!   ratings/esrb/<rating>.png           the ESRB's icons (ec, e, e10, t, m, ao, rp)
//!   games/<game id>/                    downloaded per game (data folder only):
//!     box.webp, shot.webp, marquee.webp, video.mp4
//! ```

use std::path::{Path, PathBuf};

/// An artwork folder (see the module's notes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtworkDir(PathBuf);

impl ArtworkDir {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self(root.into())
    }

    pub fn root(&self) -> &Path {
        &self.0
    }

    pub fn backgrounds(&self) -> PathBuf {
        self.0.join("backgrounds")
    }

    pub fn systems(&self) -> PathBuf {
        self.0.join("systems")
    }

    pub fn decorations(&self) -> PathBuf {
        self.0.join("decorations")
    }

    /// The ESRB's rating icons, `<rating>.png` ([`Esrb::file_stem`](crate::info::Esrb::file_stem)).
    pub fn esrb(&self) -> PathBuf {
        self.0.join("ratings").join("esrb")
    }

    /// Downloaded game art, a folder per game.
    pub fn games(&self) -> PathBuf {
        self.0.join("games")
    }
}

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

/// Writes the ESRB icons into `dir`'s `ratings/esrb/`.
pub fn write_esrb(dir: &ArtworkDir) -> std::io::Result<()> {
    let to = dir.esrb();
    std::fs::create_dir_all(&to)?;
    for (stem, png) in ESRB_BADGES {
        std::fs::write(to.join(format!("{stem}.png")), png)?;
    }
    Ok(())
}

/// The file of an ESRB rating's icon: the first of `dirs` (the data folder's, then the
/// built-in one) that has it. None if none does.
pub fn esrb_file(dirs: &[ArtworkDir], rating: crate::info::Esrb) -> Option<PathBuf> {
    let name = format!("{}.png", rating.file_stem());
    dirs.iter().map(|d| d.esrb().join(&name)).find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::info::Esrb;

    #[test]
    fn the_data_folder_replaces_a_built_in_icon() {
        let root = std::env::temp_dir().join(format!("sovemu-layout-esrb-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (data, builtin) =
            (ArtworkDir::new(root.join("data")), ArtworkDir::new(root.join("in")));
        write_esrb(&builtin).unwrap();
        for rating in Esrb::ALL {
            let file = esrb_file(std::slice::from_ref(&builtin), rating).unwrap();
            assert_eq!(&std::fs::read(file).unwrap()[1..4], b"PNG", "{rating:?}");
        }
        let dirs = [data.clone(), builtin.clone()];
        assert_eq!(esrb_file(&dirs, Esrb::Teen), Some(builtin.esrb().join("t.png")));
        std::fs::create_dir_all(data.esrb()).unwrap();
        std::fs::write(data.esrb().join("t.png"), b"mine").unwrap();
        assert_eq!(esrb_file(&dirs, Esrb::Teen), Some(data.esrb().join("t.png")), "same path wins");
        assert_eq!(esrb_file(&[data], Esrb::Mature), None);
        std::fs::remove_dir_all(root).ok();
    }
}
