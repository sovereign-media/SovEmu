//! SovEmu's accounts and sign-in.
//!
//! - **Accounts**: local users with argon2 passwords and a [`Role`] (`viewer` plays, `admin` also
//!   runs the server), declared in the environment or added by an admin; sign-in throttling per
//!   username and per client address.
//! - **The sign-in provider**: an OAuth 2.1 authorization server from the `oauth-as` crate
//!   (stored in SQLite by [`SqliteStorage`]), with OpenID Connect added over it (`provider.rs`
//!   says what each side does). The web client signs in with the code flow and PKCE; the apps
//!   with the device grant, or the password fallback.
//! - **Who a request is**: [`Identity::user_for_request`] answers for a bearer access token, so
//!   the API never reads a cookie.
//!
//! The tables are the server's migrations (`SovEmu.Server/migrations/`); this crate is handed the
//! pool. It knows nothing about GraphQL.

mod accounts;
mod error;
mod keys;
mod provider;
mod sessions;
mod store;
mod throttle;

use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::http::{HeaderMap, HeaderValue};
use axum::response::{IntoResponse, Response};
use sqlx::SqlitePool;

pub use accounts::{Credentials, NewUser, Role, User};
pub use error::{Error, Result};

/// A live sign-in: a browser's web client or an app holding a refresh token for an account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignIn {
    /// Its refresh family: what [`Identity::end_sign_in`] takes.
    pub id: String,
    /// The account; none for a sign-in whose subject is not one (none are issued so).
    pub user_id: Option<i64>,
    /// [`WEB_CLIENT`] or [`NATIVE_CLIENT`].
    pub client_id: String,
    pub signed_in_at: std::time::SystemTime,
    /// When it last had an access token: about when the client was last in use.
    pub last_used_at: std::time::SystemTime,
    /// When it lapses unless used; none for a sign-in with no limit.
    pub expires_at: Option<std::time::SystemTime>,
}
pub use provider::{NATIVE_CLIENT, WEB_CLIENT};
pub use store::SqliteStorage;
pub use throttle::{LOCKOUT_WINDOW, LoginThrottle, MAX_FAILURES, MAX_FAILURES_PER_IP};

/// The address a request came from, as the server resolved it (behind trusted proxies, the
/// client's). Put on requests as an extension; sign-in failures count against it.
#[derive(Debug, Clone, Copy, Default)]
pub struct ClientIp(pub Option<IpAddr>);

/// Told about sign-ins, for the server's activity log. Every method does nothing by default.
pub trait Observer: Send + Sync + std::fmt::Debug {
    /// Someone signed in with their password (the sign-in form, or an app's password fallback).
    fn signed_in(&self, _user: &User, _address: Option<IpAddr>) {}
    /// A sign-in was refused: wrong credentials, or (`locked_out`) too many failures already.
    /// `username` is what was typed, which need not be an account.
    fn sign_in_failed(&self, _username: &str, _address: Option<IpAddr>, _locked_out: bool) {}
}

/// Nobody listening.
impl Observer for () {}

/// Sign-in settings.
#[derive(Debug, Clone)]
pub struct Config {
    /// The addresses people reach the server at (`http://192.168.1.10:8080`,
    /// `https://games.example.com`): each is an issuer, and the web client's callback there is
    /// registered. A request to any other address cannot sign in.
    pub public_urls: Vec<String>,
    /// The secret the ID-token signing key is derived from.
    pub token_secret: String,
    /// How long an access token lasts.
    pub access_token_minutes: u32,
    /// How long a sign-in lasts (an app's, from signing in; the browser's at the server), in
    /// days. 0: until it is revoked.
    pub session_days: u32,
    /// Whether the browser's sign-in cookie is `Secure` (served over HTTPS).
    pub secure_cookies: bool,
    /// Whether the apps may sign in with a password (`POST /oauth/password`).
    pub password_sign_in: bool,
    /// Failed sign-ins per username before a lockout (0: never).
    pub max_failures: u32,
    /// Failed sign-ins per client address, across usernames, before a lockout (0: never).
    pub max_failures_per_ip: u32,
    /// How long a lockout lasts.
    pub lockout: Duration,
    /// How long a just-used refresh token is still honoured (two tabs refreshing at once get the
    /// same new tokens); presenting it again after this ends that sign-in. 0: strictly once.
    pub refresh_retry: Duration,
    /// Told about sign-ins and refusals.
    pub observer: std::sync::Arc<dyn Observer>,
}

/// Where and how an app signs in at the address it reached: what an app asks first
/// (`bootstrapConfig` in the API), so it hard-codes no path but the API's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignInConfig {
    /// The issuer for this address.
    pub issuer: String,
    /// The web client's id (the code flow with PKCE).
    pub web_client_id: &'static str,
    /// The apps' id (the device grant, the password fallback).
    pub native_client_id: &'static str,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub revocation_endpoint: String,
    /// Where an app asks for a code to show (RFC 8628).
    pub device_authorization_endpoint: String,
    /// Where a person approves that code, signed in, in a browser.
    pub verification_uri: String,
    /// The password fallback, when `SOVEMU_PASSWORD_SIGN_IN` allows it.
    pub password_endpoint: Option<String>,
}

/// Accounts and sign-in. Cheap to clone.
#[derive(Clone)]
pub struct Identity {
    inner: Arc<Inner>,
}

struct Inner {
    db: SqlitePool,
    config: Config,
    provider: provider::Provider,
    key: keys::TokenKey,
    throttle: LoginThrottle,
    ip_throttle: LoginThrottle,
    /// Verified against when a username does not exist, so a failure takes the same time.
    dummy_hash: String,
    lifetimes: provider::Lifetimes,
}

impl std::fmt::Debug for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Identity").field("provider", &self.inner.provider).finish_non_exhaustive()
    }
}

impl Identity {
    /// Sets up sign-in over the server's database (migrated already).
    pub async fn new(db: SqlitePool, config: Config) -> Result<Self> {
        let days = u64::from(config.session_days);
        let lifetimes = provider::Lifetimes {
            access: Duration::from_secs(u64::from(config.access_token_minutes.max(1)) * 60),
            sign_in: (days > 0).then(|| Duration::from_secs(days * 86_400)),
            refresh_retry: config.refresh_retry,
        };
        let store = SqliteStorage::new(db.clone());
        let provider = provider::Provider::new(&config.public_urls, store, lifetimes).await?;
        let inner = Inner {
            key: keys::TokenKey::from_secret(&config.token_secret),
            throttle: LoginThrottle::new(config.max_failures, config.lockout),
            ip_throttle: LoginThrottle::new(config.max_failures_per_ip, config.lockout),
            dummy_hash: accounts::hash_password("not a real password")?,
            lifetimes,
            db,
            config,
            provider,
        };
        Ok(Self { inner: Arc::new(inner) })
    }

    /// The sign-in routes: OpenID Connect discovery, `/oauth/…`, and the device page. Mount them
    /// beside the API; they read a [`ClientIp`] extension when one is present.
    pub fn router(&self) -> axum::Router {
        provider::router(self.clone())
    }

    /// Where and how to sign in at the address a request reached (its `Host`); `None` when that
    /// address is not one of `SOVEMU_PUBLIC_URL`'s, so nothing can sign in there.
    pub fn sign_in_config(&self, headers: &HeaderMap) -> Option<SignInConfig> {
        let issuer = self.inner.provider.issuer(headers)?;
        let url = &issuer.url;
        // The endpoints as oauth-as was configured with them (`provider.rs`), so there is one
        // place they are named.
        let config = issuer.server.config();
        let endpoint = |e: &Option<String>| e.clone().expect("every endpoint is configured");
        Some(SignInConfig {
            issuer: url.clone(),
            web_client_id: WEB_CLIENT,
            native_client_id: NATIVE_CLIENT,
            authorization_endpoint: endpoint(&config.authorization_endpoint),
            token_endpoint: endpoint(&config.token_endpoint),
            revocation_endpoint: endpoint(&config.revocation_endpoint),
            device_authorization_endpoint: endpoint(&config.device_authorization_endpoint),
            verification_uri: config.verification_uri.clone(),
            password_endpoint: self
                .inner
                .config
                .password_sign_in
                .then(|| format!("{url}/oauth/password")),
        })
    }

    /// The user a request's bearer access token belongs to, if it is current. Checked against
    /// the store each time, so a revoked sign-in or a changed role applies at once.
    pub async fn user_for_request(&self, headers: &HeaderMap) -> Result<Option<User>> {
        let Some(token) = bearer(headers) else { return Ok(None) };
        let issuer = &self.inner.provider.issuers[0];
        let Some(issued) = issuer.server.introspect(&token).await.map_err(Error::internal)? else {
            return Ok(None);
        };
        self.user_of(issued.subject.as_deref()).await
    }

    async fn user_of(&self, subject: Option<&str>) -> Result<Option<User>> {
        let Some(id) = subject.and_then(|s| s.parse::<i64>().ok()) else { return Ok(None) };
        accounts::user_by_id(&self.inner.db, id).await
    }

    /// Whether first-run setup is still to do (there are no accounts).
    pub async fn setup_required(&self) -> Result<bool> {
        Ok(accounts::count_users(&self.inner.db).await? == 0)
    }

    /// First-run setup: creates the admin account and signs the browser in at the server (the
    /// web client then finishes signing in without the password). Only while there are no
    /// accounts. The user, and the `Set-Cookie` value.
    pub async fn setup(&self, creds: Credentials) -> Result<(User, HeaderValue)> {
        if !self.setup_required().await? {
            return Err(Error::Conflict("setup is already done".into()));
        }
        let new = NewUser { username: creds.username, password: creds.password, role: Role::Admin };
        let user = accounts::create_user(&self.inner.db, &new).await?;
        let cookie = self.sign_in_browser(user.id).await?;
        Ok((user, cookie))
    }

    async fn sign_in_browser(&self, user_id: i64) -> Result<HeaderValue> {
        let c = &self.inner.config;
        sessions::start(&self.inner.db, user_id, c.session_days.max(1), c.secure_cookies).await
    }

    /// Checks a username and password: the sign-in form's and the password fallback's one
    /// check. Failures count against the username and the client address; a locked-out one is
    /// [`Error::TooManyRequests`], wrong credentials [`Error::Unauthorized`].
    pub async fn check_password(
        &self,
        creds: &Credentials,
        client: Option<IpAddr>,
    ) -> Result<User> {
        let inner = &self.inner;
        let now = Instant::now();
        let address = client.map(|ip| ip.to_string());
        let locked = inner
            .throttle
            .check(&creds.username, now)
            .err()
            .or_else(|| address.as_deref().and_then(|a| inner.ip_throttle.check(a, now).err()));
        if let Some(wait) = locked {
            tracing::warn!(user.name = %creds.username, wait_s = wait, "sign-in refused: locked out");
            inner.config.observer.sign_in_failed(&creds.username, client, true);
            return Err(Error::TooManyRequests(format!(
                "too many failed attempts; try again in {wait} s"
            )));
        }
        let found = accounts::user_with_hash(&inner.db, &creds.username).await?;
        // Hash even for an unknown user, so timing does not reveal which usernames exist.
        let hash = found.as_ref().map_or(inner.dummy_hash.as_str(), |(_, h)| h.as_str());
        let valid = accounts::verify_password(&creds.password, hash);
        let Some((user, _)) = found.filter(|_| valid) else {
            inner.throttle.failed(&creds.username, now);
            if let Some(a) = &address {
                inner.ip_throttle.failed(a, now);
            }
            tracing::warn!(user.name = %creds.username, "sign-in failed");
            inner.config.observer.sign_in_failed(&creds.username, client, false);
            return Err(Error::Unauthorized("wrong username or password".into()));
        };
        inner.throttle.succeeded(&creds.username);
        tracing::info!(user.name = %user.username, user.id = user.id, "signed in");
        inner.config.observer.signed_in(&user, client);
        Ok(user)
    }

    /// The account with this id, if there is one.
    pub async fn user(&self, id: i64) -> Result<Option<User>> {
        accounts::user_by_id(&self.inner.db, id).await
    }

    /// When an account was created (UTC, `YYYY-MM-DD HH:MM:SS`), if there is one.
    pub async fn joined(&self, id: i64) -> Result<Option<String>> {
        accounts::joined(&self.inner.db, id).await
    }

    /// Every account, by name.
    pub async fn users(&self) -> Result<Vec<User>> {
        accounts::list_users(&self.inner.db).await
    }

    /// An admin adds an account.
    pub async fn add_user(&self, admin: &User, new: &NewUser) -> Result<User> {
        if !admin.is_admin() {
            return Err(Error::Forbidden("admins only".into()));
        }
        let user = accounts::create_user(&self.inner.db, new).await?;
        tracing::info!(
            by = %admin.username,
            user.name = %user.username,
            role = user.role.as_str(),
            "added a user"
        );
        Ok(user)
    }

    /// An admin changes an account's role. The last admin cannot be made a viewer.
    pub async fn set_role(&self, admin: &User, user_id: i64, role: Role) -> Result<User> {
        let target = self.target(admin, user_id).await?;
        if target.is_admin() && role != Role::Admin {
            self.keep_an_admin(&target, "made a viewer").await?;
        }
        accounts::set_role(&self.inner.db, user_id, role).await?;
        tracing::info!(by = %admin.username, user.name = %target.username, role = role.as_str(), "changed a role");
        Ok(User { role, ..target })
    }

    /// An admin sets an account's password, which signs that user out everywhere (as a password
    /// changed in the environment does).
    pub async fn set_password(&self, admin: &User, user_id: i64, password: &str) -> Result<()> {
        let target = self.target(admin, user_id).await?;
        accounts::set_password(&self.inner.db, user_id, password).await?;
        self.sign_out_everywhere(user_id).await?;
        tracing::info!(by = %admin.username, user.name = %target.username, "set a password");
        Ok(())
    }

    /// An admin removes an account, and every sign-in of theirs. Their own account and the last
    /// admin cannot be removed. Their saves stay on disk.
    pub async fn remove_user(&self, admin: &User, user_id: i64) -> Result<()> {
        let target = self.target(admin, user_id).await?;
        if target.id == admin.id {
            return Err(Error::Conflict("you cannot remove your own account".into()));
        }
        if target.is_admin() {
            self.keep_an_admin(&target, "removed").await?;
        }
        accounts::delete(&self.inner.db, user_id).await?;
        self.sign_out_everywhere(user_id).await?;
        tracing::info!(by = %admin.username, user.name = %target.username, "removed a user");
        Ok(())
    }

    /// The account an admin is about to change.
    async fn target(&self, admin: &User, user_id: i64) -> Result<User> {
        if !admin.is_admin() {
            return Err(Error::Forbidden("admins only".into()));
        }
        accounts::user_by_id(&self.inner.db, user_id)
            .await?
            .ok_or_else(|| Error::NotFound("no such user".into()))
    }

    /// Refuses a change that would leave no admin: `target` is one, and the last.
    async fn keep_an_admin(&self, target: &User, change: &str) -> Result<()> {
        if accounts::count_admins(&self.inner.db).await? <= 1 {
            return Err(Error::Conflict(format!(
                "{} is the only admin, and cannot be {change}",
                target.username
            )));
        }
        Ok(())
    }

    /// Makes sure a declared user exists with the declared password and role (the environment is
    /// the source of truth). A changed password signs that user out everywhere.
    pub async fn ensure_user(&self, username: &str, password: &str, role: Role) -> Result<()> {
        let (id, ensured) = accounts::ensure(&self.inner.db, username, password, role).await?;
        if ensured.password_changed {
            self.sign_out_everywhere(id).await?;
        }
        Ok(())
    }

    /// Deletes every account not in `keep`, and every sign-in of theirs; how many.
    pub async fn prune_users(&self, keep: &[String]) -> Result<u64> {
        let removed = accounts::prune(&self.inner.db, keep).await?;
        for id in &removed {
            self.sign_out_everywhere(*id).await?;
        }
        Ok(removed.len() as u64)
    }

    /// Ends every sign-in of a user: browsers at the server, and every app's tokens.
    pub async fn sign_out_everywhere(&self, user_id: i64) -> Result<()> {
        sessions::end_all(&self.inner.db, user_id).await?;
        self.store()
            .revoke_subject(&user_id.to_string(), self.revocation_window())
            .await
            .map_err(Error::internal)?;
        Ok(())
    }

    /// Every live sign-in, or one account's, the most recently used first.
    pub async fn sign_ins(&self, user_id: Option<i64>) -> Result<Vec<SignIn>> {
        let subject = user_id.map(|id| id.to_string());
        let stored = self.store().sign_ins(subject.as_deref()).await.map_err(Error::internal)?;
        Ok(stored
            .into_iter()
            .map(|s| SignIn {
                id: s.family_id,
                user_id: s.subject.and_then(|s| s.parse().ok()),
                client_id: s.client_id,
                signed_in_at: s.signed_in_at,
                last_used_at: s.last_used_at,
                expires_at: s.expires_at,
            })
            .collect())
    }

    /// An admin ends one sign-in (`id`, from [`Identity::sign_ins`]) through oauth-as's family
    /// revocation: that browser or app must sign in again. Its account's other sign-ins stand.
    pub async fn end_sign_in(&self, admin: &User, id: &str) -> Result<()> {
        if !admin.is_admin() {
            return Err(Error::Forbidden("admins only".into()));
        }
        if id.is_empty()
            || !self
                .store()
                .end_sign_in(id, self.revocation_window())
                .await
                .map_err(Error::internal)?
        {
            return Err(Error::NotFound("no such sign-in".into()));
        }
        tracing::info!(by = %admin.username, sign_in = %id, "ended a sign-in");
        Ok(())
    }

    fn store(&self) -> &store::SqliteStorage {
        self.inner.provider.issuers[0].server.store()
    }

    /// How long a revocation's barrier stands, from now: past every token the revoked sign-ins
    /// could still hold. A sign-in with no lifetime lives until revoked, so its barrier does too
    /// (in effect).
    fn revocation_window(&self) -> oauth_as::store::RevocationWindow {
        let now = std::time::SystemTime::now();
        let l = self.inner.lifetimes;
        let longest = l.sign_in.unwrap_or(Duration::from_secs(100 * 365 * 86_400)).max(l.access);
        oauth_as::store::RevocationWindow { recorded_at: now, until: now + longest }
    }

    /// Upkeep: expired browser sign-ins, codes, grants and tokens deleted. How many records.
    pub async fn maintain(&self) -> Result<u64> {
        let sessions = sessions::expire(&self.inner.db).await?;
        let store = self.inner.provider.issuers[0].server.store();
        let swept = oauth_as::store::Storage::sweep_expired(store, std::time::SystemTime::now())
            .await
            .map_err(Error::internal)?;
        Ok(sessions + swept)
    }
}

/// The bearer token in an `Authorization` header.
pub fn bearer(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(axum::http::header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    let token = token.trim();
    (scheme.eq_ignore_ascii_case("bearer") && !token.is_empty()).then(|| token.to_owned())
}

/// An error as a plain page response (the sign-in routes are not the API).
fn web_error(e: &Error) -> Response {
    (e.status(), e.message().to_owned()).into_response()
}

#[cfg(test)]
mod tests {
    use axum::http::header::AUTHORIZATION;

    use super::*;

    #[test]
    fn reads_bearer_tokens() {
        let mut headers = HeaderMap::new();
        assert_eq!(bearer(&headers), None);
        headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer abc"));
        assert_eq!(bearer(&headers).as_deref(), Some("abc"));
        headers.insert(AUTHORIZATION, HeaderValue::from_static("bearer  xyz "));
        assert_eq!(bearer(&headers).as_deref(), Some("xyz"));
        headers.insert(AUTHORIZATION, HeaderValue::from_static("Basic abc"));
        assert_eq!(bearer(&headers), None);
    }
}
