//! The system (BIOS) directory a core sees.
//!
//! Some cores need data files of their own next to the BIOS files: Dolphin reads
//! `<system>/dolphin-emu/Sys`. SovEmu ships those in a `system/` folder beside the cores, but
//! the BIOS folder is usually the library's, read-only on a NAS. So when there is core data,
//! the worker gives the core a merged view instead: a temporary directory of symlinks to
//! everything in the BIOS folder, plus the shipped core data the BIOS folder lacks (the
//! library's own copy wins). A folder both have becomes a real folder of the merged contents,
//! which also lets a core create its working files there (they last the session). Nothing is
//! written to either source.
//!
//! Some cores look for BIOS files under a different name than Batocera's layout uses:
//! [`ALIASES`] links them (PCSX2 reads `pcsx2/bios`, Batocera keeps PS2 BIOSes in `ps2`).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// `(path a core reads, Batocera's folder)`, linked when the first is missing.
const ALIASES: &[(&str, &str)] = &[("pcsx2/bios", "ps2")];

/// A merged system directory, removed when dropped.
#[derive(Debug)]
pub struct SystemDir {
    path: PathBuf,
    /// Set when `path` is a temporary merge this worker made.
    owned: bool,
}

impl SystemDir {
    /// The system directory for `core`: `bios` itself, or a merge of `bios` and the core data
    /// shipped beside the core (`<core's folder>/system`), if there is any.
    pub fn for_core(core: &Path, bios: &Path) -> Result<Self> {
        let shipped = core.parent().map(|d| d.join("system")).filter(|d| d.is_dir());
        let Some(shipped) = shipped else {
            return Ok(Self { path: bios.to_owned(), owned: false });
        };
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let name = format!("sovemu-system-{}-{n}", std::process::id());
        let path = std::env::temp_dir().join(name);
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).with_context(|| format!("creating {}", path.display()))?;
        let dir = Self { path, owned: true };
        // The library's BIOS files first, so its copies win over the shipped ones.
        merge(&dir.path, &[bios, shipped.as_path()])?;
        for (wanted, batocera) in ALIASES {
            let source = bios.join(batocera);
            let link = dir.path.join(wanted);
            if source.is_dir() && link.symlink_metadata().is_err() {
                if let Some(parent) = link.parent() {
                    make_real_dir(parent)?;
                }
                symlink(&source, &link).with_context(|| format!("linking {}", link.display()))?;
            }
        }
        Ok(dir)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for SystemDir {
    fn drop(&mut self) {
        if self.owned {
            // Removes the links only, never what they point to.
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}

/// Fills `dest` with links to the entries of `sources`, earlier sources winning; a folder found
/// in more than one source becomes a real folder, merged the same way.
fn merge(dest: &Path, sources: &[&Path]) -> Result<()> {
    let mut names: Vec<std::ffi::OsString> = Vec::new();
    for source in sources {
        let Ok(entries) = std::fs::read_dir(source) else { continue };
        for entry in entries.flatten() {
            if !names.contains(&entry.file_name()) {
                names.push(entry.file_name());
            }
        }
    }
    for name in names {
        let found: Vec<PathBuf> =
            sources.iter().map(|s| s.join(&name)).filter(|p| p.exists()).collect();
        let link = dest.join(&name);
        if found.len() > 1 && found.iter().all(|p| p.is_dir()) {
            std::fs::create_dir(&link).with_context(|| format!("creating {}", link.display()))?;
            let nested: Vec<&Path> = found.iter().map(PathBuf::as_path).collect();
            merge(&link, &nested)?;
        } else if let Some(first) = found.first() {
            symlink(first, &link).with_context(|| format!("linking {}", link.display()))?;
        }
    }
    Ok(())
}

/// Makes `path` a real folder: creates it, or replaces a link to a folder with a folder of
/// links to its entries.
fn make_real_dir(path: &Path) -> Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() => {
            let target = std::fs::read_link(path)?;
            std::fs::remove_file(path)?;
            std::fs::create_dir(path)?;
            merge(path, &[target.as_path()])
        }
        Ok(_) => Ok(()),
        Err(_) => Ok(std::fs::create_dir_all(path)?),
    }
}

#[cfg(unix)]
fn symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(not(unix))]
fn symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_dir(target, link)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("sovemu-sysdir-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn without_core_data_the_bios_folder_is_used_as_is() {
        let root = tmp("plain");
        std::fs::write(root.join("core.so"), b"").unwrap();
        let dir = SystemDir::for_core(&root.join("core.so"), Path::new("/nas/bios")).unwrap();
        assert_eq!(dir.path(), Path::new("/nas/bios"));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn merges_bios_files_with_shipped_core_data_and_cleans_up() {
        let root = tmp("merge");
        let cores = root.join("cores");
        let bios = root.join("bios");
        std::fs::create_dir_all(cores.join("system/dolphin-emu/Sys")).unwrap();
        std::fs::create_dir_all(cores.join("system/shared")).unwrap();
        std::fs::create_dir_all(bios.join("shared")).unwrap();
        std::fs::write(bios.join("scph5501.bin"), b"bios").unwrap();
        std::fs::write(bios.join("shared/mine"), b"").unwrap();

        let dir = SystemDir::for_core(&cores.join("dolphin_libretro.so"), &bios).unwrap();
        let merged = dir.path().to_owned();
        assert_eq!(std::fs::read(merged.join("scph5501.bin")).unwrap(), b"bios");
        assert!(merged.join("dolphin-emu/Sys").is_dir(), "shipped core data");
        assert!(merged.join("shared/mine").exists(), "the library's copy wins");
        drop(dir);
        assert!(!merged.exists(), "the merge is removed");
        assert!(bios.join("scph5501.bin").exists(), "the sources are untouched");
        assert!(cores.join("system/dolphin-emu/Sys").exists());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn folders_in_both_are_merged_and_batocera_names_aliased() {
        let root = tmp("alias");
        let cores = root.join("cores");
        let bios = root.join("bios");
        std::fs::create_dir_all(cores.join("system/pcsx2/resources")).unwrap();
        std::fs::create_dir_all(bios.join("ps2")).unwrap();
        std::fs::write(bios.join("ps2/scph39001.bin"), b"ps2").unwrap();
        std::fs::create_dir_all(bios.join("dc")).unwrap();
        std::fs::create_dir_all(cores.join("system/dc/extra")).unwrap();

        let dir = SystemDir::for_core(&cores.join("pcsx2_libretro.so"), &bios).unwrap();
        let m = dir.path();
        assert_eq!(std::fs::read(m.join("pcsx2/bios/scph39001.bin")).unwrap(), b"ps2");
        assert!(m.join("pcsx2/resources").is_dir(), "shipped resources kept");
        assert!(!m.join("pcsx2").symlink_metadata().unwrap().file_type().is_symlink());
        std::fs::create_dir(m.join("pcsx2/memcards")).expect("a core can create folders there");
        assert!(m.join("dc/extra").is_dir(), "a folder in both is merged");
        assert!(!cores.join("system/pcsx2/memcards").exists(), "the sources are untouched");
        drop(dir);
        std::fs::remove_dir_all(root).ok();
    }
}
