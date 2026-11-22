//! Accounts and sign-in: who is signed in, where to sign in, the server's build, and the users an admin adds.

use super::*;
use crate::activity::{Entry, Kind};

/// What an account may do.
#[derive(Enum, Clone, Copy, PartialEq, Eq)]
#[graphql(remote = "sovemu_identity::Role", name = "Role")]
pub(super) enum RoleKind {
    /// Plays games: browse the library, play, keep saves and settings of their own.
    Viewer,
    /// Also runs the server: users, the library, emulator settings.
    Admin,
}

/// An account.
#[derive(SimpleObject)]
#[graphql(name = "User", complex)]
pub(super) struct Account {
    pub(super) id: i64,
    pub(super) username: String,
    pub(super) role: RoleKind,
    /// Whether the role is `ADMIN`.
    pub(super) is_admin: bool,
}

#[async_graphql::ComplexObject]
impl Account {
    /// The environment declares it (`SOVEMU_ADMIN_USERNAME`, `SOVEMU_USERS__<n>__…`) and sets
    /// it again at every start: its role and password are changed there, and it is not removed
    /// here.
    async fn declared(&self, ctx: &Context<'_>) -> bool {
        crate::auth::is_declared(&state(ctx).config, &self.username)
    }
}

impl From<User> for Account {
    fn from(user: User) -> Self {
        let is_admin = user.is_admin();
        Self { id: user.id, username: user.username, role: user.role.into(), is_admin }
    }
}

/// Where and how to sign in at the address this request reached. Apps ask it first, signed out,
/// and use these URLs as given (they hard-code nothing but `/graphql`).
#[derive(SimpleObject)]
pub(super) struct BootstrapConfig {
    /// Whether anything can sign in at this address. False when it is not one of
    /// `SOVEMU_PUBLIC_URL`'s; then the rest is null, and `address` says what to add.
    pub(super) sign_in_available: bool,
    /// The address the request reached, as `Host` named it.
    pub(super) address: String,
    /// The OpenID Connect issuer for this address (discovery at
    /// `<issuer>/.well-known/openid-configuration`).
    pub(super) issuer: Option<String>,
    /// The web client's id: the authorization code flow with PKCE.
    pub(super) web_client_id: Option<String>,
    /// The apps' id: the device grant, and the password fallback.
    pub(super) native_client_id: Option<String>,
    pub(super) authorization_endpoint: Option<String>,
    pub(super) token_endpoint: Option<String>,
    pub(super) revocation_endpoint: Option<String>,
    /// Where an app asks for a code to show (RFC 8628 device authorization).
    pub(super) device_authorization_endpoint: Option<String>,
    /// Where a person approves that code, in a signed-in browser.
    pub(super) verification_uri: Option<String>,
    /// The apps' password fallback: a form post of `client_id`, `username`, `password` and
    /// `scope`, answered like the token endpoint. Null when `SOVEMU_PASSWORD_SIGN_IN` is off.
    pub(super) password_endpoint: Option<String>,
}

#[derive(SimpleObject)]
pub(super) struct Viewer {
    /// No account exists yet: the client should offer first-run setup.
    pub(super) setup_required: bool,
    /// The signed-in user, if any.
    pub(super) user: Option<Account>,
}

#[derive(SimpleObject)]
pub(super) struct ClientConfig {
    /// `RTCIceServer` objects (STUN/TURN) for WebRTC, from `SOVEMU_ICE_SERVERS`.
    pub(super) ice_servers: Json<serde_json::Value>,
}

/// Which build of the server this is.
#[derive(SimpleObject)]
pub(super) struct BuildInfoNode {
    /// The server's version.
    pub(super) version: String,
    /// The commit it was built from; null for a local build.
    pub(super) revision: Option<String>,
    /// That commit's time (ISO 8601); null for a local build.
    pub(super) built: Option<String>,
    /// The libretro stable release its cores come from.
    pub(super) libretro_release: String,
}

#[derive(InputObject)]
pub(super) struct NewUserInput {
    pub(super) username: String,
    pub(super) password: String,
    #[graphql(default_with = "RoleKind::Viewer")]
    pub(super) role: RoleKind,
}

#[derive(Default)]
pub(super) struct AccountsQuery;

#[Object]
impl AccountsQuery {
    /// Where and how to sign in here. Works signed out: an app's first question, which also tells
    /// it a SovEmu server is at the address it was given.
    async fn bootstrap_config(&self, ctx: &Context<'_>) -> BootstrapConfig {
        let headers = &ctx.data_unchecked::<Auth>().headers;
        let address = headers
            .get(axum::http::header::HOST)
            .and_then(|h| h.to_str().ok())
            .unwrap_or_default()
            .to_owned();
        let config = state(ctx).identity.sign_in_config(headers);
        BootstrapConfig {
            sign_in_available: config.is_some(),
            address,
            issuer: config.as_ref().map(|c| c.issuer.clone()),
            web_client_id: config.as_ref().map(|c| c.web_client_id.to_owned()),
            native_client_id: config.as_ref().map(|c| c.native_client_id.to_owned()),
            authorization_endpoint: config.as_ref().map(|c| c.authorization_endpoint.clone()),
            token_endpoint: config.as_ref().map(|c| c.token_endpoint.clone()),
            revocation_endpoint: config.as_ref().map(|c| c.revocation_endpoint.clone()),
            device_authorization_endpoint: config
                .as_ref()
                .map(|c| c.device_authorization_endpoint.clone()),
            verification_uri: config.as_ref().map(|c| c.verification_uri.clone()),
            password_endpoint: config.and_then(|c| c.password_endpoint),
        }
    }

    /// Which build of the server this is, for anyone signed in (an app can show it).
    async fn build_info(&self, ctx: &Context<'_>) -> Result<BuildInfoNode> {
        user(ctx).await?;
        let b = crate::about::BUILD;
        Ok(BuildInfoNode {
            version: b.version.to_owned(),
            revision: b.revision.map(str::to_owned),
            built: b.built.map(str::to_owned),
            libretro_release: b.libretro_release.to_owned(),
        })
    }

    /// Who is signed in, and whether first-run setup is needed. Works signed out.
    async fn viewer(&self, ctx: &Context<'_>) -> Result<Viewer> {
        let setup_required =
            state(ctx).identity.setup_required().await.map_err(|e| gql(e.into()))?;
        Ok(Viewer { setup_required, user: viewer(ctx).await?.map(Account::from) })
    }

    async fn client_config(&self, ctx: &Context<'_>) -> Result<ClientConfig> {
        user(ctx).await?;
        Ok(ClientConfig { ice_servers: Json(state(ctx).config.ice_servers.clone()) })
    }

    /// Admins: the accounts.
    async fn users(&self, ctx: &Context<'_>) -> Result<Vec<Account>> {
        admin(ctx).await?;
        let users = state(ctx).identity.users().await.map_err(|e| gql(e.into()))?;
        Ok(users.into_iter().map(Account::from).collect())
    }
}

#[derive(Default)]
pub(super) struct AccountsMutation;

#[Object]
impl AccountsMutation {
    /// First run: creates the admin account. Only while no account exists. The browser is then
    /// signed in at the server's sign-in page (a cookie only `/oauth/authorize` reads), so the
    /// web client finishes signing in there without asking for the password again. Apps sign in
    /// at `/oauth/token`; there is no sign-in mutation.
    async fn setup(
        &self,
        ctx: &Context<'_>,
        username: String,
        password: String,
    ) -> Result<Account> {
        let (user, cookie) = state(ctx)
            .identity
            .setup(Credentials { username, password })
            .await
            .map_err(|e| gql(e.into()))?;
        set_cookie(ctx, cookie);
        Ok(user.into())
    }

    /// Admins: adds an account (a viewer unless `role` says otherwise).
    async fn add_user(&self, ctx: &Context<'_>, input: NewUserInput) -> Result<Account> {
        let me = admin(ctx).await?;
        let new =
            NewUser { username: input.username, password: input.password, role: input.role.into() };
        let user = state(ctx).identity.add_user(&me, &new).await.map_err(|e| gql(e.into()))?;
        let entry = Entry::new(Kind::UserAdded).about(&user.username).detail(user.role.as_str());
        record(ctx, &me, entry);
        Ok(user.into())
    }

    /// Admins: makes an account an admin or a viewer. Not the last admin, and not an account the
    /// environment declares.
    async fn set_user_role(
        &self,
        ctx: &Context<'_>,
        user_id: i64,
        role: RoleKind,
    ) -> Result<Account> {
        let me = admin(ctx).await?;
        let user = crate::auth::set_role(state(ctx), &me, user_id, role.into()).await.api()?;
        let entry = Entry::new(Kind::RoleChanged).about(&user.username).detail(user.role.as_str());
        record(ctx, &me, entry);
        Ok(user.into())
    }

    /// Admins: sets an account's password (at least 8 characters), which signs that user out
    /// everywhere. Not an account the environment declares.
    async fn set_user_password(
        &self,
        ctx: &Context<'_>,
        user_id: i64,
        password: String,
    ) -> Result<bool> {
        let me = admin(ctx).await?;
        let user = crate::auth::set_password(state(ctx), &me, user_id, &password).await.api()?;
        record(ctx, &me, Entry::new(Kind::PasswordSet).about(user.username));
        Ok(true)
    }

    /// Admins: removes an account and signs it out everywhere; everything of theirs in the
    /// database goes with it, and their saves stay on disk. Not their own, not the last admin,
    /// and not an account the environment declares.
    async fn remove_user(&self, ctx: &Context<'_>, user_id: i64) -> Result<bool> {
        let me = admin(ctx).await?;
        let user = crate::auth::remove_user(state(ctx), &me, user_id).await.api()?;
        record(ctx, &me, Entry::new(Kind::UserRemoved).about(user.username));
        Ok(true)
    }

    /// Admins: signs an account out of every browser and app.
    async fn sign_out_everywhere(&self, ctx: &Context<'_>, user_id: i64) -> Result<bool> {
        let me = admin(ctx).await?;
        let user = crate::auth::sign_out_everywhere(state(ctx), &me, user_id).await.api()?;
        record(ctx, &me, Entry::new(Kind::SignedOutEverywhere).about(user.username));
        Ok(true)
    }
}
