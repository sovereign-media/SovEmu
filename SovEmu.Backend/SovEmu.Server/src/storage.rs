//! Where the data folder's space goes, for the admin Storage page: the database, each user's
//! saves, the game cache, the mirror, artwork and the rest, with the filesystem's free space.
//!
//! One walk of the data folder (a mirror can be tens of thousands of files) credits each file to
//! the most specific area it is in. It runs on a blocking thread, and its result is kept for a
//! few minutes: the page asks again only when an admin refreshes it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::AppState;
use crate::error::{ApiError, ApiResult};

/// How long a measurement is shown before the page measures again by itself.
const FRESH_FOR: Duration = Duration::from_secs(300);

/// A part of the data folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Area {
    /// `database`, `saves`, `cache`, … (the page names them).
    pub(crate) id: &'static str,
    pub(crate) path: PathBuf,
    pub(crate) bytes: u64,
    pub(crate) files: u64,
}

/// One user's saves (a folder named by user id; a deleted user's are left behind).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UserSaves {
    pub(crate) user_id: i64,
    /// None for a user that no longer exists.
    pub(crate) username: Option<String>,
    pub(crate) bytes: u64,
    pub(crate) files: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct StorageReport {
    /// When it was measured (Unix seconds).
    pub(crate) measured_at: f64,
    pub(crate) data_dir: PathBuf,
    /// The areas, largest first; `other` is everything in the data folder no area claims.
    pub(crate) areas: Vec<Area>,
    pub(crate) saves: Vec<UserSaves>,
    /// The filesystem the data folder is on.
    pub(crate) total_bytes: Option<u64>,
    pub(crate) free_bytes: Option<u64>,
}

/// The last measurement, kept between requests.
#[derive(Debug, Default)]
pub(crate) struct StorageCache(Mutex<Option<(Instant, StorageReport)>>);

/// The areas to measure, those inside the data folder only (`roms/` and `bios/` may be
/// elsewhere, on a share).
fn areas(state: &AppState) -> Vec<(&'static str, PathBuf)> {
    let config = &state.config;
    let paths = config.paths();
    let mut areas = vec![
        ("database", paths.database()),
        ("saves", paths.all_saves()),
        ("cache", paths.cache_games()),
        ("discs", paths.disc_playlists()),
        ("mirror", paths.mirror()),
        ("artwork", paths.artwork()),
        ("launchbox", paths.launchbox()),
        ("metadata", paths.metadata()),
        ("decorations", paths.decorations()),
        ("builtin_decorations", config.builtin_decorations()),
        ("roms", config.roms_dir.clone()),
        ("bios", config.bios_dir()),
        ("backups", config.backup_dir()),
    ];
    // In mirror mode the library *is* the mirror: count it once.
    areas.retain(|(id, p)| p.starts_with(paths.root()) && !(*id == "roms" && *p == paths.mirror()));
    areas
}

/// Which area a file is in: the longest path that contains it. The database's write-ahead log
/// and shared memory (`sovemu.db-wal`, `-shm`) count as the database.
fn area_of(file: &Path, areas: &[(&'static str, PathBuf)]) -> Option<&'static str> {
    let wal_of = |db: &Path| {
        let name = db.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        file.parent() == db.parent()
            && file.file_name().is_some_and(|f| f.to_string_lossy().starts_with(&name))
    };
    areas
        .iter()
        .filter(|(id, p)| file.starts_with(p) || (*id == "database" && wal_of(p)))
        .max_by_key(|(_, p)| p.components().count())
        .map(|(id, _)| *id)
}

/// Bytes and files.
type Tally = (u64, u64);

/// Walks `root` once, crediting each file to its area, and each save to its user's folder.
fn measure(
    root: &Path,
    areas: &[(&'static str, PathBuf)],
    saves_dir: &Path,
) -> (BTreeMap<&'static str, Tally>, BTreeMap<i64, Tally>) {
    let mut by_area: BTreeMap<&'static str, Tally> = BTreeMap::new();
    let mut by_user: BTreeMap<i64, Tally> = BTreeMap::new();
    let mut stack = vec![root.to_owned()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            // Not followed: a link out of the data folder is not its space.
            let Ok(kind) = entry.file_type() else { continue };
            let path = entry.path();
            if kind.is_dir() {
                stack.push(path);
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            let area = area_of(&path, areas).unwrap_or("other");
            let slot = by_area.entry(area).or_default();
            slot.0 += meta.len();
            slot.1 += 1;
            if area == "saves"
                && let Some(user) = path
                    .strip_prefix(saves_dir)
                    .ok()
                    .and_then(|r| r.components().next())
                    .and_then(|c| c.as_os_str().to_str()?.parse::<i64>().ok())
            {
                let slot = by_user.entry(user).or_default();
                slot.0 += meta.len();
                slot.1 += 1;
            }
        }
    }
    (by_area, by_user)
}

/// One user's saves on disk, every system's: bytes and files. Walks only that folder, so a page
/// about one user need not measure the whole data folder.
pub(crate) async fn user_saves(state: &AppState, user_id: i64) -> ApiResult<(u64, u64)> {
    let dir = state.config.paths().user_saves(user_id);
    tokio::task::spawn_blocking(move || {
        let mut tally = (0, 0);
        let mut stack = vec![dir];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else { continue };
            for entry in entries.flatten() {
                let Ok(kind) = entry.file_type() else { continue };
                if kind.is_dir() {
                    stack.push(entry.path());
                } else if let Ok(meta) = entry.metadata() {
                    tally = (tally.0 + meta.len(), tally.1 + 1);
                }
            }
        }
        tally
    })
    .await
    .map_err(|e| ApiError::internal(format!("measuring saves: {e}")))
}

/// The data folder's filesystem: total and free (to unprivileged users) bytes.
fn filesystem(path: &Path) -> (Option<u64>, Option<u64>) {
    match rustix::fs::statvfs(path) {
        Ok(s) => (
            Some(s.f_blocks.saturating_mul(s.f_frsize)),
            Some(s.f_bavail.saturating_mul(s.f_frsize)),
        ),
        Err(_) => (None, None),
    }
}

/// The data folder's use: measured now when `refresh` is asked or the last one is stale,
/// else the last measurement.
pub(crate) async fn report(state: &AppState, refresh: bool) -> ApiResult<StorageReport> {
    if !refresh
        && let Some((at, report)) = &*state.storage.0.lock().unwrap_or_else(|e| e.into_inner())
        && at.elapsed() < FRESH_FOR
    {
        return Ok(report.clone());
    }
    let paths = state.config.paths();
    let root = paths.root().to_owned();
    let areas = areas(state);
    let saves_dir = paths.all_saves();
    let (by_area, by_user) = {
        let (root, areas) = (root.clone(), areas.clone());
        tokio::task::spawn_blocking(move || measure(&root, &areas, &saves_dir))
            .await
            .map_err(|e| ApiError::internal(format!("measuring the data folder: {e}")))?
    };
    let names = crate::api::usernames(state).await?;
    let mut out: Vec<Area> = areas
        .iter()
        .map(|(id, path)| {
            let (bytes, files) = by_area.get(id).copied().unwrap_or_default();
            Area { id, path: path.clone(), bytes, files }
        })
        .collect();
    let (bytes, files) = by_area.get("other").copied().unwrap_or_default();
    out.push(Area { id: "other", path: root.clone(), bytes, files });
    out.sort_by_key(|a| std::cmp::Reverse(a.bytes));
    let mut saves: Vec<UserSaves> = by_user
        .into_iter()
        .map(|(user_id, (bytes, files))| UserSaves {
            user_id,
            username: names.get(&user_id).cloned(),
            bytes,
            files,
        })
        .collect();
    saves.sort_by_key(|u| std::cmp::Reverse(u.bytes));
    let (total_bytes, free_bytes) = {
        let root = root.clone();
        tokio::task::spawn_blocking(move || filesystem(&root)).await.unwrap_or((None, None))
    };
    let report = StorageReport {
        measured_at: SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64()),
        data_dir: root,
        areas: out,
        saves,
        total_bytes,
        free_bytes,
    };
    *state.storage.0.lock().unwrap_or_else(|e| e.into_inner()) =
        Some((Instant::now(), report.clone()));
    Ok(report)
}

/// Deletes the game cache's copies, but those being played or copied; the bytes freed. The
/// next measurement is taken afresh.
pub(crate) async fn clear_cache(state: &AppState) -> ApiResult<u64> {
    let playing: Vec<i64> = state.workers.all().iter().map(|s| s.info().game_id).collect();
    let mut keep = Vec::new();
    for id in playing {
        let path: Option<String> = sqlx::query_scalar("SELECT path FROM games WHERE id = ?")
            .bind(id)
            .fetch_optional(&state.db)
            .await?;
        keep.extend(path);
    }
    let cache = state.cache.clone();
    let freed = tokio::task::spawn_blocking(move || {
        let keep: Vec<(&str, &str)> = keep.iter().map(|p| ("roms", p.as_str())).collect();
        cache.clear(&keep)
    })
    .await
    .map_err(|e| ApiError::internal(format!("clearing the cache: {e}")))?;
    *state.storage.0.lock().unwrap_or_else(|e| e.into_inner()) = None;
    tracing::info!(bytes = freed, "cache cleared");
    Ok(freed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_belong_to_the_most_specific_area() {
        let areas = vec![
            ("database", PathBuf::from("/data/sovemu.db")),
            ("metadata", PathBuf::from("/data/metadata")),
            ("artwork", PathBuf::from("/data/metadata/media")),
            ("saves", PathBuf::from("/data/saves")),
        ];
        let of = |p: &str| area_of(Path::new(p), &areas);
        assert_eq!(of("/data/metadata/media/nes/x.png"), Some("artwork"));
        assert_eq!(of("/data/metadata/launchbox/Metadata.xml"), Some("metadata"));
        assert_eq!(of("/data/sovemu.db-wal"), Some("database"));
        assert_eq!(of("/data/sovemu.db"), Some("database"));
        assert_eq!(of("/data/saves/3/snes/game.srm"), Some("saves"));
        assert_eq!(of("/data/server.json"), None);
    }

    #[test]
    fn a_walk_counts_areas_and_each_users_saves() {
        let root = std::env::temp_dir().join(format!("sovemu-storage-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let write = |rel: &str, len: usize| {
            let p = root.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, vec![0u8; len]).unwrap();
        };
        write("sovemu.db", 100);
        write("sovemu.db-wal", 10);
        write("saves/1/nes/a.srm", 8);
        write("saves/1/nes/states/a/slot1.state", 32);
        write("saves/2/gb/b.srm", 8);
        write("server.json", 5);
        let areas = vec![("database", root.join("sovemu.db")), ("saves", root.join("saves"))];
        let (by_area, by_user) = measure(&root, &areas, &root.join("saves"));
        assert_eq!(by_area["database"], (110, 2));
        assert_eq!(by_area["saves"], (48, 3));
        assert_eq!(by_area["other"], (5, 1));
        assert_eq!(by_user[&1], (40, 2));
        assert_eq!(by_user[&2], (8, 1));
        let (total, free) = filesystem(&root);
        assert!(total.unwrap() >= free.unwrap());
        std::fs::remove_dir_all(root).ok();
    }
}
