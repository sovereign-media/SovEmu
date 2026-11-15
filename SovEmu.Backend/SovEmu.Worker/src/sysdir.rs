//! The system (BIOS) directory a core sees.
//!
//! BIOS files are kept one folder per system (`<bios>/psx/scph5501.bin`,
//! `<bios>/dreamcast/dc_boot.bin`), named like the system's ROM folder, and the server hands the
//! worker that system's folder. Cores look for them elsewhere: Flycast reads `dc/dc_boot.bin`,
//! PCSX2 `pcsx2/bios/`, Dolphin `dolphin-emu/Sys/GC/`, most others the root. `--bios-path`
//! says where; the folder's files appear there.
//!
//! Some cores also need data files of their own: Dolphin reads `<system>/dolphin-emu/Sys`,
//! which SovEmu ships in a `system/` folder beside the cores. When the BIOS files must move or
//! there is core data, the worker gives the core a merged view: a temporary directory of
//! symlinks to the shipped core data, with the BIOS folder's entries linked in at `--bios-path`
//! (the BIOS folder's copies win). A folder both have becomes a real folder of the merged
//! contents, which also lets a core create its working files there (they last the session).
//! Nothing is written to the shipped data.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// A merged system directory, removed when dropped.
#[derive(Debug)]
pub struct SystemDir {
    path: PathBuf,
    /// Set when `path` is a temporary merge this worker made.
    owned: bool,
}

impl SystemDir {
    /// The system directory for `core`: `bios` itself when its files belong at the root and
    /// the core ships no data, or else a merge of the core data shipped beside the core
    /// (`<core's folder>/system`) and `bios`'s files at `at` (a relative path, `""` the root).
    pub fn for_core(core: &Path, bios: &Path, at: &str) -> Result<Self> {
        let at = Path::new(at.trim_matches('/'));
        let shipped = core.parent().map(|d| d.join("system")).filter(|d| d.is_dir());
        if shipped.is_none() && at.as_os_str().is_empty() {
            return Ok(Self { path: bios.to_owned(), owned: false });
        }
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let name = format!("sovemu-system-{}-{n}", std::process::id());
        let path = std::env::temp_dir().join(name);
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).with_context(|| format!("creating {}", path.display()))?;
        let dir = Self { path, owned: true };
        if let Some(shipped) = &shipped {
            merge(&dir.path, &[shipped.as_path()])?;
        }
        let target = dir.path.join(at);
        let mut real = dir.path.clone();
        for part in at.parent().into_iter().flat_map(Path::components) {
            real.push(part);
            make_real_dir(&real)?;
        }
        if at.as_os_str().is_empty() || target.symlink_metadata().is_ok() {
            // Shipped data is already there (Dolphin's `Sys/GC` fonts): add the BIOS files.
            make_real_dir(&target)?;
            overlay(&target, bios)?;
        } else {
            symlink(bios, &target).with_context(|| format!("linking {}", target.display()))?;
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

/// Links `source`'s entries into `dest` (a real folder this worker made), replacing what is
/// there: a folder both have is merged.
fn overlay(dest: &Path, source: &Path) -> Result<()> {
    let Ok(entries) = std::fs::read_dir(source) else { return Ok(()) };
    for entry in entries.flatten() {
        let from = entry.path();
        let link = dest.join(entry.file_name());
        match link.symlink_metadata() {
            Ok(_) if link.is_dir() && from.is_dir() => {
                make_real_dir(&link)?;
                overlay(&link, &from)?;
                continue;
            }
            // Only links and folders of links live here: removing them touches no source.
            Ok(m) if m.is_dir() => std::fs::remove_dir_all(&link)?,
            Ok(_) => std::fs::remove_file(&link)?,
            Err(_) => {}
        }
        symlink(&from, &link).with_context(|| format!("linking {}", link.display()))?;
    }
    Ok(())
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
        let dir =
            SystemDir::for_core(&root.join("core.so"), Path::new("/data/bios/psx"), "").unwrap();
        assert_eq!(dir.path(), Path::new("/data/bios/psx"));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn the_bios_folder_is_linked_where_the_core_looks() {
        let root = tmp("at");
        let bios = root.join("bios/dreamcast");
        std::fs::create_dir_all(&bios).unwrap();
        std::fs::write(bios.join("dc_boot.bin"), b"boot").unwrap();
        std::fs::write(root.join("flycast_libretro.so"), b"").unwrap();

        let dir = SystemDir::for_core(&root.join("flycast_libretro.so"), &bios, "dc").unwrap();
        let merged = dir.path().to_owned();
        assert_eq!(std::fs::read(merged.join("dc/dc_boot.bin")).unwrap(), b"boot");
        drop(dir);
        assert!(!merged.exists(), "the merge is removed");
        assert!(bios.join("dc_boot.bin").exists(), "the source is untouched");
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn merges_bios_files_with_shipped_core_data_and_cleans_up() {
        let root = tmp("merge");
        let cores = root.join("cores");
        let bios = root.join("bios/psx");
        std::fs::create_dir_all(cores.join("system/dolphin-emu/Sys")).unwrap();
        std::fs::create_dir_all(cores.join("system/shared")).unwrap();
        std::fs::write(cores.join("system/shared/theirs"), b"").unwrap();
        std::fs::create_dir_all(bios.join("shared")).unwrap();
        std::fs::write(bios.join("scph5501.bin"), b"bios").unwrap();
        std::fs::write(bios.join("shared/mine"), b"").unwrap();

        let dir = SystemDir::for_core(&cores.join("dolphin_libretro.so"), &bios, "").unwrap();
        let merged = dir.path().to_owned();
        assert_eq!(std::fs::read(merged.join("scph5501.bin")).unwrap(), b"bios");
        assert!(merged.join("dolphin-emu/Sys").is_dir(), "shipped core data");
        assert!(merged.join("shared/mine").exists(), "a folder in both is merged");
        assert!(merged.join("shared/theirs").exists());
        drop(dir);
        assert!(!merged.exists(), "the merge is removed");
        assert!(bios.join("scph5501.bin").exists(), "the sources are untouched");
        assert!(cores.join("system/dolphin-emu/Sys").exists());
        assert!(cores.join("system/shared/theirs").exists());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn bios_files_join_shipped_folders_on_the_way() {
        let root = tmp("nested");
        let cores = root.join("cores");
        std::fs::create_dir_all(cores.join("system/pcsx2/resources")).unwrap();
        std::fs::create_dir_all(cores.join("system/dolphin-emu/Sys/GC")).unwrap();
        std::fs::write(cores.join("system/dolphin-emu/Sys/GC/font_western.bin"), b"font").unwrap();
        let ps2 = root.join("bios/ps2");
        std::fs::create_dir_all(&ps2).unwrap();
        std::fs::write(ps2.join("scph39001.bin"), b"ps2").unwrap();
        let gc = root.join("bios/gamecube");
        std::fs::create_dir_all(gc.join("USA")).unwrap();
        std::fs::write(gc.join("USA/IPL.bin"), b"ipl").unwrap();

        let dir =
            SystemDir::for_core(&cores.join("pcsx2_libretro.so"), &ps2, "pcsx2/bios").unwrap();
        let m = dir.path();
        assert_eq!(std::fs::read(m.join("pcsx2/bios/scph39001.bin")).unwrap(), b"ps2");
        assert!(m.join("pcsx2/resources").is_dir(), "shipped resources kept");
        assert!(!m.join("pcsx2").symlink_metadata().unwrap().file_type().is_symlink());
        std::fs::create_dir(m.join("pcsx2/memcards")).expect("a core can create folders there");
        assert!(!cores.join("system/pcsx2/memcards").exists(), "the sources are untouched");
        drop(dir);

        let dir =
            SystemDir::for_core(&cores.join("dolphin_libretro.so"), &gc, "dolphin-emu/Sys/GC")
                .unwrap();
        let m = dir.path();
        assert_eq!(std::fs::read(m.join("dolphin-emu/Sys/GC/USA/IPL.bin")).unwrap(), b"ipl");
        assert!(m.join("dolphin-emu/Sys/GC/font_western.bin").exists(), "shipped fonts kept");
        assert!(!cores.join("system/dolphin-emu/Sys/GC/USA").exists());
        drop(dir);
        std::fs::remove_dir_all(root).ok();
    }
}
