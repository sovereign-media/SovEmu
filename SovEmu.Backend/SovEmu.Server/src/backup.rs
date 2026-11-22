//! Backups of what the server cannot make again, and restoring one.
//!
//! A backup is one zip file in the backup folder (`SOVEMU_BACKUP_DIR`, else `<data>/backups`):
//! - `sovemu.db`: the database (accounts, sign-ins, settings, mappings, collections, history),
//!   copied consistently while the server runs (`VACUUM INTO`);
//! - `server.json`: the generated signing keys, if the server keeps them;
//! - `saves/`: every user's battery saves and states;
//! - `backup.json`: what made it, and when.
//!
//! The library, the cache, the mirror and the game information are made again by a scan and a
//! lookup, so they are left out. A backup holds secrets (password hashes, signing keys): it is
//! handed out only to an admin, through a signed URL.
//!
//! A backup is made by the `Backup` job (every `SOVEMU_BACKUP_INTERVAL_HOURS`, or an admin's
//! button), written beside its final name and renamed into place, so a half-written one is never
//! listed; the newest `SOVEMU_BACKUP_KEEP` are kept.
//!
//! Restoring a live database is not safe, so a backup is restored only at startup, before the
//! database opens (`SOVEMU_RESTORE_FROM`). What was there is moved aside into
//! `<data>/before-restore-<time>/`, never deleted, and the restore is remembered
//! (`<data>/restored.json`) so that a restart with the setting still in place restores nothing.

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::paths::DataPaths;

/// The format `backup.json` describes; a restore refuses a newer one.
const FORMAT: u32 = 1;
const MANIFEST: &str = "backup.json";
const DATABASE: &str = "sovemu.db";
const KEYS: &str = "server.json";
const SAVES: &str = "saves/";
const PREFIX: &str = "sovemu-backup-";

/// What `backup.json` says.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct Manifest {
    format: u32,
    /// The server's version that made it.
    version: String,
    /// When (Unix seconds).
    created: u64,
}

/// A backup in the folder.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BackupFile {
    pub(crate) name: String,
    pub(crate) bytes: u64,
    /// When it was made (Unix seconds), from its file.
    pub(crate) created: f64,
}

/// What [`create`] did.
#[derive(Debug)]
pub(crate) struct Made {
    pub(crate) file: BackupFile,
    /// Older backups deleted to keep `SOVEMU_BACKUP_KEEP`.
    pub(crate) pruned: usize,
}

impl crate::jobs::Summary for Made {
    fn summary(&self) -> Option<String> {
        let mut s = format!("{} ({})", self.file.name, size_text(self.file.bytes));
        if self.pruned > 0 {
            s.push_str(&format!(", {} older deleted", self.pruned));
        }
        Some(s)
    }
}

fn size_text(bytes: u64) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    if bytes as f64 >= MB {
        format!("{:.1} MB", bytes as f64 / MB)
    } else {
        format!("{} KB", bytes.div_ceil(1024))
    }
}

/// Makes a backup in `dir`, then deletes all but the newest `keep` (0: keeps every one).
pub(crate) async fn create(
    db: &SqlitePool,
    paths: &DataPaths,
    dir: &Path,
    keep: u32,
) -> Result<Made> {
    fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let now = SystemTime::now();
    let name = format!("{PREFIX}{}.zip", stamp(now));
    // The database first, consistently while the server writes, beside the backup's final name.
    let copy = dir.join(format!(".{name}.db"));
    let _ = fs::remove_file(&copy);
    sqlx::query("VACUUM INTO ?")
        .bind(copy.to_string_lossy().into_owned())
        .execute(db)
        .await
        .context("copying the database")?;
    let manifest = Manifest {
        format: FORMAT,
        version: env!("CARGO_PKG_VERSION").to_owned(),
        created: now.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()),
    };
    let (paths, dir_owned, name_owned, copy_owned) =
        (paths.clone(), dir.to_owned(), name.clone(), copy.clone());
    let written = tokio::task::spawn_blocking(move || {
        let r = write_zip(&dir_owned, &name_owned, &copy_owned, &paths, &manifest);
        let _ = fs::remove_file(&copy_owned);
        r
    })
    .await
    .context("the backup's writer stopped")??;
    let pruned = prune(dir, keep)?;
    Ok(Made { file: written, pruned })
}

/// Writes the zip beside its name, then renames it into place.
fn write_zip(
    dir: &Path,
    name: &str,
    database: &Path,
    paths: &DataPaths,
    manifest: &Manifest,
) -> Result<BackupFile> {
    let partial = dir.join(format!(".{name}.partial"));
    let mut zip = zip::ZipWriter::new(io::BufWriter::new(File::create(&partial)?));
    let options = |bytes: u64| {
        zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .large_file(bytes >= u64::from(u32::MAX))
    };
    zip.start_file(MANIFEST, options(0))?;
    zip.write_all(&serde_json::to_vec_pretty(manifest)?)?;
    add_file(&mut zip, DATABASE, database, options)?;
    let keys = paths.root().join(KEYS);
    if keys.is_file() {
        add_file(&mut zip, KEYS, &keys, options)?;
    }
    let saves = paths.all_saves();
    if saves.is_dir() {
        for file in files_under(&saves)? {
            let relative = file.strip_prefix(&saves)?.to_string_lossy().replace('\\', "/");
            add_file(&mut zip, &format!("{SAVES}{relative}"), &file, options)?;
        }
    }
    zip.finish()?.into_inner().map_err(|e| e.into_error())?.sync_all()?;
    let path = dir.join(name);
    fs::rename(&partial, &path)?;
    describe(&path).with_context(|| format!("reading {}", path.display()))
}

fn add_file<W: Write + io::Seek>(
    zip: &mut zip::ZipWriter<W>,
    name: &str,
    file: &Path,
    options: impl Fn(u64) -> zip::write::SimpleFileOptions,
) -> Result<()> {
    let mut source = File::open(file).with_context(|| format!("reading {}", file.display()))?;
    zip.start_file(name, options(source.metadata()?.len()))?;
    io::copy(&mut source, zip)?;
    Ok(())
}

/// Every file under `dir`, in a stable order.
fn files_under(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_owned()];
    while let Some(d) = stack.pop() {
        for entry in fs::read_dir(&d)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            if kind.is_dir() {
                stack.push(entry.path());
            } else if kind.is_file() {
                out.push(entry.path());
            }
        }
    }
    out.sort();
    Ok(out)
}

/// The backups in `dir`, newest first (none if it does not exist).
pub(crate) fn list(dir: &Path) -> Result<Vec<BackupFile>> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e).with_context(|| format!("reading {}", dir.display())),
    };
    let mut out = Vec::new();
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if is_backup_name(&name) && entry.file_type()?.is_file() {
            out.push(describe(&entry.path())?);
        }
    }
    // The names hold their time, so they sort by it.
    out.sort_by(|a, b| b.name.cmp(&a.name));
    Ok(out)
}

/// The path of the backup `name` in `dir`, if there is one: a name the folder lists, never a
/// path that could leave it.
pub(crate) fn find(dir: &Path, name: &str) -> Option<PathBuf> {
    is_backup_name(name).then(|| dir.join(name)).filter(|p| p.is_file())
}

/// Deletes the backup `name`; false if there is none.
pub(crate) fn delete(dir: &Path, name: &str) -> Result<bool> {
    let Some(path) = find(dir, name) else { return Ok(false) };
    fs::remove_file(&path).with_context(|| format!("deleting {}", path.display()))?;
    Ok(true)
}

fn is_backup_name(name: &str) -> bool {
    name.strip_prefix(PREFIX).and_then(|s| s.strip_suffix(".zip")).is_some_and(|stamp| {
        !stamp.is_empty() && stamp.chars().all(|c| c.is_ascii_digit() || c == '-')
    })
}

fn describe(path: &Path) -> Result<BackupFile> {
    let meta = fs::metadata(path)?;
    let created = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0.0, |d| d.as_secs_f64());
    Ok(BackupFile {
        name: path.file_name().unwrap_or_default().to_string_lossy().into_owned(),
        bytes: meta.len(),
        created,
    })
}

/// Deletes all but the newest `keep` backups (0: none deleted); how many it deleted.
fn prune(dir: &Path, keep: u32) -> Result<usize> {
    if keep == 0 {
        return Ok(0);
    }
    let old: Vec<BackupFile> = list(dir)?.into_iter().skip(keep as usize).collect();
    for b in &old {
        fs::remove_file(dir.join(&b.name))?;
    }
    Ok(old.len())
}

/// What [`restore`] did.
#[derive(Debug, PartialEq)]
pub(crate) enum Restored {
    /// Restored; what was there before is in this folder.
    Done { aside: PathBuf },
    /// This backup was restored already (the setting is still in place): nothing done.
    Already,
}

/// What `restored.json` remembers: the backup last restored.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct Marker {
    file: String,
    bytes: u64,
    created: u64,
}

impl Marker {
    fn of(backup: &Path, manifest: &Manifest) -> Result<Self> {
        Ok(Self {
            file: backup.display().to_string(),
            bytes: fs::metadata(backup)?.len(),
            created: manifest.created,
        })
    }
}

/// Restores `backup` into the data folder, before the database opens (see the module's notes).
pub(crate) fn restore(paths: &DataPaths, backup: &Path) -> Result<Restored> {
    let mut zip = zip::ZipArchive::new(io::BufReader::new(
        File::open(backup).with_context(|| format!("opening {}", backup.display()))?,
    ))
    .with_context(|| format!("{} is not a SovEmu backup (not a zip file)", backup.display()))?;
    let manifest: Manifest = {
        let mut text = String::new();
        zip.by_name(MANIFEST)
            .with_context(|| {
                format!("{} is not a SovEmu backup (no {MANIFEST})", backup.display())
            })?
            .read_to_string(&mut text)?;
        serde_json::from_str(&text).with_context(|| format!("reading {MANIFEST}"))?
    };
    if manifest.format > FORMAT {
        bail!(
            "{} was made by SovEmu {} in a newer format: restore it with that version or later",
            backup.display(),
            manifest.version
        );
    }
    if zip.by_name(DATABASE).is_err() {
        bail!("{} holds no database", backup.display());
    }
    let marker = Marker::of(backup, &manifest)?;
    let marker_file = paths.root().join("restored.json");
    let last = fs::read(&marker_file).ok().and_then(|b| serde_json::from_slice::<Marker>(&b).ok());
    if last.as_ref() == Some(&marker) {
        return Ok(Restored::Already);
    }

    let aside = paths.root().join(format!("before-restore-{}", stamp(SystemTime::now())));
    fs::create_dir_all(&aside)?;
    let database = paths.database();
    let db_name = database.file_name().unwrap_or_default().to_owned();
    for (from, to) in [
        (database.clone(), aside.join(&db_name)),
        (with_suffix(&database, "-wal"), aside.join(format!("{}-wal", db_name.to_string_lossy()))),
        (with_suffix(&database, "-shm"), aside.join(format!("{}-shm", db_name.to_string_lossy()))),
        (paths.root().join(KEYS), aside.join(KEYS)),
        (paths.all_saves(), aside.join("saves")),
    ] {
        if from.exists() {
            fs::rename(&from, &to).with_context(|| format!("moving {} aside", from.display()))?;
        }
    }

    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        let Some(name) = entry.enclosed_name() else { continue };
        let target = match name.to_string_lossy().replace('\\', "/") {
            n if n == DATABASE => database.clone(),
            n if n == KEYS => paths.root().join(KEYS),
            n if n.starts_with(SAVES) && !entry.is_dir() => {
                paths.all_saves().join(&n[SAVES.len()..])
            }
            _ => continue,
        };
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        // Each file whole, or not at all: beside its name, then renamed.
        let partial = with_suffix(&target, ".restoring");
        io::copy(&mut entry, &mut File::create(&partial)?)?;
        fs::rename(&partial, &target)?;
    }
    fs::write(&marker_file, serde_json::to_vec_pretty(&marker)?)?;
    Ok(Restored::Done { aside })
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

/// A time as `YYYYMMDD-HHMMSS` (UTC), for file names that sort by it.
fn stamp(t: SystemTime) -> String {
    let secs = t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let (days, rest) = (secs / 86_400, secs % 86_400);
    let (y, m, d) = civil(days as i64);
    format!("{y:04}{m:02}{d:02}-{:02}{:02}{:02}", rest / 3600, rest % 3600 / 60, rest % 60)
}

/// Days since 1970-01-01 as a (year, month, day): Howard Hinnant's `civil_from_days`.
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("sovemu-backup-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn stamps_sort_by_time() {
        assert_eq!(stamp(UNIX_EPOCH), "19700101-000000");
        // 2026-11-22 16:30:05 UTC, a leap year's day after February, and a year's last second.
        assert_eq!(stamp(UNIX_EPOCH + Duration::from_secs(1_795_365_005)), "20261122-163005");
        assert_eq!(stamp(UNIX_EPOCH + Duration::from_secs(951_782_400)), "20000229-000000");
        assert_eq!(stamp(UNIX_EPOCH + Duration::from_secs(1_767_225_599)), "20251231-235959");
    }

    #[test]
    fn only_backup_names_are_listed_or_found() {
        assert!(is_backup_name("sovemu-backup-20261122-163005.zip"));
        for name in ["../sovemu.db", "sovemu-backup-.zip", "sovemu-backup-1/../x.zip", "x.zip"] {
            assert!(!is_backup_name(name), "{name}");
        }
    }

    #[tokio::test]
    async fn a_backup_restores_the_database_keys_and_saves_and_keeps_what_was_there() {
        let root = dir("round-trip");
        let paths = DataPaths::new(root.join("data"));
        fs::create_dir_all(paths.root()).unwrap();
        let db = crate::db::open(&paths.database()).await.unwrap();
        sqlx::query("INSERT INTO users (username, password_hash) VALUES ('kid', 'x')")
            .execute(&db)
            .await
            .unwrap();
        fs::write(paths.root().join(KEYS), b"{\"keys\":1}").unwrap();
        let save = paths.saves(1, "nes").join("Game.srm");
        fs::create_dir_all(save.parent().unwrap()).unwrap();
        fs::write(&save, b"progress").unwrap();

        let backups = root.join("backups");
        let made = create(&db, &paths, &backups, 2).await.unwrap();
        assert!(is_backup_name(&made.file.name), "{}", made.file.name);
        assert_eq!(list(&backups).unwrap(), std::slice::from_ref(&made.file));
        assert!(
            fs::read_dir(&backups).unwrap().count() == 1,
            "no partial or database copy is left beside it"
        );

        // Later: more progress, and the account gone. The restore brings back the backup's,
        // and keeps what was there aside.
        sqlx::query("DELETE FROM users").execute(&db).await.unwrap();
        db.close().await;
        fs::write(&save, b"newer progress").unwrap();
        let file = backups.join(&made.file.name);
        let Restored::Done { aside } = restore(&paths, &file).unwrap() else {
            panic!("restored nothing")
        };
        assert_eq!(fs::read(&save).unwrap(), b"progress");
        assert_eq!(fs::read(paths.root().join(KEYS)).unwrap(), b"{\"keys\":1}");
        assert_eq!(fs::read(aside.join("saves/1/nes/Game.srm")).unwrap(), b"newer progress");
        assert!(aside.join("sovemu.db").is_file());
        let db = crate::db::open(&paths.database()).await.unwrap();
        let users: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM users").fetch_one(&db).await.unwrap();
        assert_eq!(users, 1, "the backup's account is back");
        db.close().await;

        // A restart with the setting still in place restores nothing.
        assert_eq!(restore(&paths, &file).unwrap(), Restored::Already);
        assert_eq!(fs::read(&save).unwrap(), b"progress");
    }

    #[tokio::test]
    async fn the_newest_are_kept_and_anything_else_is_refused() {
        let root = dir("keep");
        let backups = root.join("backups");
        fs::create_dir_all(&backups).unwrap();
        for stamp in ["20260101-000000", "20260102-000000", "20260103-000000"] {
            fs::write(backups.join(format!("{PREFIX}{stamp}.zip")), b"x").unwrap();
        }
        fs::write(backups.join("notes.txt"), b"x").unwrap();
        assert_eq!(prune(&backups, 2).unwrap(), 1);
        let names: Vec<String> = list(&backups).unwrap().into_iter().map(|b| b.name).collect();
        assert_eq!(
            names,
            ["sovemu-backup-20260103-000000.zip", "sovemu-backup-20260102-000000.zip"]
        );
        assert!(backups.join("notes.txt").is_file(), "only backups are pruned");
        assert!(!delete(&backups, "notes.txt").unwrap());

        let paths = DataPaths::new(root.join("data"));
        fs::create_dir_all(paths.root()).unwrap();
        let not_zip = backups.join("sovemu-backup-20260103-000000.zip");
        let err = restore(&paths, &not_zip).unwrap_err().to_string();
        assert!(err.contains("not a SovEmu backup"), "{err}");
    }
}
