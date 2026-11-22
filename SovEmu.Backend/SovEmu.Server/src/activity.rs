//! The activity log: who did what, and when. Sign-ins and refusals (`sovemu_identity`, as its
//! observer), games started (by their resolver) and how they ended (`sovemu_sessions`, as its
//! observer), and what admins changed (by each change's resolver). Kept in `activity`, read by
//! the admin Activity page, pruned after `SOVEMU_ACTIVITY_HISTORY_DAYS`.
//!
//! [`Activity::record`] never waits: it writes in the background, so recording cannot slow or
//! fail what it records. Nothing secret is recorded: subjects are names (an account, a game, a
//! job), details are outcomes (a role, an end reason), never a password or a token.

use std::net::IpAddr;

use sqlx::SqlitePool;

/// What happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    SignedIn,
    SignInFailed,
    /// Refused without checking: too many failures already.
    SignInLockedOut,
    GameStarted,
    GameEnded,
    UserAdded,
    RoleChanged,
    PasswordSet,
    UserRemoved,
    SignedOutEverywhere,
    SessionStopped,
    SettingsChanged,
    JobRun,
    CacheCleared,
}

/// The page's filter: what kind of thing happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Category {
    SignIn,
    Play,
    Admin,
}

impl Category {
    /// As it is stored.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::SignIn => "sign_in",
            Self::Play => "play",
            Self::Admin => "admin",
        }
    }
}

impl Kind {
    const ALL: [Self; 14] = [
        Self::SignedIn,
        Self::SignInFailed,
        Self::SignInLockedOut,
        Self::GameStarted,
        Self::GameEnded,
        Self::UserAdded,
        Self::RoleChanged,
        Self::PasswordSet,
        Self::UserRemoved,
        Self::SignedOutEverywhere,
        Self::SessionStopped,
        Self::SettingsChanged,
        Self::JobRun,
        Self::CacheCleared,
    ];

    /// As it is stored.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::SignedIn => "signed_in",
            Self::SignInFailed => "sign_in_failed",
            Self::SignInLockedOut => "sign_in_locked_out",
            Self::GameStarted => "game_started",
            Self::GameEnded => "game_ended",
            Self::UserAdded => "user_added",
            Self::RoleChanged => "role_changed",
            Self::PasswordSet => "password_set",
            Self::UserRemoved => "user_removed",
            Self::SignedOutEverywhere => "signed_out_everywhere",
            Self::SessionStopped => "session_stopped",
            Self::SettingsChanged => "settings_changed",
            Self::JobRun => "job_run",
            Self::CacheCleared => "cache_cleared",
        }
    }

    /// From what [`Kind::as_str`] wrote; none for a kind this build does not know.
    pub(crate) fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == text)
    }

    pub(crate) fn category(self) -> Category {
        match self {
            Self::SignedIn | Self::SignInFailed | Self::SignInLockedOut => Category::SignIn,
            Self::GameStarted | Self::GameEnded => Category::Play,
            _ => Category::Admin,
        }
    }

    #[cfg(test)]
    fn of(category: Category) -> impl Iterator<Item = Self> {
        Self::ALL.into_iter().filter(move |k| k.category() == category)
    }
}

/// One thing that happened, as recorded.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Entry {
    pub(crate) kind: Option<Kind>,
    /// The account that did it; none for a refused sign-in, or the server itself.
    pub(crate) user_id: Option<i64>,
    /// The name as it was given: kept for an account since deleted, and for a refused sign-in
    /// the name that was tried.
    pub(crate) username: Option<String>,
    /// What it was about: a game, an account, a job, a system's settings.
    pub(crate) subject: Option<String>,
    /// More: a new role, how a game ended.
    pub(crate) detail: Option<String>,
    pub(crate) address: Option<IpAddr>,
}

impl Entry {
    pub(crate) fn new(kind: Kind) -> Self {
        Self { kind: Some(kind), ..Self::default() }
    }

    pub(crate) fn by(mut self, user: &crate::auth::User) -> Self {
        self.user_id = Some(user.id);
        self.username = Some(user.username.clone());
        self
    }

    pub(crate) fn about(mut self, subject: impl Into<String>) -> Self {
        self.subject = Some(subject.into());
        self
    }

    pub(crate) fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub(crate) fn from(mut self, address: Option<IpAddr>) -> Self {
        self.address = address;
        self
    }
}

/// Records into the database, in the background.
#[derive(Debug, Clone)]
pub(crate) struct Activity {
    db: SqlitePool,
}

impl Activity {
    pub(crate) fn new(db: SqlitePool) -> Self {
        Self { db }
    }

    /// Records `entry` now, without waiting for the write. A write that fails is logged.
    pub(crate) fn record(&self, entry: Entry) {
        let db = self.db.clone();
        let at = crate::client_events::now_unix();
        // Outside a runtime (a synchronous test) there is nothing to write with.
        let Ok(runtime) = tokio::runtime::Handle::try_current() else { return };
        runtime.spawn(async move {
            if let Err(e) = insert(&db, &entry, at).await {
                tracing::warn!(error = %e, kind = ?entry.kind, "could not record activity");
            }
        });
    }
}

async fn insert(db: &SqlitePool, e: &Entry, at: i64) -> sqlx::Result<()> {
    let Some(kind) = e.kind else { return Ok(()) };
    sqlx::query(
        "INSERT INTO activity (at, user_id, username, kind, category, subject, detail, address)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(at)
    .bind(e.user_id)
    .bind(&e.username)
    .bind(kind.as_str())
    .bind(kind.category().as_str())
    .bind(&e.subject)
    .bind(&e.detail)
    .bind(e.address.map(|a| a.to_string()))
    .execute(db)
    .await?;
    Ok(())
}

impl sovemu_identity::Observer for Activity {
    fn signed_in(&self, user: &sovemu_identity::User, address: Option<IpAddr>) {
        self.record(Entry::new(Kind::SignedIn).by(user).from(address));
    }

    fn sign_in_failed(&self, username: &str, address: Option<IpAddr>, locked_out: bool) {
        let kind = if locked_out { Kind::SignInLockedOut } else { Kind::SignInFailed };
        // The name tried, kept to a sensible length: it is whatever was typed.
        let tried: String = username.chars().take(64).collect();
        self.record(Entry { username: Some(tried), ..Entry::new(kind).from(address) });
    }
}

impl sovemu_sessions::Observer for Activity {
    fn session_ended(&self, ended: &sovemu_sessions::Ended<'_>) {
        // A start that never loaded is a failure (`failures.rs`), not a game played.
        if !ended.loaded {
            return;
        }
        let minutes = (ended.ran.as_secs() + 30) / 60;
        self.record(Entry {
            user_id: Some(ended.user_id),
            ..Entry::new(Kind::GameEnded)
                .about(ended.title)
                .detail(format!("{} after {minutes} min", ended.reason.as_str()))
        });
    }
}

/// Deletes entries older than `days` (0: kept). How many.
pub(crate) async fn prune(db: &SqlitePool, days: u32, now: i64) -> sqlx::Result<u64> {
    if days == 0 {
        return Ok(0);
    }
    let cutoff = now - i64::from(days) * 86_400;
    let r = sqlx::query("DELETE FROM activity WHERE at < ?").bind(cutoff).execute(db).await?;
    Ok(r.rows_affected())
}

/// One stored entry.
#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub(crate) struct Stored {
    pub(crate) id: i64,
    pub(crate) at: i64,
    pub(crate) user_id: Option<i64>,
    pub(crate) username: Option<String>,
    pub(crate) kind: String,
    pub(crate) subject: Option<String>,
    pub(crate) detail: Option<String>,
    pub(crate) address: Option<String>,
}

/// Which entries a page asks for.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Filter {
    pub(crate) user_id: Option<i64>,
    pub(crate) category: Option<Category>,
    /// Only entries before this one (its time and id): the next page after it.
    pub(crate) before: Option<(i64, i64)>,
}

/// The newest entries the filter matches, newest first, at most `limit`. Paged by (time, id), so
/// a page never repeats or skips an entry written meanwhile.
pub(crate) const PAGE: &str = "SELECT id, at, user_id, username, kind, subject, detail, address
     FROM activity
     WHERE (?1 IS NULL OR user_id = ?1)
       AND (?2 IS NULL OR category = ?2)
       AND (?3 IS NULL OR at < ?3 OR (at = ?3 AND id < ?4))
     ORDER BY at DESC, id DESC LIMIT ?5";

pub(crate) async fn page(db: &SqlitePool, filter: Filter, limit: u32) -> sqlx::Result<Vec<Stored>> {
    let (before_at, before_id) = filter.before.unzip();
    sqlx::query_as(PAGE)
        .bind(filter.user_id)
        .bind(filter.category.map(Category::as_str))
        .bind(before_at)
        .bind(before_id)
        .bind(limit)
        .fetch_all(db)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn db() -> SqlitePool {
        let dir = std::env::temp_dir().join(format!("sovemu-activity-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        crate::db::open(&dir.join("activity.db")).await.unwrap()
    }

    #[test]
    fn kinds_read_back_as_written_and_fall_in_one_category() {
        for kind in Kind::ALL {
            assert_eq!(Kind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(Kind::parse("a kind from a newer build"), None);
        assert_eq!(Kind::of(Category::SignIn).count(), 3);
        assert_eq!(Kind::of(Category::Play).count(), 2);
        assert_eq!(Kind::ALL.len(), 3 + 2 + Kind::of(Category::Admin).count());
    }

    #[tokio::test]
    async fn pages_filter_by_user_and_category_and_follow_on() {
        let db = db().await;
        let user = |id| crate::auth::User {
            id,
            username: format!("user{id}"),
            role: crate::auth::Role::Viewer,
        };
        let entries = [
            (1, Entry::new(Kind::SignedIn).by(&user(1))),
            (2, Entry::new(Kind::GameStarted).by(&user(1)).about("Tetris")),
            (2, Entry::new(Kind::RoleChanged).by(&user(2)).about("user1").detail("admin")),
            (3, Entry::new(Kind::SignInFailed).from(Some([192, 168, 1, 20].into()))),
        ];
        for (at, entry) in &entries {
            insert(&db, entry, *at).await.unwrap();
        }
        let all = page(&db, Filter::default(), 10).await.unwrap();
        assert_eq!(
            all.iter().map(|e| e.kind.as_str()).collect::<Vec<_>>(),
            ["sign_in_failed", "role_changed", "game_started", "signed_in"],
            "newest first; equal times by id"
        );
        assert_eq!(all[0].address.as_deref(), Some("192.168.1.20"));
        let mine = Filter { user_id: Some(1), ..Filter::default() };
        assert_eq!(page(&db, mine, 10).await.unwrap().len(), 2);
        let admin = Filter { category: Some(Category::Admin), ..Filter::default() };
        assert_eq!(page(&db, admin, 10).await.unwrap()[0].detail.as_deref(), Some("admin"));

        // Two at a time: the second page follows on from the first's last entry.
        let first = page(&db, Filter::default(), 2).await.unwrap();
        let last = first.last().unwrap();
        let next = Filter { before: Some((last.at, last.id)), ..Filter::default() };
        let second = page(&db, next, 2).await.unwrap();
        assert_eq!(
            second.iter().map(|e| e.kind.as_str()).collect::<Vec<_>>(),
            ["game_started", "signed_in"]
        );

        assert_eq!(prune(&db, 1, 3 + 86_400).await.unwrap(), 3, "older than a day");
        assert_eq!(prune(&db, 0, i64::MAX).await.unwrap(), 0, "0: kept");
    }
}
