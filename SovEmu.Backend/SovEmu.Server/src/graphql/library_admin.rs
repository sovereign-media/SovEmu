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
    /// The configured sources, in the order they are asked.
    pub(super) sources: Vec<String>,
    /// Why a source stopped early (bad credentials, quota) or is not used, if one did.
    pub(super) notes: Vec<String>,
    /// The box art downloads that follow a lookup, in the background.
    pub(super) artwork: ArtworkStatus,
    /// The games being looked up now: several at once when a source's account allows it.
    pub(super) working: Vec<MetadataCurrent>,
    /// When the current (or last) lookup started and finished, Unix seconds.
    pub(super) started_at: Option<f64>,
    pub(super) finished_at: Option<f64>,
    /// Seconds left, from the time per game so far.
    pub(super) eta_seconds: Option<f64>,
    /// Each system's share of the lookup.
    pub(super) systems: Vec<MetadataSystemProgress>,
    /// What each source answered.
    pub(super) providers: Vec<MetadataProviderProgress>,
    /// The last games looked up, most recent first.
    pub(super) recent: Vec<MetadataRecent>,
}

/// What a source is doing for the game being looked up.
#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub(super) enum MetadataStep {
    /// Hashing the game's file, for a source that matches by hash.
    Hashing,
    Asking,
    /// Pacing between requests, or backing off a busy service.
    Waiting,
}

/// The game a lookup is on.
#[derive(SimpleObject)]
pub(super) struct MetadataCurrent {
    pub(super) game_id: i64,
    pub(super) title: String,
    pub(super) system: String,
    /// The source being asked, if one is.
    pub(super) provider: Option<String>,
    pub(super) step: MetadataStep,
}

#[derive(SimpleObject)]
pub(super) struct MetadataSystemProgress {
    pub(super) system: String,
    pub(super) total: i64,
    pub(super) done: i64,
}

/// A service's own count of today's requests.
#[derive(SimpleObject)]
pub(super) struct MetadataQuota {
    pub(super) requests_today: f64,
    pub(super) max_requests_per_day: Option<f64>,
    /// Lookups today that found nothing, which the service caps on its own.
    pub(super) misses_today: f64,
    pub(super) max_misses_per_day: Option<f64>,
}

#[derive(SimpleObject)]
pub(super) struct MetadataProviderProgress {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) asked: i64,
    pub(super) found: i64,
    pub(super) not_found: i64,
    pub(super) errors: i64,
    pub(super) quota: Option<MetadataQuota>,
    /// The account the source uses, as the service last described it.
    pub(super) account: Option<MetadataAccount>,
    /// Lookups the source takes at once, from its account's limits (and
    /// `SOVEMU_SCREENSCRAPER_THREADS`).
    pub(super) threads: i32,
}

/// A source's account: its level and the limits that come with it.
#[derive(SimpleObject)]
pub(super) struct MetadataAccount {
    pub(super) user: Option<String>,
    /// The service's own level number.
    pub(super) level: Option<i32>,
    /// ScreenScraper: 2 adds a thread, 3 or more add five.
    pub(super) contribution: Option<i32>,
    pub(super) max_threads: Option<i32>,
    pub(super) max_requests_per_minute: Option<i32>,
    pub(super) max_requests_per_day: Option<f64>,
    pub(super) max_misses_per_day: Option<f64>,
    /// KB/s.
    pub(super) max_download_kbps: Option<i32>,
}

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub(super) enum MetadataOutcome {
    Found,
    NotFound,
    /// Not stored: asked again by the next lookup.
    Failed,
}

/// A game a lookup has just looked up.
#[derive(SimpleObject)]
pub(super) struct MetadataRecent {
    pub(super) game_id: i64,
    /// Its title after the lookup.
    pub(super) title: String,
    pub(super) system: String,
    pub(super) outcome: MetadataOutcome,
    /// The sources that found something.
    pub(super) sources: Vec<String>,
    /// Why it failed, if it did.
    pub(super) detail: Option<String>,
}

/// A field of game information, to refresh only some.
#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub(super) enum MetadataField {
    Title,
    Description,
    ReleaseDate,
    Developer,
    Publisher,
    Genre,
    Players,
    Rating,
    BoxArt,
    /// A screenshot, shown behind the game.
    Screenshot,
    /// The game's logo.
    Marquee,
    Video,
}

impl From<MetadataField> for crate::metadata::Field {
    fn from(f: MetadataField) -> Self {
        use crate::metadata::Field as F;
        match f {
            MetadataField::Title => F::Title,
            MetadataField::Description => F::Description,
            MetadataField::ReleaseDate => F::ReleaseDate,
            MetadataField::Developer => F::Developer,
            MetadataField::Publisher => F::Publisher,
            MetadataField::Genre => F::Genre,
            MetadataField::Players => F::Players,
            MetadataField::Rating => F::Rating,
            MetadataField::BoxArt => F::BoxArt,
            MetadataField::Screenshot => F::Screenshot,
            MetadataField::Marquee => F::Marquee,
            MetadataField::Video => F::Video,
        }
    }
}

/// Artwork (box art, screenshots, logos) and videos being downloaded after a lookup.
#[derive(SimpleObject)]
pub(super) struct ArtworkStatus {
    pub(super) running: bool,
    /// Downloads queued since the job started.
    pub(super) total: i64,
    pub(super) done: i64,
    pub(super) downloaded: i64,
    pub(super) failed: i64,
    /// Of `downloaded`, the gameplay videos.
    pub(super) videos: i64,
}

impl From<crate::metadata::JobStatus> for MetadataStatus {
    fn from(j: crate::metadata::JobStatus) -> Self {
        use crate::metadata::Outcome;
        use sovemu_library::info::Step;
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
                videos: j.artwork.videos as i64,
            },
            working: j
                .working
                .into_iter()
                .map(|c| MetadataCurrent {
                    game_id: c.game_id,
                    title: c.title,
                    system: c.system,
                    provider: c.provider.map(str::to_owned),
                    step: match c.step {
                        Step::Hashing => MetadataStep::Hashing,
                        Step::Asking => MetadataStep::Asking,
                        Step::Waiting => MetadataStep::Waiting,
                    },
                })
                .collect(),
            started_at: j.started_at,
            finished_at: j.finished_at,
            eta_seconds: j.eta_seconds,
            systems: j
                .systems
                .into_iter()
                .map(|s| MetadataSystemProgress {
                    system: s.system,
                    total: s.total as i64,
                    done: s.done as i64,
                })
                .collect(),
            providers: j
                .providers
                .into_iter()
                .map(|p| MetadataProviderProgress {
                    id: p.id.to_owned(),
                    name: p.name.to_owned(),
                    asked: p.asked as i64,
                    found: p.found as i64,
                    not_found: p.not_found as i64,
                    errors: p.errors as i64,
                    quota: p.quota.map(|q| MetadataQuota {
                        requests_today: q.requests_today as f64,
                        max_requests_per_day: q.max_requests_per_day.map(|n| n as f64),
                        misses_today: q.misses_today as f64,
                        max_misses_per_day: q.max_misses_per_day.map(|n| n as f64),
                    }),
                    account: p.account.map(|a| MetadataAccount {
                        user: a.user,
                        level: a.level.map(|n| n as i32),
                        contribution: a.contribution.map(|n| n as i32),
                        max_threads: a.max_threads.map(|n| n as i32),
                        max_requests_per_minute: a.max_requests_per_minute.map(|n| n as i32),
                        max_requests_per_day: a.max_requests_per_day.map(|n| n as f64),
                        max_misses_per_day: a.max_misses_per_day.map(|n| n as f64),
                        max_download_kbps: a.max_download_kbps.map(|n| n as i32),
                    }),
                    threads: p.threads as i32,
                })
                .collect(),
            recent: j
                .recent
                .into_iter()
                .map(|r| MetadataRecent {
                    game_id: r.game_id,
                    title: r.title,
                    system: r.system,
                    outcome: match r.outcome {
                        Outcome::Found => MetadataOutcome::Found,
                        Outcome::NotFound => MetadataOutcome::NotFound,
                        Outcome::Failed => MetadataOutcome::Failed,
                    },
                    sources: r.sources.into_iter().map(str::to_owned).collect(),
                    detail: r.detail,
                })
                .collect(),
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
    /// with `force` every game, or with `staleAfterDays` the games last looked up longer ago;
    /// one system, or all. `fields` refreshes only those fields (the rest are kept) and
    /// `sources` asks only those sources (ids, as `metadataStatus.sources` lists them). What a
    /// game has is never cleared by a source that finds nothing.
    async fn fetch_metadata(
        &self,
        ctx: &Context<'_>,
        system: Option<ID>,
        #[graphql(default)] force: bool,
        #[graphql(default)] fields: Vec<MetadataField>,
        #[graphql(default)] sources: Vec<String>,
        stale_after_days: Option<i32>,
    ) -> Result<MetadataStatus> {
        let me = admin(ctx).await?;
        let on =
            system.as_ref().map_or_else(|| "every system".to_owned(), |s| s.as_str().to_owned());
        let stale_after_days = match stale_after_days {
            Some(d) if d < 1 => {
                return Err(ApiError::bad_request("staleAfterDays must be 1 or more")).api();
            }
            d => d.map(|d| d as u32),
        };
        let scope = crate::metadata::Scope {
            system: system.map(|s| s.to_string()),
            game: None,
            force,
            fields: fields.iter().map(|f| (*f).into()).collect(),
            providers: sources,
            stale_after_days,
        };
        let status = crate::admin::metadata_fetch(state(ctx), scope).api()?;
        let mut detail = on;
        if force {
            detail.push_str(", every game again");
        }
        if let Some(d) = stale_after_days {
            detail.push_str(&format!(", older than {d} days"));
        }
        if !fields.is_empty() {
            detail.push_str(&format!(", {} fields", fields.len()));
        }
        let entry =
            Entry::new(Kind::JobRun).about(crate::jobs::Job::Metadata.label()).detail(detail);
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
