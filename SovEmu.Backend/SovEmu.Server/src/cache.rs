//! A local copy of a slow (network) library: ROMs and artwork, least recently used first out.
//!
//! On a library reached over a high-latency link (an SMB share through a tunnel), opening a
//! game or showing its art can take seconds. The cache keeps a copy on local disk:
//!
//! - [`Cache::local_or_fetch`] returns the cached copy if it is current, and otherwise returns
//!   the original while copying it in the background, so the first use never waits for the copy
//!   and every later one is local.
//! - A copy is current when its size and modification time match the original's.
//! - Past the size cap, the least recently used files go. Use is recorded as the modification
//!   time of a `.used` sidecar, so the copy's own time keeps mirroring the original's.
//! - [`Cache::prefetch_many`] warms a batch (a system's artwork) on one background thread.
//!
//! Files that name other files (`.cue`, `.m3u`, `.ccd`, `.gdi`) are never cached: a copy without
//! its tracks would not load.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

/// Parallel copies when prefetching: each file is a few round trips on a high-latency share.
const PREFETCH_THREADS: usize = 4;

/// Extensions whose files point at other files.
const MULTI_FILE: &[&str] = &["cue", "m3u", "ccd", "gdi", "toc"];

#[derive(Debug)]
pub struct Cache {
    dir: PathBuf,
    max_bytes: u64,
    /// Copies in flight, by destination, so one file is never copied twice at once.
    copying: Mutex<HashSet<PathBuf>>,
    /// Batches being prefetched, by name, so opening a system twice does not queue it twice.
    batches: Mutex<HashSet<String>>,
}

/// Whether a file at `path` should be cached at all.
pub fn cacheable(path: &Path) -> bool {
    let ext =
        path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    !MULTI_FILE.contains(&ext.as_str())
}

fn used_marker(copy: &Path) -> PathBuf {
    let mut name = copy.file_name().unwrap_or_default().to_os_string();
    name.push(".used");
    copy.with_file_name(name)
}

/// Whether `copy` matches `original`: same size, and modification times within two seconds.
/// Not exact: filesystems keep times at different precisions (an SMB share reports 100 ns, a
/// macOS bind mount into Docker less), so a faithful copy can differ in the last digits.
fn current(copy: &Path, original: &Path) -> bool {
    let (Ok(c), Ok(o)) = (std::fs::metadata(copy), std::fs::metadata(original)) else {
        return false;
    };
    let (Ok(cm), Ok(om)) = (c.modified(), o.modified()) else { return false };
    let apart = cm.duration_since(om).or_else(|_| om.duration_since(cm)).unwrap_or_default();
    c.len() == o.len() && apart < std::time::Duration::from_secs(2)
}

impl Cache {
    /// `max_bytes` of 0 disables caching.
    pub fn new(dir: PathBuf, max_bytes: u64) -> Arc<Self> {
        Arc::new(Self {
            dir,
            max_bytes,
            copying: Mutex::new(HashSet::new()),
            batches: Mutex::new(HashSet::new()),
        })
    }

    pub fn enabled(&self) -> bool {
        self.max_bytes > 0
    }

    /// Where `relative` (a path under the library root, `/`-separated) is cached, under a
    /// namespace (`roms`, `media`). `None` for paths that try to leave the cache.
    fn path_for(&self, namespace: &str, relative: &str) -> Option<PathBuf> {
        let rel = Path::new(relative);
        if rel.components().any(|c| !matches!(c, std::path::Component::Normal(_))) {
            return None;
        }
        Some(self.dir.join(namespace).join(rel))
    }

    /// The cached copy, if there is a current one (and marks it used).
    pub fn cached(&self, namespace: &str, relative: &str, original: &Path) -> Option<PathBuf> {
        if !self.enabled() || !cacheable(original) {
            return None;
        }
        let copy = self.path_for(namespace, relative)?;
        current(&copy, original).then(|| {
            let _ = std::fs::write(used_marker(&copy), b"");
            copy
        })
    }

    /// The cached copy if current; otherwise `original`, with a copy started in the background.
    pub fn local_or_fetch(
        self: &Arc<Self>,
        namespace: &str,
        relative: &str,
        original: &Path,
    ) -> PathBuf {
        if let Some(copy) = self.cached(namespace, relative, original) {
            return copy;
        }
        if self.enabled()
            && cacheable(original)
            && let Some(copy) = self.path_for(namespace, relative)
        {
            self.spawn_copy(original.to_owned(), copy);
        }
        original.to_owned()
    }

    /// Copies a batch of `(namespace, relative, original)` in the background, [`PREFETCH_THREADS`]
    /// at a time, skipping what is already cached. `name` identifies the batch.
    pub fn prefetch_many(
        self: &Arc<Self>,
        name: String,
        items: Vec<(&'static str, String, PathBuf)>,
    ) {
        if !self.enabled()
            || !self.batches.lock().unwrap_or_else(|e| e.into_inner()).insert(name.clone())
        {
            return;
        }
        let cache = self.clone();
        std::thread::spawn(move || {
            let queue = Mutex::new(items.into_iter());
            let copied = std::sync::atomic::AtomicUsize::new(0);
            std::thread::scope(|scope| {
                for _ in 0..PREFETCH_THREADS {
                    scope.spawn(|| {
                        loop {
                            let next = queue.lock().unwrap_or_else(|e| e.into_inner()).next();
                            let Some((namespace, relative, original)) = next else { break };
                            let Some(copy) = cache.path_for(namespace, &relative) else { continue };
                            if !cacheable(&original) || current(&copy, &original) {
                                continue;
                            }
                            if cache
                                .copying
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .insert(copy.clone())
                            {
                                if cache.copy(&original, &copy).is_ok() {
                                    copied.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                                }
                                cache
                                    .copying
                                    .lock()
                                    .unwrap_or_else(|e| e.into_inner())
                                    .remove(&copy);
                            }
                        }
                    });
                }
            });
            tracing::info!("prefetched {} file(s) for {name}", copied.into_inner());
            cache.batches.lock().unwrap_or_else(|e| e.into_inner()).remove(&name);
        });
    }

    fn spawn_copy(self: &Arc<Self>, original: PathBuf, copy: PathBuf) {
        if !self.copying.lock().unwrap_or_else(|e| e.into_inner()).insert(copy.clone()) {
            return;
        }
        let cache = self.clone();
        std::thread::spawn(move || {
            if let Err(e) = cache.copy(&original, &copy) {
                tracing::warn!("caching {} failed: {e}", original.display());
            }
            cache.copying.lock().unwrap_or_else(|e| e.into_inner()).remove(&copy);
        });
    }

    /// Copies through a temporary file, gives the copy the original's modification time, then
    /// evicts down to the cap.
    pub fn copy(&self, original: &Path, copy: &Path) -> std::io::Result<()> {
        let size = std::fs::metadata(original)?.len();
        if size > self.max_bytes {
            return Ok(());
        }
        if let Some(dir) = copy.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let part = copy.with_extension("part");
        std::fs::copy(original, &part)?;
        let modified = std::fs::metadata(original)?.modified()?;
        std::fs::File::options().write(true).open(&part)?.set_modified(modified)?;
        std::fs::rename(&part, copy)?;
        std::fs::write(used_marker(copy), b"")?;
        self.evict(Some(copy));
        Ok(())
    }

    /// Deletes least recently used copies until the cache fits its cap. `keep` is never evicted.
    pub fn evict(&self, keep: Option<&Path>) {
        let mut files = Vec::new();
        let mut stack = vec![self.dir.clone()];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else { continue };
            for entry in entries.flatten() {
                let path = entry.path();
                let Ok(meta) = entry.metadata() else { continue };
                if meta.is_dir() {
                    stack.push(path);
                } else if !path.to_string_lossy().ends_with(".used")
                    && path.extension().is_none_or(|e| e != "part")
                {
                    let used = std::fs::metadata(used_marker(&path))
                        .and_then(|m| m.modified())
                        .unwrap_or(SystemTime::UNIX_EPOCH);
                    files.push((used, meta.len(), path));
                }
            }
        }
        let mut total: u64 = files.iter().map(|(_, len, _)| len).sum();
        files.sort_by_key(|(used, _, _)| *used);
        for (_, len, path) in files {
            if total <= self.max_bytes {
                break;
            }
            if Some(path.as_path()) == keep {
                continue;
            }
            let _ = std::fs::remove_file(&path);
            let _ = std::fs::remove_file(used_marker(&path));
            total -= len;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn tmp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sovemu-cache-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn wait_for(path: &Path) {
        for _ in 0..200 {
            if path.exists() {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("{} never appeared", path.display());
    }

    #[test]
    fn first_use_reads_the_original_and_later_ones_the_copy() {
        let lib = tmp("lib");
        let cache = Cache::new(tmp("store"), 1 << 20);
        let rom = lib.join("nes/Game.nes");
        std::fs::create_dir_all(rom.parent().unwrap()).unwrap();
        std::fs::write(&rom, [7u8; 100]).unwrap();

        assert_eq!(
            cache.local_or_fetch("roms", "nes/Game.nes", &rom),
            rom,
            "first use: the original"
        );
        let copy = cache.dir.join("roms/nes/Game.nes");
        wait_for(&copy);
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(cache.local_or_fetch("roms", "nes/Game.nes", &rom), copy, "then the copy");
        assert_eq!(std::fs::read(&copy).unwrap(), vec![7u8; 100]);

        // The original changes: the copy is stale, so the original is used again.
        std::fs::write(&rom, [8u8; 120]).unwrap();
        assert_eq!(cache.local_or_fetch("roms", "nes/Game.nes", &rom), rom);
    }

    #[test]
    fn evicts_the_least_recently_used_past_the_cap() {
        let lib = tmp("lib-evict");
        let cache = Cache::new(tmp("store-evict"), 250);
        for name in ["a", "b", "c"] {
            let src = lib.join(format!("{name}.gba"));
            std::fs::write(&src, [0u8; 100]).unwrap();
            cache.copy(&src, &cache.dir.join(format!("roms/{name}.gba"))).unwrap();
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!cache.dir.join("roms/a.gba").exists(), "the oldest went");
        assert!(cache.dir.join("roms/b.gba").exists());
        assert!(cache.dir.join("roms/c.gba").exists());
    }

    #[test]
    fn skips_multi_file_formats_traversal_and_a_disabled_cache() {
        assert!(!cacheable(Path::new("psx/Game.cue")));
        assert!(cacheable(Path::new("psx/Game.chd")));
        let cache = Cache::new(tmp("store-skip"), 1 << 20);
        assert!(cache.path_for("roms", "../../etc/passwd").is_none());
        let off = Cache::new(tmp("store-off"), 0);
        let rom = tmp("lib-off").join("x.nes");
        std::fs::write(&rom, [1u8]).unwrap();
        assert_eq!(off.local_or_fetch("roms", "x.nes", &rom), rom);
        std::thread::sleep(Duration::from_millis(50));
        assert!(!off.dir.join("roms/x.nes").exists());
    }
}
