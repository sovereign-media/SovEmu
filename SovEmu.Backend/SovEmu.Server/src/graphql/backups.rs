//! Backups (`backups`): the backups kept, where and how often they are made, and deleting one.
//! One is made by the `BACKUP` job (`runJob`); one is restored only at startup, from
//! `SOVEMU_RESTORE_FROM` (`backup.rs`).

use super::*;
use crate::activity::{Entry, Kind};

/// A backup kept.
#[derive(SimpleObject)]
pub(super) struct BackupFile {
    pub(super) name: String,
    pub(super) bytes: f64,
    /// When it was made (Unix seconds).
    pub(super) created: f64,
    /// Its download: a signed URL, for this admin only.
    pub(super) url: String,
}

/// The backups, and how they are made.
#[derive(SimpleObject)]
pub(super) struct Backups {
    /// Where they are kept (`SOVEMU_BACKUP_DIR`).
    pub(super) dir: String,
    /// A backup every this many hours (`SOVEMU_BACKUP_INTERVAL_HOURS`; 0: none on a schedule).
    pub(super) interval_hours: f64,
    /// How many are kept (`SOVEMU_BACKUP_KEEP`; 0: every one).
    pub(super) keep: i32,
    /// The backup restored at startup, if `SOVEMU_RESTORE_FROM` is set.
    pub(super) restore_from: Option<String>,
    /// Newest first.
    pub(super) files: Vec<BackupFile>,
}

#[derive(Default)]
pub(super) struct BackupsQuery;

#[Object]
impl BackupsQuery {
    /// Admins: the backups kept, and how they are made.
    async fn backups(&self, ctx: &Context<'_>) -> Result<Backups> {
        let me = admin(ctx).await?;
        let state = state(ctx);
        let dir = state.config.backup_dir();
        let files = crate::backup::list(&dir).map_err(ApiError::internal).api()?;
        Ok(Backups {
            dir: dir.display().to_string(),
            interval_hours: state.config.backup_interval_hours as f64,
            keep: i32::try_from(state.config.backup_keep).unwrap_or(i32::MAX),
            restore_from: state
                .config
                .restore_from
                .as_ref()
                .filter(|p| !p.as_os_str().is_empty())
                .map(|p| p.display().to_string()),
            files: files
                .into_iter()
                .map(|f| BackupFile {
                    url: state.assets.sign(me.id, &format!("/media/backups/{}", f.name)),
                    name: f.name,
                    bytes: f.bytes as f64,
                    created: f.created,
                })
                .collect(),
        })
    }
}

#[derive(Default)]
pub(super) struct BackupsMutation;

#[Object]
impl BackupsMutation {
    /// Admins: deletes a backup (`NOT_FOUND` if there is none by that name).
    async fn delete_backup(&self, ctx: &Context<'_>, name: String) -> Result<bool> {
        let me = admin(ctx).await?;
        let dir = state(ctx).config.backup_dir();
        if !crate::backup::delete(&dir, &name).map_err(ApiError::internal).api()? {
            return Err(gql(ApiError::not_found("backup")));
        }
        record(
            ctx,
            &me,
            Entry::new(Kind::SettingsChanged).about("Backups").detail(format!("Deleted {name}")),
        );
        Ok(true)
    }
}
