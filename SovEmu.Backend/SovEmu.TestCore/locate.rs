//! Finds the test core for a test that loads it. Not part of the core: the RetroHost and Worker
//! tests include it with `#[path]`.
//!
//! nextest runs every test as a process of its own, all at once, and each process asks for the
//! core. Three things keep them from tripping over one another:
//!
//! - The build asks for the features the workspace's build unifies to (`sovemu-libretro-ffi`
//!   with `frontend`, which RetroHost turns on). Built alone without them, the core is a
//!   different build written to the same file, so the workspace's build and this one rebuilt
//!   it in turn, under a worker that had it open.
//! - One process at a time builds, behind a file lock in the target directory: even a build
//!   with nothing to do relinks the library's file.
//! - Each test loads a copy named for the build, which nothing rewrites, never the file cargo
//!   relinks.
//!
//! `SOVEMU_TEST_CORE` names a core to use instead, as is.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The test core's shared library, built if it needs to be (once per process).
pub(crate) fn test_core() -> &'static Path {
    static PATH: OnceLock<PathBuf> = OnceLock::new();
    PATH.get_or_init(locate)
}

fn locate() -> PathBuf {
    if let Some(path) = std::env::var_os("SOVEMU_TEST_CORE") {
        return PathBuf::from(path);
    }
    // Every crate that includes this sits one level under the workspace.
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map_or_else(|| workspace.join("target"), PathBuf::from);
    std::fs::create_dir_all(&target).expect("create the target directory");
    let lock = File::create(target.join("sovemu-test-core.lock")).expect("open the build lock");
    lock.lock().expect("take the build lock");

    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let status = Command::new(cargo)
        .args(["build", "--quiet", "-p", "sovemu-test-core"])
        .args(["--features", "sovemu-libretro-ffi/frontend"])
        .current_dir(&workspace)
        .status()
        .expect("failed to run cargo to build sovemu-test-core");
    assert!(status.success(), "building sovemu-test-core failed");

    let (prefix, suffix) = (std::env::consts::DLL_PREFIX, std::env::consts::DLL_SUFFIX);
    let dir = target.join("debug");
    let built = dir.join(format!("{prefix}sovemu_test_core{suffix}"));
    let meta = std::fs::metadata(&built).expect("the built test core");
    let stamp = meta
        .modified()
        .expect("the test core's build time")
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let copies = dir.join("test-core");
    let copy = copies.join(format!("{prefix}sovemu_test_core-{stamp}-{}{suffix}", meta.len()));
    if !copy.exists() {
        std::fs::create_dir_all(&copies).expect("create the test core's copies directory");
        prune(&copies);
        let partial = copies.join(format!("partial-{}", std::process::id()));
        std::fs::copy(&built, &partial).expect("copy the test core");
        std::fs::rename(&partial, &copy).expect("name the test core's copy");
    }
    copy
}

/// Removes copies of builds more than a day old. Never newer ones: a test that already has a
/// copy's path may not have started its worker yet, if the core was rebuilt during the run.
fn prune(copies: &Path) {
    let Ok(entries) = std::fs::read_dir(copies) else { return };
    let day_ago = SystemTime::now() - Duration::from_secs(24 * 60 * 60);
    for entry in entries.flatten() {
        let old = entry.metadata().and_then(|m| m.modified()).is_ok_and(|t| t < day_ago);
        if old {
            // Best effort: a copy that cannot go now goes on a later run.
            let _ = std::fs::remove_file(entry.path());
        }
    }
}
