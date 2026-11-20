//! `mirror` library mode: keep a full local copy of a network library and play from it.
//!
//! A sync walks the source's system folders (optionally only some systems), copies every file
//! that is new or changed (ROMs and their `images/` / `videos/` artwork alike), and deletes local
//! files the source no longer has. Copies go through a temporary file and keep the source's
//! modification time, so the next sync can tell them apart from changed originals. Several files
//! copy at once: on a network share each file costs round trips.

use std::collections::HashSet;
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use serde::Serialize;

use crate::systems;

/// Progress of the current (or last) sync, for `GET /api/library/status`.
#[derive(Debug, Clone, Default, Serialize)]
pub struct SyncStatus {
    pub running: bool,
    pub files_total: u64,
    pub files_done: u64,
    pub bytes_total: u64,
    pub bytes_done: u64,
    pub removed: u64,
    pub failed: u64,
    /// Unix seconds when the last sync finished.
    pub finished_at: Option<u64>,
    pub error: Option<String>,
}

/// Same size, and modification times within two seconds (filesystems keep different precision).
fn same(a: &std::fs::Metadata, b: &std::fs::Metadata) -> bool {
    let (Ok(am), Ok(bm)) = (a.modified(), b.modified()) else { return false };
    let apart = am.duration_since(bm).or_else(|_| bm.duration_since(am)).unwrap_or_default();
    a.len() == b.len() && apart < Duration::from_secs(2)
}

/// Every file under `dir` (relative paths, `/`-separated), skipping hidden entries.
fn list(dir: &Path, prefix: &str, out: &mut Vec<(String, u64)>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || name.ends_with(".part") {
            continue;
        }
        let rel = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_dir() || (kind.is_symlink() && entry.path().is_dir()) {
            list(&entry.path(), &rel, out);
        } else if let Ok(meta) = entry.metadata() {
            out.push((rel, meta.len()));
        }
    }
}

/// Which top-level folders of `source` to mirror: recognised system folders, filtered to
/// `systems` when given (by SovEmu system id, e.g. `nes`).
fn folders(source: &Path, only: &[String]) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(source) else { return Vec::new() };
    let mut out: Vec<String> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let system = systems::by_folder(&name)?;
            (only.is_empty() || only.iter().any(|s| s == system.id)).then_some(name)
        })
        .collect();
    out.sort();
    out
}

fn copy_file(src: &Path, dest: &Path) -> std::io::Result<()> {
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let part = dest.with_extension(format!(
        "{}.part",
        dest.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default()
    ));
    std::fs::copy(src, &part)?;
    let modified = std::fs::metadata(src)?.modified()?;
    std::fs::File::options().write(true).open(&part)?.set_modified(modified)?;
    std::fs::rename(&part, dest)
}

/// Brings `dest` in line with `source`. Blocking; run it on a background thread.
pub fn sync(
    source: &Path,
    dest: &Path,
    only: &[String],
    threads: usize,
    status: &Mutex<SyncStatus>,
) {
    let set =
        |f: &dyn Fn(&mut SyncStatus)| f(&mut status.lock().unwrap_or_else(|e| e.into_inner()));
    set(&|s| *s = SyncStatus { running: true, ..SyncStatus::default() });

    if !source.is_dir() {
        set(&|s| {
            s.running = false;
            s.error = Some(format!("{} is not reachable", source.display()));
        });
        return;
    }
    if let Err(e) = std::fs::create_dir_all(dest) {
        set(&|s| {
            s.running = false;
            s.error = Some(format!("cannot create {}: {e}", dest.display()));
        });
        return;
    }

    // What the source has, and what of it needs copying.
    let mut wanted = Vec::new();
    for folder in folders(source, only) {
        list(&source.join(&folder), &folder, &mut wanted);
    }
    let todo: Vec<(String, u64)> = wanted
        .iter()
        .filter(|(rel, _)| {
            let (Ok(s), Ok(d)) =
                (std::fs::metadata(source.join(rel)), std::fs::metadata(dest.join(rel)))
            else {
                return true;
            };
            !same(&s, &d)
        })
        .cloned()
        .collect();
    let total_bytes: u64 = todo.iter().map(|(_, n)| n).sum();
    set(&|s| {
        s.files_total = todo.len() as u64;
        s.bytes_total = total_bytes;
    });
    tracing::info!(
        "mirror: {} of {} files to copy ({} MB)",
        todo.len(),
        wanted.len(),
        total_bytes >> 20
    );

    // Copy, several at a time.
    let queue = Mutex::new(todo.into_iter());
    let (done, bytes, failed) = (AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0));
    std::thread::scope(|scope| {
        for _ in 0..threads.max(1) {
            scope.spawn(|| {
                loop {
                    let next = queue.lock().unwrap_or_else(|e| e.into_inner()).next();
                    let Some((rel, len)) = next else { break };
                    match copy_file(&source.join(&rel), &dest.join(&rel)) {
                        Ok(()) => {
                            bytes.fetch_add(len, Ordering::Relaxed);
                        }
                        Err(e) => {
                            tracing::warn!("mirror: copying {rel} failed: {e}");
                            failed.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                    let b = bytes.load(Ordering::Relaxed);
                    let f = failed.load(Ordering::Relaxed);
                    set(&|s| {
                        s.files_done = n;
                        s.bytes_done = b;
                        s.failed = f;
                    });
                }
            });
        }
    });

    // Remove what the source no longer has (only inside the mirrored folders).
    let keep: HashSet<&str> = wanted.iter().map(|(rel, _)| rel.as_str()).collect();
    let mut present = Vec::new();
    for folder in folders(dest, only) {
        list(&dest.join(&folder), &folder, &mut present);
    }
    let mut removed = 0;
    for (rel, _) in present {
        if !keep.contains(rel.as_str()) && std::fs::remove_file(dest.join(&rel)).is_ok() {
            removed += 1;
        }
    }

    let now = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    set(&|s| {
        s.running = false;
        s.removed = removed;
        s.finished_at = Some(now);
    });
    tracing::info!(
        "mirror: done ({} copied, {} failed, {removed} removed)",
        done.load(Ordering::Relaxed),
        failed.load(Ordering::Relaxed)
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn tmp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sovemu-mirror-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn put(root: &Path, rel: &str, bytes: &[u8]) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
    }

    #[test]
    fn copies_new_and_changed_files_and_removes_deleted_ones() {
        let (src, dst) = (tmp("src"), tmp("dst"));
        put(&src, "nes/A.nes", b"aaaa");
        put(&src, "nes/images/A-thumb.png", b"png");
        put(&src, "snes/B.sfc", b"bbbb");
        put(&src, "notes/readme.txt", b"not a system folder");
        let status = Mutex::new(SyncStatus::default());

        sync(&src, &dst, &[], 4, &status);
        let s = status.lock().unwrap().clone();
        assert_eq!((s.running, s.files_total, s.files_done, s.failed), (false, 3, 3, 0));
        assert_eq!(std::fs::read(dst.join("nes/images/A-thumb.png")).unwrap(), b"png");
        assert!(!dst.join("notes").exists(), "only system folders are mirrored");

        // Nothing changed: nothing to copy.
        sync(&src, &dst, &[], 4, &status);
        assert_eq!(status.lock().unwrap().files_total, 0);

        // A change, an addition and a deletion.
        put(&src, "nes/A.nes", b"changed!");
        put(&src, "nes/C.nes", b"c");
        std::fs::remove_file(src.join("snes/B.sfc")).unwrap();
        sync(&src, &dst, &[], 4, &status);
        let s = status.lock().unwrap().clone();
        assert_eq!((s.files_total, s.removed), (2, 1));
        assert_eq!(std::fs::read(dst.join("nes/A.nes")).unwrap(), b"changed!");
        assert!(!dst.join("snes/B.sfc").exists());
    }

    #[test]
    fn mirrors_only_the_chosen_systems() {
        let (src, dst) = (tmp("src-only"), tmp("dst-only"));
        put(&src, "nes/A.nes", b"a");
        put(&src, "genesis/B.md", b"b");
        let status = Mutex::new(SyncStatus::default());
        sync(&src, &dst, &["megadrive".to_owned()], 4, &status);
        assert!(dst.join("genesis/B.md").exists(), "folder aliases resolve to system ids");
        assert!(!dst.join("nes").exists());
    }

    #[test]
    fn reports_an_unreachable_source() {
        let status = Mutex::new(SyncStatus::default());
        sync(Path::new("/nonexistent/share"), &tmp("dst-missing"), &[], 4, &status);
        let s = status.lock().unwrap().clone();
        assert!(!s.running);
        assert!(s.error.unwrap().contains("not reachable"));
    }
}
