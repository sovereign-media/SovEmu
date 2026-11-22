//! One account as the admin's user page shows it: the account, when it joined, every browser and
//! app signed in as it (`sovemu_identity`'s sign-ins, each of which can be ended), what it has
//! played (`play_history`), and its saves on disk.

use sovemu_identity::SignIn;

use crate::AppState;
use crate::auth::User;
use crate::error::{ApiError, ApiResult};

/// What an account has played, from `play_history`.
#[derive(Debug, Clone, Default, PartialEq, sqlx::FromRow)]
pub(crate) struct Played {
    /// Different games started.
    pub(crate) games: i64,
    /// Starts in all.
    pub(crate) starts: i64,
    /// The latest (UTC, `YYYY-MM-DD HH:MM:SS`); none if nothing yet.
    pub(crate) last_played_at: Option<String>,
}

pub(crate) struct UserDetails {
    pub(crate) user: User,
    /// UTC, `YYYY-MM-DD HH:MM:SS`.
    pub(crate) joined_at: Option<String>,
    pub(crate) sign_ins: Vec<SignIn>,
    pub(crate) played: Played,
    /// Its saves on disk: bytes and files.
    pub(crate) saves: (u64, u64),
}

pub(crate) async fn details(state: &AppState, user_id: i64) -> ApiResult<UserDetails> {
    let user = state.identity.user(user_id).await?.ok_or_else(|| ApiError::not_found("user"))?;
    let played: Played = sqlx::query_as(
        "SELECT COUNT(*) AS games, COALESCE(SUM(play_count), 0) AS starts,
                MAX(last_played_at) AS last_played_at
           FROM play_history WHERE user_id = ?",
    )
    .bind(user_id)
    .fetch_one(&state.db)
    .await?;
    Ok(UserDetails {
        joined_at: state.identity.joined(user_id).await?,
        sign_ins: state.identity.sign_ins(Some(user_id)).await?,
        played,
        saves: crate::storage::user_saves(state, user_id).await?,
        user,
    })
}
