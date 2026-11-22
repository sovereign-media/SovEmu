//! Accounts and sign-in are `sovemu_identity`'s (`SovEmu.Identity`): local users with roles, and
//! the OAuth 2.1 / OpenID Connect provider every client signs in through. The server uses its
//! types under these names, and adds what only it knows: which accounts the environment declares
//! (reconciled at every start), which an admin may therefore not change here.

pub(crate) use sovemu_identity::{Credentials, NewUser, Role, User};

use crate::error::{ApiError, ApiResult};
use crate::{AppState, Config};

/// The accounts the environment declares (`SOVEMU_ADMIN_USERNAME` with its password, and
/// `SOVEMU_USERS__<n>__…`): reconciled at every start, so they are changed there, not here.
pub(crate) fn declared_usernames(config: &Config) -> Vec<String> {
    let admin = config.admin_username.clone().filter(|_| config.admin_password.is_some());
    admin.into_iter().chain(config.users.iter().map(|u| u.username.clone())).collect()
}

/// Whether `username` is one of [`declared_usernames`] (names match without regard to case).
pub(crate) fn is_declared(config: &Config, username: &str) -> bool {
    declared_usernames(config).iter().any(|d| d.eq_ignore_ascii_case(username))
}

/// The account an admin may change: it exists, and the environment does not declare it.
async fn changeable(state: &AppState, user_id: i64) -> ApiResult<User> {
    let user = state.identity.user(user_id).await?.ok_or_else(|| ApiError::not_found("user"))?;
    if is_declared(&state.config, &user.username) {
        return Err(ApiError::conflict(format!(
            "{} is declared in the environment, which sets it again at every start: change it there",
            user.username
        )));
    }
    Ok(user)
}

/// An admin changes an account's role.
pub(crate) async fn set_role(
    state: &AppState,
    admin: &User,
    user_id: i64,
    role: Role,
) -> ApiResult<User> {
    changeable(state, user_id).await?;
    Ok(state.identity.set_role(admin, user_id, role).await?)
}

/// An admin sets an account's password; that user is signed out everywhere. The account.
pub(crate) async fn set_password(
    state: &AppState,
    admin: &User,
    user_id: i64,
    password: &str,
) -> ApiResult<User> {
    let user = changeable(state, user_id).await?;
    state.identity.set_password(admin, user_id, password).await?;
    Ok(user)
}

/// An admin removes an account (their saves stay on disk, shown as a deleted user's).
pub(crate) async fn remove_user(state: &AppState, admin: &User, user_id: i64) -> ApiResult<User> {
    let user = changeable(state, user_id).await?;
    state.identity.remove_user(admin, user_id).await?;
    Ok(user)
}

/// An admin signs an account out of every browser and app. The account. Allowed for declared accounts too:
/// it changes nothing the environment sets.
pub(crate) async fn sign_out_everywhere(
    state: &AppState,
    admin: &User,
    user_id: i64,
) -> ApiResult<User> {
    let user = state.identity.user(user_id).await?.ok_or_else(|| ApiError::not_found("user"))?;
    state.identity.sign_out_everywhere(user_id).await?;
    tracing::info!(by = %admin.username, user.name = %user.username, "signed a user out everywhere");
    Ok(user)
}
