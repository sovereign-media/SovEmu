//! Games needing attention: what the library lacks or could not place, and looking a game up
//! again (`attention`).

use super::library_admin::MetadataStatus;
use super::*;
use crate::activity::{Entry, Kind as Activity};
use crate::attention::Kind;

/// What needs attention.
#[derive(Enum, Clone, Copy, PartialEq, Eq)]
pub(super) enum AttentionKind {
    /// No game information: never looked up, or not found.
    NoInformation,
    /// Has information, but is queued to be looked up again (a source stopped before reaching
    /// it, or fields were added since); the lookup after each scan asks it.
    AwaitingLookup,
    /// No box art, beside it or downloaded.
    NoBoxArt,
    /// No gameplay video downloaded: none found, not downloaded yet, failing, or given up (the
    /// detail says which, and why). Listed only while `SovEmu__Metadata__Videos` is on.
    NoVideo,
    /// A picture or video whose download failed 3 times and is no longer tried: the detail
    /// names each, with the reason its last attempt failed. `retryArtworkDownloads` tries
    /// them again.
    DownloadGaveUp,
    /// The same title and tags in two or more files of a system.
    Duplicate,
    /// A multi-part game with a part missing.
    MissingParts,
    /// A file the last scan could not place as a game.
    Unrecognised,
}

impl From<AttentionKind> for Kind {
    fn from(k: AttentionKind) -> Self {
        match k {
            AttentionKind::NoInformation => Self::NoInformation,
            AttentionKind::AwaitingLookup => Self::AwaitingLookup,
            AttentionKind::NoBoxArt => Self::NoBoxArt,
            AttentionKind::NoVideo => Self::NoVideo,
            AttentionKind::DownloadGaveUp => Self::DownloadGaveUp,
            AttentionKind::Duplicate => Self::Duplicate,
            AttentionKind::MissingParts => Self::MissingParts,
            AttentionKind::Unrecognised => Self::Unrecognised,
        }
    }
}

impl From<Kind> for AttentionKind {
    fn from(k: Kind) -> Self {
        match k {
            Kind::NoInformation => Self::NoInformation,
            Kind::AwaitingLookup => Self::AwaitingLookup,
            Kind::NoBoxArt => Self::NoBoxArt,
            Kind::NoVideo => Self::NoVideo,
            Kind::DownloadGaveUp => Self::DownloadGaveUp,
            Kind::Duplicate => Self::Duplicate,
            Kind::MissingParts => Self::MissingParts,
            Kind::Unrecognised => Self::Unrecognised,
        }
    }
}

/// A game (or file) needing attention.
#[derive(SimpleObject)]
pub(super) struct AttentionItem {
    /// Null for a file that is not a game.
    pub(super) game_id: Option<i64>,
    /// Empty for a file in a folder no system uses.
    pub(super) system: String,
    /// Empty for a file that is not a game.
    pub(super) title: String,
    /// Relative to the library.
    pub(super) path: String,
    /// Why it has no information, the tags it shares, the parts missing, why a file was
    /// skipped, or where its video or a download stands and why (a line per download).
    pub(super) detail: String,
}

/// How many of a kind a system has.
#[derive(SimpleObject)]
pub(super) struct AttentionCount {
    pub(super) kind: AttentionKind,
    /// Empty for a folder no system uses.
    pub(super) system: String,
    pub(super) count: i32,
}

const MAX_ITEMS: i32 = 1000;

#[derive(Default)]
pub(super) struct AttentionQuery;

#[Object]
impl AttentionQuery {
    /// Admins: the games (or files) of one kind needing attention, one system's or all, by
    /// system and title (at most `limit`, up to 1000).
    async fn games_needing_attention(
        &self,
        ctx: &Context<'_>,
        kind: AttentionKind,
        system: Option<ID>,
        #[graphql(default = 200)] limit: i32,
    ) -> Result<Vec<AttentionItem>> {
        admin(ctx).await?;
        let limit = limit.clamp(1, MAX_ITEMS) as u32;
        let items = crate::attention::items(
            &state(ctx).db,
            kind.into(),
            system.as_ref().map(|s| s.as_str()),
            limit,
            state(ctx).metadata.videos,
        )
        .await
        .map_err(ApiError::from)
        .api()?;
        Ok(items
            .into_iter()
            .map(|i| AttentionItem {
                game_id: i.game_id,
                system: i.system,
                title: i.title,
                path: i.path,
                detail: i.detail,
            })
            .collect())
    }

    /// Admins: how many games (or files) of each kind need attention, per system.
    async fn attention_counts(&self, ctx: &Context<'_>) -> Result<Vec<AttentionCount>> {
        admin(ctx).await?;
        let counts = crate::attention::counts(&state(ctx).db, state(ctx).metadata.videos)
            .await
            .map_err(ApiError::from)
            .api()?;
        Ok(counts
            .into_iter()
            .map(|(kind, system, count)| AttentionCount {
                kind: kind.into(),
                system,
                count: count as i32,
            })
            .collect())
    }
}

#[derive(Default)]
pub(super) struct AttentionMutation;

#[Object]
impl AttentionMutation {
    /// Admins: looks one game up again (its game information and box art), as a background
    /// job (`METADATA`), whether or not it has information already. `CONFLICT` while a lookup
    /// runs.
    async fn look_up_game(&self, ctx: &Context<'_>, game_id: i64) -> Result<MetadataStatus> {
        let me = admin(ctx).await?;
        let state = state(ctx);
        crate::api::require_game(state, game_id).await.api()?;
        let scope = crate::metadata::Scope {
            game: Some(game_id),
            force: true,
            ..crate::metadata::Scope::default()
        };
        let status = crate::admin::metadata_fetch(state, scope).api()?;
        let title: Option<String> = sqlx::query_scalar("SELECT title FROM games WHERE id = ?")
            .bind(game_id)
            .fetch_optional(&state.db)
            .await
            .ok()
            .flatten();
        let entry = Entry::new(Activity::JobRun)
            .about(crate::jobs::Job::Metadata.label())
            .detail(title.unwrap_or_else(|| format!("game {game_id}")));
        record(ctx, &me, entry);
        Ok(status.into())
    }

    /// Admins: tries again the pictures and videos whose downloads gave up
    /// (`DOWNLOAD_GAVE_UP`), one system's or all: their attempts start again and the
    /// downloads start in the background, unless they are running (the next round takes
    /// them). Returns how many downloads were queued again.
    async fn retry_artwork_downloads(&self, ctx: &Context<'_>, system: Option<ID>) -> Result<i32> {
        let me = admin(ctx).await?;
        let state = state(ctx);
        let system = system.as_ref().map(|s| s.as_str());
        let queued = crate::admin::retry_downloads(state, system).await.api()?;
        let entry = Entry::new(Activity::JobRun).about("Artwork downloads").detail(format!(
            "{queued} given up, tried again{}",
            match system {
                Some(s) => format!(" ({s})"),
                None => String::new(),
            }
        ));
        record(ctx, &me, entry);
        Ok(queued as i32)
    }
}
