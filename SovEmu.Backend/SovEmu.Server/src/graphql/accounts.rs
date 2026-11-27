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
    /// The environment declares it (`SovEmu__Authentication__AdminUsername`, `SovEmu__Users__<n>__…`) and sets
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

/// Which client holds a sign-in.
#[derive(Enum, Clone, Copy, PartialEq, Eq)]
pub(super) enum SignInClient {
    /// The web client, in a browser.
    Web,
    /// The Android or iOS app (a phone, a tablet, a TV).
    App,
    Other,
}

/// A browser or app signed in as an account: it holds a refresh token, and can be signed out
/// on its own (`endSignIn`).
#[derive(SimpleObject)]
pub(super) struct SignInNode {
    pub(super) id: ID,
    pub(super) user_id: Option<i64>,
    /// The account's name; null if it has gone.
    pub(super) username: Option<String>,
    pub(super) client: SignInClient,
    /// When it signed in, as a Unix time in seconds.
    pub(super) signed_in_at: f64,
    /// When it last renewed its access: about when it was last in use (Unix seconds).
    pub(super) last_used_at: f64,
    /// When it lapses unless used again (Unix seconds); null for none.
    pub(super) expires_at: Option<f64>,
    /// What the app calls its device (`nameDevice`); null for a browser, or an app that has not
    /// said.
    pub(super) device: Option<String>,
    /// What its device says it can decode and show, from its latest `sessionOffer`; null until
    /// it has started a stream with one, or if what is kept is of another version.
    pub(super) capabilities: Option<crate::capabilities::ClientCapabilities>,
}

fn unix(t: std::time::SystemTime) -> f64 {
    t.duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64())
}

impl SignInNode {
    fn new(
        s: sovemu_identity::SignIn,
        username: Option<String>,
        kind: Option<sovemu_identity::ClientKind>,
    ) -> Self {
        let client = match kind {
            Some(sovemu_identity::ClientKind::Web) => SignInClient::Web,
            Some(sovemu_identity::ClientKind::Device | sovemu_identity::ClientKind::Mobile) => {
                SignInClient::App
            }
            None => SignInClient::Other,
        };
        Self {
            id: ID(s.id),
            user_id: s.user_id,
            username,
            client,
            signed_in_at: unix(s.signed_in_at),
            last_used_at: unix(s.last_used_at),
            expires_at: s.expires_at.map(unix),
            device: s.device,
            capabilities: s
                .capabilities
                .as_deref()
                .and_then(crate::capabilities::ClientCapabilities::from_json),
        }
    }
}

/// One account, as the admin's user page shows it.
#[derive(SimpleObject)]
pub(super) struct UserDetails {
    pub(super) user: Account,
    /// When it was created (UTC, `YYYY-MM-DD HH:MM:SS`).
    pub(super) joined_at: Option<String>,
    /// Every browser and app signed in as it, the most recently used first.
    pub(super) sign_ins: Vec<SignInNode>,
    /// Different games it has started, and starts in all.
    pub(super) games_played: i32,
    pub(super) starts: i32,
    /// Its latest game start (UTC, `YYYY-MM-DD HH:MM:SS`); null if none yet.
    pub(super) last_played_at: Option<String>,
    /// Its battery saves and states on disk: bytes (a Float: it can pass 2 GiB) and files.
    pub(super) save_bytes: f64,
    pub(super) save_files: i32,
}

/// Where and how to sign in at the address this request reached. Apps ask it first, signed out,
/// and use these URLs as given (they hard-code nothing but `/graphql`).
#[derive(SimpleObject)]
pub(super) struct BootstrapConfig {
    /// Whether anything can sign in at this address. False when it is not one of
    /// `SovEmu__Http__PublicUrl`'s; then the rest is null, and `address` says what to add.
    pub(super) sign_in_available: bool,
    /// The address the request reached, as `Host` named it.
    pub(super) address: String,
    /// The OpenID Connect issuer for this address (discovery at
    /// `<issuer>/.well-known/openid-configuration`).
    pub(super) issuer: Option<String>,
    /// The web client's id: the authorization code flow with PKCE.
    pub(super) web_client_id: Option<String>,
    /// The devices' id: the device grant (a code shown on the screen and approved at
    /// `verificationUri`), and the password fallback. A TV signs in as it, and so does an app
    /// signing in with a code or a password.
    pub(super) device_client_id: Option<String>,
    /// The phone and tablet apps' id for signing in through the system browser: the
    /// authorization code flow with PKCE, back to `mobileRedirectUri`.
    pub(super) mobile_client_id: Option<String>,
    /// Where the mobile client's sign-in comes back to, the apps' own scheme.
    pub(super) mobile_redirect_uri: Option<String>,
    /// The device client's id, by its old name.
    #[graphql(deprecation = "Use deviceClientId.")]
    pub(super) native_client_id: Option<String>,
    pub(super) authorization_endpoint: Option<String>,
    pub(super) token_endpoint: Option<String>,
    pub(super) revocation_endpoint: Option<String>,
    /// Where an app asks for a code to show (RFC 8628 device authorization).
    pub(super) device_authorization_endpoint: Option<String>,
    /// Where a person approves that code, in a signed-in browser.
    pub(super) verification_uri: Option<String>,
    /// The apps' password fallback: a form post of `client_id`, `username`, `password` and
    /// `scope`, answered like the token endpoint. Null when `SovEmu__Authentication__PasswordSignIn` is off.
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
    /// `RTCIceServer` objects for WebRTC: the STUN servers, then the built-in relay (TURN) with
    /// this player's own credentials, when it runs.
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
    /// SovEmu's licence (SPDX: `AGPL-3.0-or-later`).
    pub(super) licence: String,
    /// Where SovEmu's source is. A modified server must point this at its own.
    pub(super) source: String,
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
            device_client_id: config.as_ref().map(|c| c.device_client_id.clone()),
            mobile_client_id: config.as_ref().map(|c| c.mobile_client_id.clone()),
            mobile_redirect_uri: config.as_ref().map(|c| c.mobile_redirect_uri.clone()),
            native_client_id: config.as_ref().map(|c| c.device_client_id.clone()),
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
        let stamp = crate::about::stamp();
        Ok(BuildInfoNode {
            version: b.version.to_owned(),
            revision: stamp.revision.clone(),
            built: stamp.built.clone(),
            libretro_release: b.libretro_release.to_owned(),
            licence: b.licence.to_owned(),
            source: b.source.to_owned(),
        })
    }

    /// Who is signed in, and whether first-run setup is needed. Works signed out.
    async fn viewer(&self, ctx: &Context<'_>) -> Result<Viewer> {
        let setup_required =
            state(ctx).identity.setup_required().await.map_err(|e| gql(e.into()))?;
        Ok(Viewer { setup_required, user: viewer(ctx).await?.map(Account::from) })
    }

    async fn client_config(&self, ctx: &Context<'_>) -> Result<ClientConfig> {
        let user = user(ctx).await?;
        let state = state(ctx);
        let ice_servers = crate::relay::ice_servers(
            &state.config,
            state.relay.as_deref(),
            user.id,
            request_headers(ctx),
            std::time::SystemTime::now(),
        );
        Ok(ClientConfig { ice_servers: Json(ice_servers.into()) })
    }

    /// Admins: one account, with its sign-ins, what it has played and its saves.
    async fn user(&self, ctx: &Context<'_>, id: i64) -> Result<UserDetails> {
        admin(ctx).await?;
        let identity = &state(ctx).identity;
        let d = crate::user_details::details(state(ctx), id).await.api()?;
        let name = d.user.username.clone();
        Ok(UserDetails {
            sign_ins: d
                .sign_ins
                .into_iter()
                .map(|s| {
                    let kind = identity.client_kind(&s.client_id);
                    SignInNode::new(s, Some(name.clone()), kind)
                })
                .collect(),
            user: d.user.into(),
            joined_at: d.joined_at,
            games_played: d.played.games as i32,
            starts: d.played.starts as i32,
            last_played_at: d.played.last_played_at,
            save_bytes: d.saves.0 as f64,
            save_files: d.saves.1 as i32,
        })
    }

    /// Admins: every browser and app signed in, across accounts, the most recently used first.
    async fn sign_ins(&self, ctx: &Context<'_>) -> Result<Vec<SignInNode>> {
        admin(ctx).await?;
        let state = state(ctx);
        let sign_ins = state.identity.sign_ins(None).await.map_err(|e| gql(e.into()))?;
        let names = crate::api::usernames(state).await.api()?;
        Ok(sign_ins
            .into_iter()
            .map(|s| {
                let name = s.user_id.and_then(|id| names.get(&id).cloned());
                let kind = state.identity.client_kind(&s.client_id);
                SignInNode::new(s, name, kind)
            })
            .collect())
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

    /// Signs a TV in as you: the code its *Sign in with your phone* screen shows, typed on a
    /// phone or tablet already signed in here (*Sign in a TV*), or on the web client's `/link/`
    /// page, where the TV sends people. `NOT_FOUND` when no TV is waiting with that code;
    /// `TOO_MANY_REQUESTS` after several wrong ones.
    async fn approve_device_code(&self, ctx: &Context<'_>, user_code: String) -> Result<bool> {
        let me = user(ctx).await?;
        let headers = &ctx.data_unchecked::<Auth>().headers;
        state(ctx)
            .identity
            .approve_device(headers, &me, &user_code)
            .await
            .map_err(|e| gql(e.into()))?;
        Ok(true)
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

    /// Admins: signs one browser or app out (`id` from `signIns` or `user`); it must sign in
    /// again. The account's other sign-ins stand.
    async fn end_sign_in(&self, ctx: &Context<'_>, id: ID) -> Result<bool> {
        let me = admin(ctx).await?;
        let state = state(ctx);
        // Whose it was, for the log: the sign-in goes with the ending.
        let whose = state.identity.sign_ins(None).await.ok().and_then(|all| {
            all.into_iter().find(|s| s.id == id.as_str()).map(|s| (s.user_id, s.client_id))
        });
        state.identity.end_sign_in(&me, &id).await.map_err(|e| gql(e.into()))?;
        let (user_id, client) = whose.unwrap_or((None, String::new()));
        let name = match user_id {
            Some(uid) => state.identity.user(uid).await.ok().flatten().map(|u| u.username),
            None => None,
        };
        let entry = Entry::new(Kind::SignInEnded)
            .about(name.unwrap_or_else(|| "a deleted account".to_owned()))
            .detail(client);
        record(ctx, &me, entry);
        Ok(true)
    }

    /// Names this sign-in's device, as the app describes itself ("SovEmu for Android 1.4.0 ·
    /// Mi TV Stick"), for the admin's lists of sign-ins. An app calls it once signed in, and
    /// again when its version changes. `BAD_REQUEST` for an empty name or one over 80
    /// characters.
    async fn name_device(&self, ctx: &Context<'_>, name: String) -> Result<bool> {
        user(ctx).await?;
        state(ctx)
            .identity
            .name_device(request_headers(ctx), &name)
            .await
            .map_err(|e| gql(e.into()))?;
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
