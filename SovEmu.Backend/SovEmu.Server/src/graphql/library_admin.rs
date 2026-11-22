//! The admin's library: scans, the mirror, game information, soft patches, decorations and the game cache.

use super::*;
use crate::activity::{Entry, Kind};

/// A soft patch in the library, as the admin sees it.
#[derive(SimpleObject)]
pub(super) struct AdminPatch {
    pub(super) id: i64,
    /// From its file: "BPS patch" beside the game, else the file's name.
    pub(super) name: String,
    /// The file, relative to the ROM folder.
    pub(super) file: String,
    /// `IPS`, `UPS` or `BPS`.
    pub(super) format: String,
    /// Named like its game, so applied unless a player chose otherwise.
    pub(super) is_default: bool,
    pub(super) game_id: i64,
    pub(super) game_title: String,
    /// The game's system, by id.
    pub(super) system: String,
    /// Players who chose this patch.
    pub(super) chosen_by: i32,
    /// Players who chose the original for its game, over the default.
    pub(super) original_by: i32,
}

/// The decoration sets found, and the set each system's games are framed with.
pub(super) struct DecorationsReportNode(pub(super) crate::admin::DecorationsReport);

#[Object(name = "DecorationsReport")]
impl DecorationsReportNode {
    /// Every set found in the decoration folders, by name (`sovemu` is the built-in one).
    async fn sets(&self) -> Vec<DecorationSet> {
        self.0
            .sets
            .iter()
            .map(|(name, s)| DecorationSet {
                name: name.clone(),
                builtin: name == sovemu_library::decorations::BUILTIN,
                has_default: s.has_default,
                systems: s.systems.clone(),
                games: s.games as i32,
            })
            .collect()
    }
    /// The set for systems without one of their own (`SOVEMU_DECORATION`; `none`: no frame).
    async fn default(&self) -> &str {
        &self.0.default
    }
    /// The folders sets are read from, the first winning for the same file: those in
    /// `SOVEMU_DECORATIONS_DIRS`, `<data>/decorations`, then the built-in one.
    async fn folders(&self) -> &[String] {
        &self.0.folders
    }
    async fn systems(&self) -> Vec<SystemDecorationNode<'_>> {
        self.0.systems.iter().map(SystemDecorationNode).collect()
    }
}

/// A decoration set, and what it holds.
#[derive(SimpleObject)]
pub(super) struct DecorationSet {
    pub(super) name: String,
    /// SovEmu's own set, drawn at build time.
    pub(super) builtin: bool,
    /// It has an image for every game nothing closer covers (`default.png`).
    pub(super) has_default: bool,
    /// The systems with an image of their own, by id.
    pub(super) systems: Vec<String>,
    /// Images for single games.
    pub(super) games: i32,
}

/// A system and the decoration set its games are framed with.
pub(super) struct SystemDecorationNode<'a>(pub(super) &'a crate::admin::SystemDecoration);

#[Object(name = "SystemDecoration")]
impl SystemDecorationNode<'_> {
    async fn system(&self) -> SystemNode {
        SystemNode(self.0.system.clone())
    }
    /// The set (`none`: no frame).
    async fn set(&self) -> &str {
        &self.0.set
    }
    /// Chosen for this system (`SOVEMU_SYSTEMS__<id>__DECORATION`) rather than the default.
    async fn per_system(&self) -> bool {
        self.0.per_system
    }
    /// What its games are framed with when they have no image of their own. None for `none`,
    /// or a set that isn't there.
    async fn preview(&self) -> Option<DecorationNode> {
        self.0.preview.clone().map(DecorationNode)
    }
}

#[derive(SimpleObject)]
pub(super) struct ScanReport {
    pub(super) found: i64,
    pub(super) added: i64,
    pub(super) removed: i64,
}

/// How the library is read.
#[derive(Enum, Clone, Copy, PartialEq, Eq)]
pub(super) enum LibraryMode {
    /// Straight from the library (local disk or a share on the LAN).
    Direct,
    /// From the library, keeping local copies of what is used.
    Cache,
    /// From a full local copy kept in sync.
    Mirror,
}

/// A mirror sync's progress.
#[derive(SimpleObject)]
pub(super) struct MirrorSync {
    pub(super) running: bool,
    pub(super) files_total: i64,
    pub(super) files_done: i64,
    /// Bytes.
    pub(super) bytes_total: f64,
    /// Bytes.
    pub(super) bytes_done: f64,
    pub(super) removed: i64,
    pub(super) failed: i64,
    /// Unix seconds when the last sync finished.
    pub(super) finished_at: Option<f64>,
    pub(super) error: Option<String>,
}

#[derive(SimpleObject)]
pub(super) struct LibraryStatus {
    pub(super) mode: LibraryMode,
    pub(super) games: i64,
    /// `CACHE` mode: bytes on local disk.
    pub(super) cache_bytes: Option<f64>,
    /// `MIRROR` mode: the current or last sync.
    pub(super) mirror: Option<MirrorSync>,
}

/// A game-information lookup's progress.
#[derive(SimpleObject)]
pub(super) struct MetadataStatus {
    pub(super) running: bool,
    pub(super) total: i64,
    pub(super) done: i64,
    pub(super) found: i64,
    pub(super) not_found: i64,
    pub(super) failed: i64,
    /// The configured sources.
    pub(super) sources: Vec<String>,
    /// Why a source stopped early (bad credentials, quota), if one did.
    pub(super) notes: Vec<String>,
    /// The box art downloads that follow a lookup, in the background.
    pub(super) artwork: ArtworkStatus,
}

/// Box art being downloaded after a lookup, for the games that have no local thumb.
#[derive(SimpleObject)]
pub(super) struct ArtworkStatus {
    pub(super) running: bool,
    /// Downloads queued since the job started.
    pub(super) total: i64,
    pub(super) done: i64,
    pub(super) downloaded: i64,
    pub(super) failed: i64,
}

impl From<crate::metadata::JobStatus> for MetadataStatus {
    fn from(j: crate::metadata::JobStatus) -> Self {
        Self {
            running: j.running,
            total: j.total as i64,
            done: j.done as i64,
            found: j.found as i64,
            not_found: j.not_found as i64,
            failed: j.failed as i64,
            sources: j.sources.into_iter().map(str::to_owned).collect(),
            notes: j.notes,
            artwork: ArtworkStatus {
                running: j.artwork.running,
                total: j.artwork.total as i64,
                done: j.artwork.done as i64,
                downloaded: j.artwork.downloaded as i64,
                failed: j.artwork.failed as i64,
            },
        }
    }
}

#[derive(Default)]
pub(super) struct LibraryAdminQuery;

#[Object]
impl LibraryAdminQuery {
    /// Admins: how the library is played (direct, cache or mirror), its size, and the mirror's
    /// sync. Its errors can name the share's paths.
    async fn library_status(&self, ctx: &Context<'_>) -> Result<LibraryStatus> {
        admin(ctx).await?;
        let s = crate::admin::library_status(state(ctx)).await.api()?;
        Ok(LibraryStatus {
            mode: match s.mode {
                crate::LibraryMode::Direct => LibraryMode::Direct,
                crate::LibraryMode::Cache => LibraryMode::Cache,
                crate::LibraryMode::Mirror => LibraryMode::Mirror,
            },
            games: s.games,
            cache_bytes: s.cache_bytes.map(|b| b as f64),
            mirror: s.mirror.map(|m| MirrorSync {
                running: m.running,
                files_total: m.files_total as i64,
                files_done: m.files_done as i64,
                bytes_total: m.bytes_total as f64,
                bytes_done: m.bytes_done as f64,
                removed: m.removed as i64,
                failed: m.failed as i64,
                finished_at: m.finished_at.map(|t| t as f64),
                error: m.error,
            }),
        })
    }

    /// Admins: the game-information lookup's progress and its notes.
    async fn metadata_status(&self, ctx: &Context<'_>) -> Result<MetadataStatus> {
        admin(ctx).await?;
        Ok(crate::admin::metadata_status(state(ctx)).into())
    }

    /// Admins: every soft patch in the library, with its game and how many players chose it.
    async fn patches(&self, ctx: &Context<'_>) -> Result<Vec<AdminPatch>> {
        admin(ctx).await?;
        let all = crate::patches::all(state(ctx)).await.api()?;
        Ok(all
            .into_iter()
            .map(|p| AdminPatch {
                format: p.format(),
                id: p.id,
                name: p.name,
                file: p.path,
                is_default: p.is_default,
                game_id: p.game_id,
                game_title: p.game_title,
                system: p.system,
                chosen_by: p.chosen_by as i32,
                original_by: p.original_by as i32,
            })
            .collect())
    }

    /// Admins: the decoration sets found, and the set each system's games are framed with.
    async fn decorations(&self, ctx: &Context<'_>) -> Result<DecorationsReportNode> {
        admin(ctx).await?;
        crate::admin::decorations_report(state(ctx)).await.api().map(DecorationsReportNode)
    }
}

#[derive(Default)]
pub(super) struct LibraryAdminMutation;

#[Object]
impl LibraryAdminMutation {
    /// Admins: rescans the library.
    async fn scan_library(&self, ctx: &Context<'_>) -> Result<ScanReport> {
        let me = admin(ctx).await?;
        let r = crate::api::scan(state(ctx)).await.api()?;
        record(ctx, &me, Entry::new(Kind::JobRun).about(crate::jobs::Job::Scan.label()));
        Ok(ScanReport { found: r.found as i64, added: r.added as i64, removed: r.removed as i64 })
    }

    /// Admins: starts a mirror sync in the background (`MIRROR` mode only).
    async fn sync_library(&self, ctx: &Context<'_>) -> Result<bool> {
        let me = admin(ctx).await?;
        crate::admin::library_sync(state(ctx)).api()?;
        record(ctx, &me, Entry::new(Kind::JobRun).about(crate::jobs::Job::MirrorSync.label()));
        Ok(true)
    }

    /// Admins: starts looking up game information in the background: games without it, or
    /// with `force` every game; one system, or all.
    async fn fetch_metadata(
        &self,
        ctx: &Context<'_>,
        system: Option<ID>,
        #[graphql(default)] force: bool,
    ) -> Result<MetadataStatus> {
        let me = admin(ctx).await?;
        let on =
            system.as_ref().map_or_else(|| "every system".to_owned(), |s| s.as_str().to_owned());
        let status =
            crate::admin::metadata_fetch(state(ctx), system.map(|s| s.to_string()), force).api()?;
        let entry = Entry::new(Kind::JobRun)
            .about(crate::jobs::Job::Metadata.label())
            .detail(format!("{on}{}", if force { ", every game again" } else { "" }));
        record(ctx, &me, entry);
        Ok(status.into())
    }

    /// Plays a game with one of its soft patches from now on (`Game.patches`), or the original
    /// (`patchId` null). Its next session loads it so; a patched game keeps its own saves.
    /// Admins: deletes the game cache's copies (mode `cache`), but those of games being played;
    /// the bytes freed. Games are copied again as they are played.
    async fn clear_cache(&self, ctx: &Context<'_>) -> Result<f64> {
        let me = admin(ctx).await?;
        let freed = crate::storage::clear_cache(state(ctx)).await.api()?;
        let entry =
            Entry::new(Kind::CacheCleared).detail(format!("{} MB freed", freed / 1_000_000));
        record(ctx, &me, entry);
        Ok(freed as f64)
    }
}
