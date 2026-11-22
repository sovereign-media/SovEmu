//! The activity log: who did what, and when (`activity`).

use super::*;
use crate::activity::{Category, Filter, Kind};

/// What kind of thing happened, for the page's filter.
#[derive(Enum, Clone, Copy, PartialEq, Eq)]
pub(super) enum ActivityCategory {
    /// Signing in, and refusals.
    SignIn,
    /// Games started and ended.
    Play,
    /// What admins changed.
    Admin,
}

impl From<ActivityCategory> for Category {
    fn from(c: ActivityCategory) -> Self {
        match c {
            ActivityCategory::SignIn => Self::SignIn,
            ActivityCategory::Play => Self::Play,
            ActivityCategory::Admin => Self::Admin,
        }
    }
}

impl From<Category> for ActivityCategory {
    fn from(c: Category) -> Self {
        match c {
            Category::SignIn => Self::SignIn,
            Category::Play => Self::Play,
            Category::Admin => Self::Admin,
        }
    }
}

/// What happened.
#[derive(Enum, Clone, Copy, PartialEq, Eq)]
pub(super) enum ActivityKind {
    SignedIn,
    SignInFailed,
    /// Refused without checking: too many failed attempts already.
    SignInLockedOut,
    GameStarted,
    GameEnded,
    UserAdded,
    RoleChanged,
    PasswordSet,
    UserRemoved,
    SignedOutEverywhere,
    /// One browser or app signed out.
    SignInEnded,
    SessionStopped,
    SettingsChanged,
    JobRun,
    CacheCleared,
}

impl From<Kind> for ActivityKind {
    fn from(k: Kind) -> Self {
        match k {
            Kind::SignedIn => Self::SignedIn,
            Kind::SignInFailed => Self::SignInFailed,
            Kind::SignInLockedOut => Self::SignInLockedOut,
            Kind::GameStarted => Self::GameStarted,
            Kind::GameEnded => Self::GameEnded,
            Kind::UserAdded => Self::UserAdded,
            Kind::RoleChanged => Self::RoleChanged,
            Kind::PasswordSet => Self::PasswordSet,
            Kind::UserRemoved => Self::UserRemoved,
            Kind::SignedOutEverywhere => Self::SignedOutEverywhere,
            Kind::SignInEnded => Self::SignInEnded,
            Kind::SessionStopped => Self::SessionStopped,
            Kind::SettingsChanged => Self::SettingsChanged,
            Kind::JobRun => Self::JobRun,
            Kind::CacheCleared => Self::CacheCleared,
        }
    }
}

/// One thing that happened.
#[derive(SimpleObject)]
pub(super) struct ActivityEntry {
    /// When (Unix seconds).
    pub(super) at: f64,
    /// The account that did it; null for a refused sign-in.
    pub(super) user_id: Option<i64>,
    /// Its name now, else the name it had (an account since deleted), or for a refused sign-in
    /// the name that was tried.
    pub(super) username: Option<String>,
    pub(super) kind: ActivityKind,
    pub(super) category: ActivityCategory,
    /// What it was about: a game, an account, a job, a system's settings.
    pub(super) subject: Option<String>,
    /// More: a new role, how a game ended.
    pub(super) detail: Option<String>,
    /// The client's address, where there was a request.
    pub(super) address: Option<String>,
}

/// A page of the activity log, newest first.
#[derive(SimpleObject)]
pub(super) struct ActivityPage {
    pub(super) entries: Vec<ActivityEntry>,
    /// Give as `after` for the next page; null when this was the last.
    pub(super) next: Option<String>,
}

/// A page's end, as `at:id`.
fn cursor(at: i64, id: i64) -> String {
    format!("{at}:{id}")
}

fn parse_cursor(text: &str) -> Option<(i64, i64)> {
    let (at, id) = text.split_once(':')?;
    Some((at.parse().ok()?, id.parse().ok()?))
}

const MAX_PAGE: i32 = 500;

#[derive(Default)]
pub(super) struct ActivityQuery;

#[Object]
impl ActivityQuery {
    /// Admins: the activity log, newest first: one account's (`userId`), one kind of thing
    /// (`category`), or all. Up to `limit` (500 at most) a page; pass a page's `next` as `after`
    /// for the one after it.
    async fn activity(
        &self,
        ctx: &Context<'_>,
        user_id: Option<i64>,
        category: Option<ActivityCategory>,
        after: Option<String>,
        #[graphql(default = 100)] limit: i32,
    ) -> Result<ActivityPage> {
        admin(ctx).await?;
        let state = state(ctx);
        let before = match after.as_deref() {
            None => None,
            Some(text) => Some(
                parse_cursor(text)
                    .ok_or_else(|| gql(ApiError::bad_request("after: not a page")))?,
            ),
        };
        let limit = limit.clamp(1, MAX_PAGE);
        let filter = Filter { user_id, category: category.map(Into::into), before };
        let rows = crate::activity::page(&state.db, filter, limit as u32)
            .await
            .map_err(ApiError::from)
            .api()?;
        let next = (rows.len() == limit as usize)
            .then(|| rows.last().map(|r| cursor(r.at, r.id)))
            .flatten();
        let names = crate::api::usernames(state).await.api()?;
        let entries = rows
            .into_iter()
            .filter_map(|r| {
                // A kind from a newer build is left out rather than misnamed.
                let kind = Kind::parse(&r.kind)?;
                Some(ActivityEntry {
                    at: r.at as f64,
                    user_id: r.user_id,
                    username: r.user_id.and_then(|id| names.get(&id).cloned()).or(r.username),
                    kind: kind.into(),
                    category: kind.category().into(),
                    subject: r.subject,
                    detail: r.detail,
                    address: r.address,
                })
            })
            .collect();
        Ok(ActivityPage { entries, next })
    }
}
