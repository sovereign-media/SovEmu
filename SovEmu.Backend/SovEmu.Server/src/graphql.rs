//! The API: GraphQL at `POST /graphql` (GraphiQL at `GET /graphql`), the only API the clients
//! use. Binary files (artwork, videos, save screenshots) are plain HTTP under `/media/`, whose
//! URLs the API hands out; `/health` stays plain HTTP for probes.
//!
//! Resolvers call the same functions the rest of the server does (`crate::api`, `crate::auth`,
//! …). Requests are signed in with `Authorization: Bearer <access token>`, from the server's
//! OpenID Connect provider (`crate::oidc`); cookies sign nothing in here. Errors carry a `code` in their extensions (`UNAUTHENTICATED`, `FORBIDDEN`,
//! `NOT_FOUND`, `BAD_REQUEST`, `CONFLICT`, `GONE`, `TOO_MANY_REQUESTS`, `UNAVAILABLE`, …).
//!
//! Numbers: GraphQL's `Int` is 32-bit, so byte counts and Unix times are `Float` (exact to
//! 2^53); ids and counts are `Int`. The schema is checked in as `schema.graphql` (see
//! [`tests`]), for the web, Android and iOS clients.

use std::collections::BTreeMap;

use async_graphql::extensions::{Extension, ExtensionContext, ExtensionFactory, NextParseQuery};
use async_graphql::http::GraphiQLSource;
use async_graphql::parser::types::ExecutableDocument;
use async_graphql::{
    Context, EmptySubscription, Enum, ErrorExtensions, ID, InputObject, Json, Object, Schema,
    ServerResult, SimpleObject, Variables,
};
use async_graphql_axum::{GraphQLRequest, GraphQLResponse};
use axum::extract::State;
use axum::http::header::SET_COOKIE;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse};
use tokio::sync::OnceCell;

use crate::AppState;
use crate::auth::{Credentials, NewUser, User};
use crate::error::ApiError;

pub(crate) type ApiSchema = Schema<Query, Mutation, EmptySubscription>;

/// The schema, without per-request data. Queries deeper than 12 levels or costlier than 2000
/// fields are refused.
pub(crate) fn schema() -> ApiSchema {
    Schema::build(Query, Mutation, EmptySubscription)
        .extension(OperationType)
        .limit_depth(12)
        .limit_complexity(2000)
        .finish()
}

/// Per request: its headers (for the bearer access token), and the user they name, looked up
/// once.
struct Auth {
    headers: HeaderMap,
    user: OnceCell<Option<User>>,
}

pub(crate) async fn handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    request: GraphQLRequest,
) -> GraphQLResponse {
    let auth = Auth { headers, user: OnceCell::new() };
    let request = request.into_inner();
    // The request span (telemetry::trace_request) names the operation, for logs and traces.
    if let Some(name) = &request.operation_name {
        tracing::Span::current().record("graphql.operation.name", name.as_str());
    }
    let request = request.data(state.clone()).data(auth);
    let response = state.schema.execute(request).await;
    log_errors(&response.errors);
    response.into()
}

/// Names the operation's type (`query`, `mutation`) on the request span, once the query is
/// parsed: the span's other GraphQL attribute, after OpenTelemetry's `graphql.operation.type`.
struct OperationType;

impl ExtensionFactory for OperationType {
    fn create(&self) -> std::sync::Arc<dyn Extension> {
        std::sync::Arc::new(OperationType)
    }
}

#[async_graphql::async_trait::async_trait]
impl Extension for OperationType {
    async fn parse_query(
        &self,
        ctx: &ExtensionContext<'_>,
        query: &str,
        variables: &Variables,
        next: NextParseQuery<'_>,
    ) -> ServerResult<ExecutableDocument> {
        let document = next.run(ctx, query, variables).await?;
        let mut types = document.operations.iter().map(|(_, op)| op.node.ty);
        if let Some(first) = types.next()
            && types.all(|ty| ty == first)
        {
            tracing::Span::current().record("graphql.operation.type", first.to_string());
        }
        Ok(document)
    }
}

/// How a GraphQL error is logged: a 200 response can still carry errors, so without this a
/// failed menu action leaves nothing in the log.
#[derive(Debug, PartialEq, Eq)]
enum ErrorKind {
    /// The query did not parse or validate (no `code`, no `path`): usually a client out of step
    /// with the schema.
    InvalidQuery,
    /// The caller's own doing, or routine (an expired token, an id that is gone): debug.
    Routine(&'static str),
    /// Worth seeing at the default level, but the server is fine (a refused action): info.
    Refused(&'static str),
    /// The server's fault, or a service it relies on: warn, and the request span is an error.
    Failed(&'static str),
}

fn error_kind(code: Option<&str>, has_path: bool) -> ErrorKind {
    match code {
        None if !has_path => ErrorKind::InvalidQuery,
        Some("UNAUTHENTICATED") => ErrorKind::Routine("UNAUTHENTICATED"),
        Some("FORBIDDEN") => ErrorKind::Routine("FORBIDDEN"),
        Some("NOT_FOUND") => ErrorKind::Routine("NOT_FOUND"),
        Some("BAD_REQUEST") => ErrorKind::Refused("BAD_REQUEST"),
        Some("CONFLICT") => ErrorKind::Refused("CONFLICT"),
        Some("GONE") => ErrorKind::Refused("GONE"),
        Some("TOO_MANY_REQUESTS") => ErrorKind::Refused("TOO_MANY_REQUESTS"),
        Some("BAD_GATEWAY") => ErrorKind::Failed("BAD_GATEWAY"),
        Some("UNAVAILABLE") => ErrorKind::Failed("UNAVAILABLE"),
        // INTERNAL, or an error that never went through `gql` (a panic, a stray `?`).
        _ => ErrorKind::Failed("INTERNAL"),
    }
}

/// A result path as text: `game.saves.0.screenshot`.
fn error_path(path: &[async_graphql::PathSegment]) -> String {
    let segments = path.iter().map(|segment| match segment {
        async_graphql::PathSegment::Field(name) => name.clone(),
        async_graphql::PathSegment::Index(i) => i.to_string(),
    });
    segments.collect::<Vec<_>>().join(".")
}

/// Logs each error in a response inside the request span (which names the operation and the
/// user), with its code as `error.type` and the result path that failed.
fn log_errors(errors: &[async_graphql::ServerError]) {
    let span = tracing::Span::current();
    for error in errors {
        let code = error.extensions.as_ref().and_then(|ext| match ext.get("code") {
            Some(async_graphql::Value::String(code)) => Some(code.as_str()),
            _ => None,
        });
        let path = error_path(&error.path);
        let message = error.message.as_str();
        match error_kind(code, !error.path.is_empty()) {
            ErrorKind::InvalidQuery => {
                span.record("error.type", "GRAPHQL_VALIDATION_FAILED");
                tracing::warn!(error = message, "graphql: invalid query");
            }
            ErrorKind::Routine(code) => {
                tracing::debug!(error.type = code, graphql.error.path = path, error = message, "graphql error");
            }
            ErrorKind::Refused(code) => {
                span.record("error.type", code);
                tracing::info!(error.type = code, graphql.error.path = path, error = message, "graphql error");
            }
            ErrorKind::Failed(code) => {
                span.record("error.type", code);
                span.record("otel.status_code", "ERROR");
                tracing::warn!(error.type = code, graphql.error.path = path, error = message, "graphql error");
            }
        }
    }
}

/// GraphiQL, for trying queries in a browser. Signed-in queries need an access token in the
/// headers pane: `{"Authorization": "Bearer …"}`.
pub(crate) async fn graphiql() -> impl IntoResponse {
    Html(GraphiQLSource::build().endpoint("/graphql").finish())
}

// --- Context helpers -------------------------------------------------------------------------

type Result<T> = async_graphql::Result<T>;

/// An [`ApiError`] as a GraphQL error, with its kind as `extensions.code`.
fn gql(e: ApiError) -> async_graphql::Error {
    let code = match e.status {
        StatusCode::BAD_REQUEST => "BAD_REQUEST",
        StatusCode::UNAUTHORIZED => "UNAUTHENTICATED",
        StatusCode::FORBIDDEN => "FORBIDDEN",
        StatusCode::NOT_FOUND => "NOT_FOUND",
        StatusCode::CONFLICT => "CONFLICT",
        StatusCode::GONE => "GONE",
        StatusCode::TOO_MANY_REQUESTS => "TOO_MANY_REQUESTS",
        StatusCode::BAD_GATEWAY => "BAD_GATEWAY",
        StatusCode::SERVICE_UNAVAILABLE => "UNAVAILABLE",
        _ => "INTERNAL",
    };
    async_graphql::Error::new(e.message).extend_with(|_, ext| ext.set("code", code))
}

trait Api<T> {
    fn api(self) -> Result<T>;
}

impl<T> Api<T> for crate::error::ApiResult<T> {
    fn api(self) -> Result<T> {
        self.map_err(gql)
    }
}

fn state<'a>(ctx: &Context<'a>) -> &'a AppState {
    ctx.data_unchecked::<AppState>()
}

async fn viewer(ctx: &Context<'_>) -> Result<Option<User>> {
    let auth = ctx.data_unchecked::<Auth>();
    let user = auth
        .user
        .get_or_try_init(|| async {
            let user = state(ctx).identity.user_for_request(&auth.headers).await?;
            // Every line logged for the rest of the request then names whose it was.
            if let Some(user) = &user {
                tracing::Span::current().record("user.id", user.id);
            }
            Ok::<_, sovemu_identity::Error>(user)
        })
        .await
        .map_err(|e| gql(e.into()))?;
    Ok(user.clone())
}

/// The signed-in user, or `UNAUTHENTICATED`.
async fn user(ctx: &Context<'_>) -> Result<User> {
    viewer(ctx).await?.ok_or_else(|| gql(ApiError::unauthorized()))
}

/// The signed-in user, who must be an admin, or `FORBIDDEN`.
async fn admin(ctx: &Context<'_>) -> Result<User> {
    let user = user(ctx).await?;
    if user.is_admin() { Ok(user) } else { Err(gql(ApiError::forbidden())) }
}

fn set_cookie(ctx: &Context<'_>, cookie: HeaderValue) {
    ctx.insert_http_header(SET_COOKIE, cookie);
}

// --- Types -----------------------------------------------------------------------------------

/// What an account may do.
#[derive(Enum, Clone, Copy, PartialEq, Eq)]
#[graphql(remote = "sovemu_identity::Role", name = "Role")]
enum RoleKind {
    /// Plays games: browse the library, play, keep saves and settings of their own.
    Viewer,
    /// Also runs the server: users, the library, emulator settings.
    Admin,
}

/// An account.
#[derive(SimpleObject)]
#[graphql(name = "User")]
struct Account {
    id: i64,
    username: String,
    role: RoleKind,
    /// Whether the role is `ADMIN`.
    is_admin: bool,
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
struct BootstrapConfig {
    /// Whether anything can sign in at this address. False when it is not one of
    /// `SOVEMU_PUBLIC_URL`'s; then the rest is null, and `address` says what to add.
    sign_in_available: bool,
    /// The address the request reached, as `Host` named it.
    address: String,
    /// The OpenID Connect issuer for this address (discovery at
    /// `<issuer>/.well-known/openid-configuration`).
    issuer: Option<String>,
    /// The web client's id: the authorization code flow with PKCE.
    web_client_id: Option<String>,
    /// The apps' id: the device grant, and the password fallback.
    native_client_id: Option<String>,
    authorization_endpoint: Option<String>,
    token_endpoint: Option<String>,
    revocation_endpoint: Option<String>,
    /// Where an app asks for a code to show (RFC 8628 device authorization).
    device_authorization_endpoint: Option<String>,
    /// Where a person approves that code, in a signed-in browser.
    verification_uri: Option<String>,
    /// The apps' password fallback: a form post of `client_id`, `username`, `password` and
    /// `scope`, answered like the token endpoint. Null when `SOVEMU_PASSWORD_SIGN_IN` is off.
    password_endpoint: Option<String>,
}

#[derive(SimpleObject)]
struct Viewer {
    /// No account exists yet: the client should offer first-run setup.
    setup_required: bool,
    /// The signed-in user, if any.
    user: Option<Account>,
}

#[derive(SimpleObject)]
struct ClientConfig {
    /// `RTCIceServer` objects (STUN/TURN) for WebRTC, from `SOVEMU_ICE_SERVERS`.
    ice_servers: Json<serde_json::Value>,
}

/// Where a 3D system renders on this server.
/// What went wrong on a client (`reportClientEvents`).
#[derive(Enum, Clone, Copy, PartialEq, Eq)]
enum ClientEventKind {
    /// WebRTC could not connect.
    ConnectionFailed,
    /// A connected stream dropped.
    ConnectionLost,
    /// No new picture for a while during play.
    StreamStalled,
    /// A controller the client has no layout for.
    ControllerUnknown,
    /// A screen or the app failed.
    ScreenError,
    Other,
}

/// One thing that went wrong on a client.
#[derive(InputObject)]
struct ClientEventInput {
    kind: ClientEventKind,
    /// What happened, in a line (kept to 500 characters).
    message: String,
    /// The game session it happened in, if any.
    session_id: Option<ID>,
    /// When, in seconds since the Unix epoch on the client's clock.
    at: Option<f64>,
}

impl From<ClientEventInput> for crate::client_events::Event {
    fn from(e: ClientEventInput) -> Self {
        use crate::client_events::Kind;
        let kind = match e.kind {
            ClientEventKind::ConnectionFailed => Kind::ConnectionFailed,
            ClientEventKind::ConnectionLost => Kind::ConnectionLost,
            ClientEventKind::StreamStalled => Kind::StreamStalled,
            ClientEventKind::ControllerUnknown => Kind::ControllerUnknown,
            ClientEventKind::ScreenError => Kind::ScreenError,
            ClientEventKind::Other => Kind::Other,
        };
        Self { kind, message: e.message, session_id: e.session_id.map(|id| id.0), at: e.at }
    }
}

#[derive(Enum, Clone, Copy, PartialEq, Eq)]
enum Rendering {
    Gpu,
    Software,
    Off,
}

impl From<sovemu_sessions::gpu::Mode> for Rendering {
    fn from(m: sovemu_sessions::gpu::Mode) -> Self {
        match m {
            sovemu_sessions::gpu::Mode::Gpu => Self::Gpu,
            sovemu_sessions::gpu::Mode::Software => Self::Software,
            sovemu_sessions::gpu::Mode::Off => Self::Off,
        }
    }
}

/// A console (or arcade platform) SovEmu plays.
struct SystemNode(crate::api::SystemSummary);

#[Object(name = "System")]
impl SystemNode {
    async fn id(&self) -> ID {
        ID::from(self.0.system.id)
    }
    /// What the system calls its buttons, and a diagram of its pad (docs/CONTROLLERS.md).
    async fn controls(&self) -> crate::controls::SystemControls {
        crate::controls::system_controls(&self.0.system)
    }
    async fn name(&self) -> &str {
        self.0.system.name
    }
    /// A short label for tight spaces ("SNES").
    async fn short(&self) -> &str {
        self.0.system.short
    }
    async fn manufacturer(&self) -> &str {
        self.0.system.manufacturer
    }
    async fn year(&self) -> i32 {
        self.0.system.year.into()
    }
    /// The UI accent colour (CSS).
    async fn color(&self) -> &str {
        self.0.system.color
    }
    async fn game_count(&self) -> i64 {
        self.0.game_count
    }
    /// A core that plays it is installed (and, for a 3D system, there is a renderer).
    async fn playable(&self) -> bool {
        self.0.playable
    }
    /// A 3D system: its cores render with OpenGL and read analog sticks.
    async fn needs_gpu(&self) -> bool {
        self.0.system.needs_gpu
    }
    /// For a 3D system, where it renders here.
    async fn rendering(&self) -> Option<Rendering> {
        self.0.rendering.map(Into::into)
    }
    /// A BIOS file it cannot start games without is missing. Cheap: no checksums (see `bios`).
    async fn bios_missing(&self, ctx: &Context<'_>) -> bool {
        let (dir, games) = bios_places(state(ctx), self.0.system);
        let id = self.0.system.id;
        tokio::task::spawn_blocking(move || sovemu_library::bios::missing_required(&dir, games, id))
            .await
            .unwrap_or(false)
    }
    /// Its BIOS files, checked by name and MD5; null for a system that uses none.
    async fn bios(&self, ctx: &Context<'_>) -> Option<SystemBios> {
        let (dir, games) = bios_places(state(ctx), self.0.system);
        let id = self.0.system.id;
        let report =
            tokio::task::spawn_blocking(move || sovemu_library::bios::check(&dir, &games(), id))
                .await
                .ok()??;
        Some(SystemBios::from(report, id))
    }
    /// Its games, by title.
    async fn games(&self, ctx: &Context<'_>) -> Result<Vec<GameNode>> {
        let user = user(ctx).await?;
        let games = crate::api::list_games(state(ctx), &user, self.0.system.id).await.api()?;
        Ok(games.into_iter().map(GameNode).collect())
    }
}

/// The BIOS folder, and how to find the system's game folders (where FBNeo also looks). In
/// cache mode the core plays a copy of the game alone, so only the BIOS folder counts.
fn bios_places(
    state: &AppState,
    system: sovemu_library::systems::System,
) -> (std::path::PathBuf, impl Fn() -> Vec<std::path::PathBuf> + Send + 'static) {
    let roms = (state.config.library_mode != crate::LibraryMode::Cache)
        .then(|| state.config.roms_dir.clone());
    let games = move || {
        roms.as_deref().map(|r| sovemu_library::bios::game_dirs(r, &system)).unwrap_or_default()
    };
    (state.config.bios_dir(), games)
}

/// How much a system depends on a BIOS file.
#[derive(Enum, Clone, Copy, PartialEq, Eq)]
enum BiosImportance {
    /// Games will not start without it.
    Required,
    /// The core has a built-in replacement that plays most games.
    Recommended,
    /// A boot logo, an add-on.
    Optional,
}

/// One BIOS file.
#[derive(Enum, Clone, Copy, PartialEq, Eq)]
enum BiosFileState {
    /// Where the core looks, and a known good dump.
    Ok,
    /// Where the core looks, but not a dump the list knows: a bad dump, or a revision it lacks.
    Unverified,
    /// A known dump, or the right name in the wrong case, but not where the core looks.
    Misplaced,
    Missing,
}

/// A system's BIOS files as a whole.
#[derive(Enum, Clone, Copy, PartialEq, Eq)]
enum BiosState {
    /// Everything required and recommended is there and verified.
    Ready,
    /// Playable, but a file is unverified or misplaced.
    Warning,
    /// Playable, without a recommended file.
    Incomplete,
    /// A required file is missing: games will not start.
    Missing,
}

#[derive(SimpleObject)]
struct SystemBios {
    state: BiosState,
    /// The system's folder, relative to the server's BIOS folder (`SOVEMU_BIOS_DIR`).
    folder: String,
    groups: Vec<BiosGroup>,
}

/// A BIOS the system uses: one file, or any one of several.
#[derive(SimpleObject)]
struct BiosGroup {
    label: String,
    importance: BiosImportance,
    /// What it does, and what happens without it.
    help: String,
    /// Any one of `files` is enough.
    any_one: bool,
    satisfied: bool,
    files: Vec<BiosFile>,
}

#[derive(SimpleObject)]
struct BiosFile {
    /// Its name in the system's folder.
    name: String,
    label: String,
    state: BiosFileState,
    /// Where it was found, relative to the BIOS folder (`roms/<folder>/…` beside the games).
    found_at: Option<String>,
    /// Of the file found.
    md5: Option<String>,
    /// Bytes expected.
    size: f64,
}

impl SystemBios {
    fn from(r: sovemu_library::bios::Report, system: &str) -> Self {
        use sovemu_library::bios::{FileState, Importance, Kind, State};
        Self {
            state: match r.state {
                State::Ready => BiosState::Ready,
                State::Warning => BiosState::Warning,
                State::Incomplete => BiosState::Incomplete,
                State::Missing => BiosState::Missing,
            },
            folder: system.to_owned(),
            groups: r
                .groups
                .into_iter()
                .map(|g| BiosGroup {
                    label: g.group.label.to_owned(),
                    importance: match g.group.importance {
                        Importance::Required => BiosImportance::Required,
                        Importance::Recommended => BiosImportance::Recommended,
                        Importance::Optional => BiosImportance::Optional,
                    },
                    help: g.group.help.to_owned(),
                    any_one: match &g.group.kind {
                        Kind::AnyOf(files) => files.len() > 1,
                        Kind::Dumps { .. } => true,
                    },
                    satisfied: g.satisfied,
                    files: g
                        .files
                        .into_iter()
                        .map(|f| BiosFile {
                            name: f.name,
                            label: f.label,
                            state: match f.state {
                                FileState::Ok => BiosFileState::Ok,
                                FileState::Unverified => BiosFileState::Unverified,
                                FileState::Misplaced => BiosFileState::Misplaced,
                                FileState::Missing => BiosFileState::Missing,
                            },
                            found_at: f.found_at,
                            md5: f.md5,
                            size: f.size as f64,
                        })
                        .collect(),
                })
                .collect(),
        }
    }
}

/// Artwork and video kinds a game can have.
#[derive(Enum, Clone, Copy, PartialEq, Eq)]
enum MediaKind {
    /// A screenshot or composite image.
    Image,
    /// Box art.
    Thumb,
    /// The title's logo.
    Marquee,
    /// A short video snap.
    Video,
}

impl MediaKind {
    fn name(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Thumb => "thumb",
            Self::Marquee => "marquee",
            Self::Video => "video",
        }
    }
    fn parse(s: &str) -> Option<Self> {
        [Self::Image, Self::Thumb, Self::Marquee, Self::Video].into_iter().find(|k| k.name() == s)
    }
}

/// A game in the library, with this user's play history and saves.
struct GameNode(crate::api::Game);

#[Object(name = "Game")]
impl GameNode {
    async fn id(&self) -> i64 {
        self.0.id
    }
    /// The system's id.
    async fn system(&self) -> &str {
        &self.0.system
    }
    async fn title(&self) -> &str {
        &self.0.title
    }
    /// The title from the file name, when game information supplied a better `title`.
    async fn file_title(&self) -> Option<&str> {
        self.0.file_title.as_deref()
    }
    /// No-Intro tags from the file name: "USA", "Rev 1" …
    async fn tags(&self) -> Vec<String> {
        self.0.tags.split(',').map(str::trim).filter(|t| !t.is_empty()).map(str::to_owned).collect()
    }
    /// The file, relative to the library.
    async fn path(&self) -> &str {
        &self.0.path
    }
    /// Bytes.
    async fn size(&self) -> f64 {
        self.0.size as f64
    }
    /// When this user last played it (UTC, `YYYY-MM-DD HH:MM:SS`).
    async fn last_played_at(&self) -> Option<&str> {
        self.0.last_played_at.as_deref()
    }
    async fn play_count(&self) -> i64 {
        self.0.play_count.unwrap_or(0)
    }
    /// What tells it apart from games of the same title on its system: the tags they do not
    /// share ("Translation", "Rev 1"), else its file's own title. Null when its title is its own.
    async fn release(&self) -> Option<&str> {
        self.0.release.as_deref()
    }
    /// What its several files are (discs, disks, tapes, sides or parts), shown with its count
    /// (`discs`). Null for a game in one file.
    async fn part_kind(&self) -> Option<PartKind> {
        match self.0.parts.as_deref()? {
            "disc" => Some(PartKind::Disc),
            "disk" => Some(PartKind::Disk),
            "tape" => Some(PartKind::Tape),
            "side" => Some(PartKind::Side),
            "part" => Some(PartKind::Part),
            _ => None,
        }
    }
    /// The soft patch this user plays it with (a translation, a fix, a hack), by name: their
    /// choice, else the patch beside it with its name. Null: the original.
    async fn patch(&self) -> Option<&str> {
        self.0.patch.as_deref()
    }
    /// The soft patches found with it (IPS, UPS or BPS, beside it with its name or in a folder
    /// named as it), the default first. Choose one with `setPatch`.
    async fn patches(&self, ctx: &Context<'_>) -> Result<Vec<PatchNode>> {
        Ok(crate::patches::list(state(ctx), self.0.id)
            .await
            .api()?
            .into_iter()
            .map(PatchNode)
            .collect())
    }
    /// The decoration (bezel) to draw around its picture: the set chosen for its system
    /// (`SOVEMU_DECORATION`, `SOVEMU_SYSTEMS__<id>__DECORATION`), its own image there, else its
    /// system's, else the set's default. Null when the set is `none` or has nothing for it.
    async fn decoration(&self, ctx: &Context<'_>) -> Option<DecorationNode> {
        let state = state(ctx);
        let set = state.config.decoration_for(&self.0.system);
        if set == sovemu_library::decorations::NONE {
            return None;
        }
        let stem = std::path::Path::new(&self.0.path).file_stem()?.to_string_lossy().into_owned();
        state.decorations().for_game(set, &self.0.system, &stem).map(DecorationNode)
    }
    /// One of this user's favourites.
    async fn favourite(&self) -> bool {
        self.0.favourite
    }
    /// The ids of this user's collections that hold it.
    async fn collection_ids(&self, ctx: &Context<'_>) -> Result<Vec<i64>> {
        let user = user(ctx).await?;
        crate::collections::of_game(state(ctx), user.id, self.0.id).await.api()
    }
    /// How many discs (1 for most games): a multi-disc game is one entry, its discs changed in play.
    async fn discs(&self) -> i32 {
        self.0.discs as i32
    }
    /// The artwork and video it has.
    async fn media(&self) -> Vec<MediaKind> {
        self.0.media.split(',').filter_map(MediaKind::parse).collect()
    }
    /// Where to load a kind of artwork from, if the game has it: a URL signed for this user
    /// (valid `SOVEMU_ASSET_URL_HOURS`; ask again for a fresh one).
    async fn media_url(&self, ctx: &Context<'_>, kind: MediaKind) -> Result<Option<String>> {
        if !self.0.media.split(',').any(|k| k == kind.name()) {
            return Ok(None);
        }
        let user = user(ctx).await?;
        Ok(Some(
            state(ctx).assets.sign(user.id, &format!("/media/games/{}/{}", self.0.id, kind.name())),
        ))
    }
    async fn description(&self) -> Option<&str> {
        self.0.description.as_deref()
    }
    /// `YYYY-MM-DD`, `YYYY-MM` or `YYYY`.
    async fn release_date(&self) -> Option<&str> {
        self.0.release_date.as_deref()
    }
    async fn developer(&self) -> Option<&str> {
        self.0.developer.as_deref()
    }
    async fn publisher(&self) -> Option<&str> {
        self.0.publisher.as_deref()
    }
    async fn genre(&self) -> Option<&str> {
        self.0.genre.as_deref()
    }
    /// As the game information says it: "1", "1-2", "1 to 4" …
    async fn players(&self) -> Option<&str> {
        self.0.players.as_deref()
    }
    /// 0 to 1.
    async fn rating(&self) -> Option<f64> {
        self.0.rating
    }
    /// This user's save states (slot 0 is the auto save, written when a session ends).
    async fn saves(&self, ctx: &Context<'_>) -> Result<Vec<SaveSlotNode>> {
        let user = user(ctx).await?;
        Ok(crate::api::slots(state(ctx), user.id, &self.0)
            .into_iter()
            .map(|s| SaveSlotNode { game_id: self.0.id, slot: s })
            .collect())
    }
    /// The auto save, if there is one: where this user left off.
    async fn auto_save(&self, ctx: &Context<'_>) -> Result<Option<SaveSlotNode>> {
        let user = user(ctx).await?;
        Ok(crate::api::slots(state(ctx), user.id, &self.0)
            .into_iter()
            .find(|s| s.slot == 0)
            .map(|s| SaveSlotNode { game_id: self.0.id, slot: s }))
    }
    /// This user's cheats for it.
    async fn cheats(&self, ctx: &Context<'_>) -> Result<Vec<crate::cheats::Cheat>> {
        let user = user(ctx).await?;
        crate::cheats::list(state(ctx), &user, self.0.id).await.api()
    }
}

/// A save state.
struct SaveSlotNode {
    game_id: i64,
    slot: crate::api::SaveSlot,
}

#[Object(name = "SaveSlot")]
impl SaveSlotNode {
    /// 0 is the auto save; 1–9 are the player's.
    async fn slot(&self) -> i32 {
        self.slot.slot as i32
    }
    /// Unix seconds.
    async fn saved_at(&self) -> f64 {
        self.slot.saved_at as f64
    }
    /// The screen when it was saved, if there is one: a URL signed for this user that changes
    /// with each save.
    async fn screenshot_url(&self, ctx: &Context<'_>) -> Result<Option<String>> {
        if !self.slot.screenshot {
            return Ok(None);
        }
        let user = user(ctx).await?;
        let url = format!(
            "/media/games/{}/saves/{}?t={}",
            self.game_id, self.slot.slot, self.slot.saved_at
        );
        Ok(Some(state(ctx).assets.sign(user.id, &url)))
    }
}

/// The home screen's feed.
#[derive(SimpleObject)]
struct Home {
    /// The hero: games with artwork and a description first, a new pick each day.
    featured: Vec<GameNode>,
    /// This user's games, most recently played first.
    recent: Vec<GameNode>,
    /// The library's newest games.
    added: Vec<GameNode>,
    /// This user's favourites, by title.
    favourites: Vec<GameNode>,
}

/// A named list of this user's games.
struct CollectionNode(crate::collections::Collection);

/// What a multi-part game's files are.
#[derive(Enum, Copy, Clone, Eq, PartialEq)]
enum PartKind {
    /// Optical discs (CDs, DVDs, GD-ROMs).
    Disc,
    /// Floppy disks (the Famicom Disk System's included).
    Disk,
    /// Cassette tapes.
    Tape,
    /// The sides of one disk or tape.
    Side,
    /// Numbered files of any other kind.
    Part,
}

/// A soft patch for a game.
struct PatchNode(crate::patches::Patch);

#[Object(name = "Patch")]
impl PatchNode {
    async fn id(&self) -> i64 {
        self.0.id
    }
    /// From its file: "BPS patch" beside the game, else the file's name ("English v1.1").
    async fn name(&self) -> &str {
        &self.0.name
    }
    /// Applied unless the player chose otherwise (it is beside the game, with its name).
    async fn is_default(&self) -> bool {
        self.0.is_default
    }
}

/// Artwork drawn around a game's picture, in Batocera's decoration format: an image with a
/// transparent window, and where that window is in its pixels. Scale the image to the screen
/// and fit the picture, keeping its shape, in the scaled window.
struct DecorationNode(sovemu_library::decorations::Decoration);

#[Object(name = "Decoration")]
impl DecorationNode {
    /// The image (PNG), under `/media/decorations/`: a URL signed for this user.
    async fn url(&self, ctx: &Context<'_>) -> Result<String> {
        let user = user(ctx).await?;
        Ok(state(ctx).assets.sign(user.id, &self.0.url()))
    }
    /// Its size in pixels.
    async fn width(&self) -> u32 {
        self.0.window.width
    }
    async fn height(&self) -> u32 {
        self.0.window.height
    }
    /// The window's margins from the image's edges, in the image's pixels.
    async fn top(&self) -> u32 {
        self.0.window.top
    }
    async fn left(&self) -> u32 {
        self.0.window.left
    }
    async fn bottom(&self) -> u32 {
        self.0.window.bottom
    }
    async fn right(&self) -> u32 {
        self.0.window.right
    }
    /// The set it comes from (`sovemu` is the built-in one).
    async fn set(&self) -> &str {
        &self.0.set
    }
}

#[Object(name = "Collection")]
impl CollectionNode {
    async fn id(&self) -> i64 {
        self.0.id
    }
    async fn name(&self) -> &str {
        &self.0.name
    }
    async fn game_count(&self) -> i64 {
        self.0.game_count
    }
    /// Its games, most recently added first.
    async fn games(&self, ctx: &Context<'_>) -> Result<Vec<GameNode>> {
        let user = user(ctx).await?;
        Ok(crate::collections::games(state(ctx), &user, self.0.id)
            .await
            .api()?
            .into_iter()
            .map(GameNode)
            .collect())
    }
}

/// A running game: one worker streaming over WebRTC.
struct SessionNode(sovemu_sessions::workers::SessionInfo);

#[Object(name = "GameSession")]
impl SessionNode {
    async fn id(&self) -> ID {
        ID::from(&self.0.id)
    }
    async fn game_id(&self) -> i64 {
        self.0.game_id
    }
    async fn system(&self) -> &str {
        &self.0.system
    }
    async fn title(&self) -> &str {
        &self.0.title
    }
    /// The core's name.
    async fn core(&self) -> &str {
        &self.0.core
    }
    async fn width(&self) -> i32 {
        self.0.width as i32
    }
    async fn height(&self) -> i32 {
        self.0.height as i32
    }
    async fn fps(&self) -> f64 {
        self.0.fps
    }
    /// A client is streaming it.
    async fn connected(&self) -> bool {
        self.0.connected
    }
    /// How many discs the game has (1 for most games).
    async fn discs(&self) -> i32 {
        self.0.discs as i32
    }
    /// The disc in the tray, from 1.
    async fn disc(&self) -> i32 {
        self.0.disc as i32
    }
}

/// What a game start is doing.
#[derive(Enum, Clone, Copy, PartialEq, Eq)]
enum StartPhase {
    /// Saving and stopping the player's previous session.
    Stopping,
    /// Starting the emulator's process.
    Launching,
    /// Opening the emulator.
    Core,
    /// Reading the game.
    Content,
    Ready,
    Failed,
}

/// How far a game start has got, for a loading bar.
#[derive(SimpleObject)]
struct StartStatus {
    phase: StartPhase,
    /// Bytes read so far in this phase, where known.
    read: Option<f64>,
    /// Bytes this phase will read, when known.
    total: Option<f64>,
    /// Since the start began.
    elapsed_ms: f64,
    /// Since this phase began.
    phase_ms: f64,
    error: Option<String>,
}

impl From<sovemu_sessions::starts::StartStatus> for StartStatus {
    fn from(s: sovemu_sessions::starts::StartStatus) -> Self {
        use sovemu_sessions::starts::Phase;
        Self {
            phase: match s.phase {
                Phase::Stopping => StartPhase::Stopping,
                Phase::Launching => StartPhase::Launching,
                Phase::Core => StartPhase::Core,
                Phase::Content => StartPhase::Content,
                Phase::Ready => StartPhase::Ready,
                Phase::Failed => StartPhase::Failed,
            },
            read: s.read.map(|n| n as f64),
            total: s.total.map(|n| n as f64),
            elapsed_ms: s.elapsed_ms as f64,
            phase_ms: s.phase_ms as f64,
            error: s.error,
        }
    }
}

#[derive(SimpleObject)]
struct ScanReport {
    found: i64,
    added: i64,
    removed: i64,
}

/// How the library is read.
#[derive(Enum, Clone, Copy, PartialEq, Eq)]
enum LibraryMode {
    /// Straight from the library (local disk or a share on the LAN).
    Direct,
    /// From the library, keeping local copies of what is used.
    Cache,
    /// From a full local copy kept in sync.
    Mirror,
}

/// A mirror sync's progress.
#[derive(SimpleObject)]
struct MirrorSync {
    running: bool,
    files_total: i64,
    files_done: i64,
    /// Bytes.
    bytes_total: f64,
    /// Bytes.
    bytes_done: f64,
    removed: i64,
    failed: i64,
    /// Unix seconds when the last sync finished.
    finished_at: Option<f64>,
    error: Option<String>,
}

#[derive(SimpleObject)]
struct LibraryStatus {
    mode: LibraryMode,
    games: i64,
    /// `CACHE` mode: bytes on local disk.
    cache_bytes: Option<f64>,
    /// `MIRROR` mode: the current or last sync.
    mirror: Option<MirrorSync>,
}

/// A game-information lookup's progress.
#[derive(SimpleObject)]
struct MetadataStatus {
    running: bool,
    total: i64,
    done: i64,
    found: i64,
    not_found: i64,
    failed: i64,
    /// The configured sources.
    sources: Vec<String>,
    /// Why a source stopped early (bad credentials, quota), if one did.
    notes: Vec<String>,
    /// The box art downloads that follow a lookup, in the background.
    artwork: ArtworkStatus,
}

/// Box art being downloaded after a lookup, for the games that have no local thumb.
#[derive(SimpleObject)]
struct ArtworkStatus {
    running: bool,
    /// Downloads queued since the job started.
    total: i64,
    done: i64,
    downloaded: i64,
    failed: i64,
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

/// A device the startup GPU probe tried.
#[derive(SimpleObject)]
struct GpuDevice {
    /// The render node; none for the software renderer.
    node: Option<String>,
    driver: Option<String>,
    /// `hardware`, `virtual` (a hypervisor's adapter) or `software`.
    kind: String,
    vendor: Option<String>,
    renderer: Option<String>,
    version: Option<String>,
    works: bool,
    error: Option<String>,
}

/// Where 3D systems render, decided at startup.
#[derive(SimpleObject)]
struct GpuStatus {
    /// `SOVEMU_GPU` as configured.
    setting: String,
    mode: Rendering,
    /// What 3D games draw with.
    renderer: Option<String>,
    devices: Vec<GpuDevice>,
    /// Why the setting could not be honoured, or why nothing could be probed.
    note: Option<String>,
}

/// A background job (`jobs.rs`).
#[derive(Enum, Clone, Copy, PartialEq, Eq)]
enum JobName {
    /// Rescans the library.
    Scan,
    /// `MIRROR` mode: copies the source into the local mirror, then rescans it.
    MirrorSync,
    /// Looks up game information for games without it.
    Metadata,
    /// Database upkeep: expired sign-ins and old job history deleted.
    Maintenance,
}

impl From<crate::jobs::Job> for JobName {
    fn from(j: crate::jobs::Job) -> Self {
        use crate::jobs::Job;
        match j {
            Job::Scan => Self::Scan,
            Job::MirrorSync => Self::MirrorSync,
            Job::Metadata => Self::Metadata,
            Job::Maintenance => Self::Maintenance,
        }
    }
}

impl From<JobName> for crate::jobs::Job {
    fn from(j: JobName) -> Self {
        match j {
            JobName::Scan => Self::Scan,
            JobName::MirrorSync => Self::MirrorSync,
            JobName::Metadata => Self::Metadata,
            JobName::Maintenance => Self::Maintenance,
        }
    }
}

/// What started a job's run.
#[derive(Enum, Clone, Copy, PartialEq, Eq)]
enum JobTrigger {
    Startup,
    /// Its interval (`SOVEMU_*_INTERVAL_MINUTES`).
    Schedule,
    /// An admin.
    Admin,
}

impl From<crate::jobs::Trigger> for JobTrigger {
    fn from(t: crate::jobs::Trigger) -> Self {
        use crate::jobs::Trigger;
        match t {
            Trigger::Startup => Self::Startup,
            Trigger::Schedule => Self::Schedule,
            Trigger::Admin => Self::Admin,
        }
    }
}

/// How a job's run ended.
#[derive(Enum, Clone, Copy, PartialEq, Eq)]
enum JobOutcome {
    Running,
    Succeeded,
    Failed,
    /// Stopped by the server shutting down.
    Cancelled,
    /// The server stopped without finishing it.
    Interrupted,
}

impl From<crate::jobs::Outcome> for JobOutcome {
    fn from(o: crate::jobs::Outcome) -> Self {
        use crate::jobs::Outcome;
        match o {
            Outcome::Running => Self::Running,
            Outcome::Succeeded => Self::Succeeded,
            Outcome::Failed => Self::Failed,
            Outcome::Cancelled => Self::Cancelled,
            Outcome::Interrupted => Self::Interrupted,
        }
    }
}

/// One run of a background job.
#[derive(SimpleObject)]
struct JobRun {
    id: ID,
    job: JobName,
    trigger: JobTrigger,
    outcome: JobOutcome,
    /// Unix seconds.
    started_at: f64,
    /// Unix seconds; none while it runs.
    finished_at: Option<f64>,
    seconds: Option<f64>,
    /// What it did ("1,204 games: 3 added, 0 removed"), or why it failed.
    message: Option<String>,
}

impl From<crate::jobs::JobRun> for JobRun {
    fn from(r: crate::jobs::JobRun) -> Self {
        Self {
            id: ID(r.id.to_string()),
            job: r.job.into(),
            trigger: r.trigger.into(),
            outcome: r.outcome.into(),
            started_at: r.started_at as f64,
            finished_at: r.finished_at.map(|t| t as f64),
            seconds: r.duration_ms.map(|ms| ms as f64 / 1000.0),
            message: r.message,
        }
    }
}

/// A background job this server has, and its state now.
#[derive(SimpleObject)]
struct JobState {
    name: JobName,
    description: String,
    /// How often it runs on its own, in minutes; none if it does not (it follows another job,
    /// or its interval is 0).
    interval_minutes: Option<f64>,
    /// When it next runs on its own (Unix seconds); none while it runs or if it does not.
    next_run_at: Option<f64>,
    running: bool,
    /// What started the run under way.
    running_trigger: Option<JobTrigger>,
    /// When the run under way started (Unix seconds).
    running_since: Option<f64>,
    /// Its latest run (the one under way, if it is running).
    last_run: Option<JobRun>,
}

fn unix(t: std::time::SystemTime) -> f64 {
    t.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or_default()
}

/// Every job this server has, as [`JobState`]s.
async fn job_states(state: &AppState) -> Result<Vec<JobState>> {
    let mut last = state.jobs.last_runs().await.map_err(|e| gql(e.into()))?;
    Ok(state
        .jobs
        .info(state)
        .into_iter()
        .map(|i| JobState {
            name: i.job.into(),
            description: i.job.description().to_owned(),
            interval_minutes: i.every.map(|d| d.as_secs_f64() / 60.0),
            next_run_at: i.next_at.map(unix),
            running: i.running.is_some(),
            running_trigger: i.running.map(|(t, _)| t.into()),
            running_since: i.running.map(|(_, at)| unix(at)),
            last_run: last.remove(&i.job).map(Into::into),
        })
        .collect())
}

/// Where an emulator setting's value comes from.
#[derive(Enum, Clone, Copy, PartialEq, Eq)]
enum SettingSource {
    /// The emulator's own default.
    Core,
    /// SovEmu's default for the system.
    Sovemu,
    /// Changed by an admin.
    Admin,
    /// Set by `SOVEMU_SYSTEMS__<id>__OPTIONS__<name>`, which always wins (locked).
    Environment,
}

/// An emulator setting (a libretro core option).
#[derive(SimpleObject)]
struct EmulatorSetting {
    key: String,
    description: String,
    /// What it does, in the emulator's words (empty where it does not say).
    info: String,
    /// The key of the emulator's category for it.
    category: Option<String>,
    choices: Vec<String>,
    /// A display label per choice.
    labels: Vec<String>,
    /// The emulator's default.
    default: String,
    value: String,
    source: SettingSource,
}

/// How many frames a system's pictures run ahead of its games, hiding the games' own input lag:
/// a press shows up that many frames sooner. Each frame costs that many more emulated frames.
#[derive(SimpleObject)]
struct RunAheadSetting {
    /// 0: off.
    frames: i32,
    /// The most a system may run ahead.
    max: i32,
    /// `SOVEMU` when nothing sets it (1 frame on the systems that support it); `ENVIRONMENT` is
    /// `SOVEMU_SYSTEMS__<id>__RUN_AHEAD`, which always wins (locked).
    source: SettingSource,
    /// Whether the system can run ahead at all (the 2D systems).
    supported: bool,
}

/// A group of settings the emulator declares ("Video", "Audio" …).
#[derive(SimpleObject)]
struct SettingCategory {
    key: String,
    description: String,
    info: String,
}

/// A system's emulator settings.
#[derive(SimpleObject)]
struct EmulatorSettings {
    system: String,
    /// The emulator's name and version.
    core: String,
    settings: Vec<EmulatorSetting>,
    categories: Vec<SettingCategory>,
    /// The emulator lists its settings only with a game loaded, and none has run yet.
    needs_game: bool,
    /// The admin's changes as `SOVEMU_SYSTEMS__…` lines, to move into configuration.
    environment: Vec<String>,
    run_ahead: RunAheadSetting,
}

impl From<crate::settings::Source> for SettingSource {
    fn from(source: crate::settings::Source) -> Self {
        use crate::settings::Source;
        match source {
            Source::Core => SettingSource::Core,
            Source::Sovemu => SettingSource::Sovemu,
            Source::Admin => SettingSource::Admin,
            Source::Environment => SettingSource::Environment,
        }
    }
}

impl From<crate::settings::SystemSettings> for EmulatorSettings {
    fn from(s: crate::settings::SystemSettings) -> Self {
        Self {
            system: s.system,
            core: s.core,
            settings: s
                .settings
                .into_iter()
                .map(|x| EmulatorSetting {
                    key: x.key,
                    description: x.description,
                    info: x.info,
                    category: x.category,
                    choices: x.choices,
                    labels: x.labels,
                    default: x.default,
                    value: x.value,
                    source: x.source.into(),
                })
                .collect(),
            categories: s
                .categories
                .into_iter()
                .map(|c| SettingCategory { key: c.key, description: c.description, info: c.info })
                .collect(),
            needs_game: s.needs_game,
            environment: s.environment,
            run_ahead: RunAheadSetting {
                frames: s.run_ahead.frames as i32,
                max: sovemu_protocol::ipc::MAX_RUN_AHEAD as i32,
                source: s.run_ahead.source.into(),
                supported: s.run_ahead.supported,
            },
        }
    }
}

/// A change to one emulator setting.
#[derive(InputObject)]
struct SettingChange {
    key: String,
    /// The new value; null goes back to the default.
    value: Option<String>,
}

#[derive(InputObject)]
struct NewUserInput {
    username: String,
    password: String,
    #[graphql(default_with = "RoleKind::Viewer")]
    role: RoleKind,
}

// --- Queries ---------------------------------------------------------------------------------

/// The default controller mappings (admins).
#[derive(SimpleObject)]
struct ControllerDefaults {
    /// From the environment (locked) and from the admin page.
    mappings: Vec<crate::controllers::ControllerMapping>,
    /// The admin page's defaults as `SOVEMU_CONTROLLERS__<n>__…` lines.
    environment: Vec<String>,
}

#[derive(Debug)]
pub struct Query;

#[Object]
impl Query {
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

    /// The home screen's feed.
    async fn home(&self, ctx: &Context<'_>) -> Result<Home> {
        let user = user(ctx).await?;
        let home = crate::home::home(state(ctx), &user).await.api()?;
        Ok(Home {
            featured: home.featured.into_iter().map(GameNode).collect(),
            recent: home.recent.into_iter().map(GameNode).collect(),
            added: home.added.into_iter().map(GameNode).collect(),
            favourites: home.favourites.into_iter().map(GameNode).collect(),
        })
    }

    /// Games from every system matching every word of `text` (title, file title, developer,
    /// publisher or genre; case and accents ignored), best first: titles starting with it,
    /// then titles holding every word, then the rest. At most `limit` (default 48, up to 200).
    async fn search(
        &self,
        ctx: &Context<'_>,
        text: String,
        limit: Option<i32>,
    ) -> Result<Vec<GameNode>> {
        let user = user(ctx).await?;
        let limit = limit.map_or(crate::search::DEFAULT_LIMIT, |n| n.max(1) as u32);
        Ok(crate::search::search(state(ctx), &user, &text, limit)
            .await
            .api()?
            .into_iter()
            .map(GameNode)
            .collect())
    }

    /// This user's collections, by name.
    async fn collections(&self, ctx: &Context<'_>) -> Result<Vec<CollectionNode>> {
        let user = user(ctx).await?;
        Ok(crate::collections::list(state(ctx), &user)
            .await
            .api()?
            .into_iter()
            .map(CollectionNode)
            .collect())
    }

    /// Every system SovEmu offers (hidden ones left out), in SovEmu's order.
    async fn systems(&self, ctx: &Context<'_>) -> Result<Vec<SystemNode>> {
        user(ctx).await?;
        Ok(crate::api::list_systems(state(ctx)).await.api()?.into_iter().map(SystemNode).collect())
    }

    async fn system(&self, ctx: &Context<'_>, id: ID) -> Result<Option<SystemNode>> {
        user(ctx).await?;
        let all = crate::api::list_systems(state(ctx)).await.api()?;
        Ok(all.into_iter().find(|s| s.system.id == id.as_str()).map(SystemNode))
    }

    async fn game(&self, ctx: &Context<'_>, id: i64) -> Result<GameNode> {
        let user = user(ctx).await?;
        crate::api::get_game(state(ctx), &user, id).await.api().map(GameNode)
    }

    /// This user's running game, if any.
    async fn current_session(&self, ctx: &Context<'_>) -> Result<Option<SessionNode>> {
        let user = user(ctx).await?;
        Ok(crate::api::current_session(state(ctx), &user).map(SessionNode))
    }

    /// How far the start `startId` (given to `startSession`) has got. Poll it while
    /// `startSession` waits.
    async fn start_progress(&self, ctx: &Context<'_>, start_id: String) -> Result<StartStatus> {
        let user = user(ctx).await?;
        crate::api::start_progress(state(ctx), &user, &start_id).api().map(Into::into)
    }

    async fn library_status(&self, ctx: &Context<'_>) -> Result<LibraryStatus> {
        user(ctx).await?;
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

    async fn metadata_status(&self, ctx: &Context<'_>) -> Result<MetadataStatus> {
        user(ctx).await?;
        Ok(crate::admin::metadata_status(state(ctx)).into())
    }

    /// Admins: the accounts.
    async fn users(&self, ctx: &Context<'_>) -> Result<Vec<Account>> {
        admin(ctx).await?;
        let users = state(ctx).identity.users().await.map_err(|e| gql(e.into()))?;
        Ok(users.into_iter().map(Account::from).collect())
    }

    /// Admins: where 3D systems render, and every device the startup probe tried.
    async fn gpu(&self, ctx: &Context<'_>) -> Result<GpuStatus> {
        admin(ctx).await?;
        let g = &state(ctx).gpu;
        Ok(GpuStatus {
            setting: g.setting.clone(),
            mode: g.mode.into(),
            renderer: g.renderer.clone(),
            devices: g
                .devices
                .iter()
                .map(|d| GpuDevice {
                    node: d.node.clone(),
                    driver: d.driver.clone(),
                    kind: d.kind.clone(),
                    vendor: d.vendor.clone(),
                    renderer: d.renderer.clone(),
                    version: d.version.clone(),
                    works: d.works,
                    error: d.error.clone(),
                })
                .collect(),
            note: g.note.clone(),
        })
    }

    /// Admins: the background jobs (scans, syncs, lookups, upkeep), and how each last ran.
    async fn jobs(&self, ctx: &Context<'_>) -> Result<Vec<JobState>> {
        admin(ctx).await?;
        job_states(state(ctx)).await
    }

    /// Admins: the latest runs of background jobs, newest first: of one job, or of all.
    async fn job_runs(
        &self,
        ctx: &Context<'_>,
        job: Option<JobName>,
        #[graphql(default = 50, validator(minimum = 1, maximum = 500))] limit: i32,
    ) -> Result<Vec<JobRun>> {
        admin(ctx).await?;
        let runs = state(ctx)
            .jobs
            .runs(job.map(Into::into), limit as u32)
            .await
            .map_err(|e| gql(e.into()))?;
        Ok(runs.into_iter().map(Into::into).collect())
    }

    /// Admins: a system's emulator settings.
    async fn emulator_settings(&self, ctx: &Context<'_>, system: ID) -> Result<EmulatorSettings> {
        admin(ctx).await?;
        crate::settings::get_settings(state(ctx), &system).await.api().map(Into::into)
    }

    /// Diagrams of the player's controllers, by family (`xbox`, `playstation`, `nintendo`,
    /// `generic`), keyed by input (`south`, `l1` …).
    async fn controller_layouts(&self) -> Vec<crate::controls::PadLayout> {
        crate::controls::controller_layouts()
    }

    /// Admins: the default controller mappings every user starts from, and the admin's as
    /// environment lines.
    async fn controller_defaults(&self, ctx: &Context<'_>) -> Result<ControllerDefaults> {
        admin(ctx).await?;
        let state = state(ctx);
        Ok(ControllerDefaults {
            mappings: crate::controllers::defaults(state).await.api()?,
            environment: crate::controllers::environment_lines(state).await.api()?,
        })
    }

    /// The controller mappings in force for this user on a system (or, with none, on every
    /// system), one per controller model (docs/CONTROLLERS.md): their own for the system, their
    /// own for every system, the default for the system, the default for every system. A model
    /// missing here uses the clients' built-in defaults.
    async fn controller_mappings(
        &self,
        ctx: &Context<'_>,
        system: Option<ID>,
    ) -> Result<Vec<crate::controllers::ControllerMapping>> {
        let user = user(ctx).await?;
        crate::controllers::list(state(ctx), &user, system.as_ref().map(|s| s.as_str())).await.api()
    }
}

// --- Mutations -------------------------------------------------------------------------------

#[derive(Debug)]
pub struct Mutation;

#[Object]
impl Mutation {
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
        Ok(user.into())
    }

    /// Admins: rescans the library.
    async fn scan_library(&self, ctx: &Context<'_>) -> Result<ScanReport> {
        admin(ctx).await?;
        let r = crate::api::scan(state(ctx)).await.api()?;
        Ok(ScanReport { found: r.found as i64, added: r.added as i64, removed: r.removed as i64 })
    }

    /// Admins: starts a background job now, as its schedule would (`METADATA`: games without
    /// game information). `CONFLICT` if it is running; `BAD_REQUEST` if this server does not
    /// have it (`MIRROR_SYNC` outside `MIRROR` mode).
    async fn run_job(&self, ctx: &Context<'_>, job: JobName) -> Result<Vec<JobState>> {
        admin(ctx).await?;
        let state = state(ctx);
        crate::jobs::start_by_admin(state, job.into()).map_err(|e| gql(e.into()))?;
        job_states(state).await
    }

    /// Admins: starts a mirror sync in the background (`MIRROR` mode only).
    async fn sync_library(&self, ctx: &Context<'_>) -> Result<bool> {
        admin(ctx).await?;
        crate::admin::library_sync(state(ctx)).api()?;
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
        admin(ctx).await?;
        crate::admin::metadata_fetch(state(ctx), system.map(|s| s.to_string()), force)
            .api()
            .map(Into::into)
    }

    /// Admins: changes a system's emulator settings. Applies to the next game started.
    async fn change_emulator_settings(
        &self,
        ctx: &Context<'_>,
        system: ID,
        changes: Vec<SettingChange>,
    ) -> Result<EmulatorSettings> {
        admin(ctx).await?;
        let changes: BTreeMap<String, Option<String>> =
            changes.into_iter().map(|c| (c.key, c.value)).collect();
        crate::settings::put_settings(state(ctx), &system, changes).await.api().map(Into::into)
    }

    /// Signed-in users: what went wrong on this client (a connection that failed, a stalled
    /// picture, a screen that crashed), for the server's log beside the session's own lines.
    /// `client` names it and its version (`android 0.1.0 (MiTV-AFMU1, Android 11)`). At most
    /// 20 events a call and 60 a minute per user; the rest are dropped. Returns how many were
    /// logged (0 when `SOVEMU_CLIENT_EVENTS` is off). Nothing is stored.
    async fn report_client_events(
        &self,
        ctx: &Context<'_>,
        client: String,
        events: Vec<ClientEventInput>,
    ) -> Result<i32> {
        let user = user(ctx).await?;
        let state = state(ctx);
        if !state.config.client_events {
            return Ok(0);
        }
        let events: Vec<crate::client_events::Event> = events.into_iter().map(Into::into).collect();
        let logged =
            state.client_events.report(user.id, &client, &events, std::time::Instant::now());
        Ok(logged as i32)
    }

    /// Admins: how many frames a system's pictures run ahead of its games (0: off), or with no
    /// `frames` back to SovEmu's default (1 frame on the systems that support it). Applies to the next game started. Refused while
    /// `SOVEMU_SYSTEMS__<id>__RUN_AHEAD` sets it.
    async fn set_run_ahead(
        &self,
        ctx: &Context<'_>,
        system: ID,
        frames: Option<i32>,
    ) -> Result<EmulatorSettings> {
        admin(ctx).await?;
        let frames = match frames {
            Some(f) if f < 0 => {
                return Err(crate::error::ApiError::bad_request("frames cannot be negative")).api();
            }
            f => f.map(|f| f as u32),
        };
        crate::settings::set_run_ahead(state(ctx), &system, frames).await.api().map(Into::into)
    }

    /// Starts a game (ending this user's running one first) and returns once it is ready to
    /// stream. Give a `startId` (8–64 letters, digits, `-` or `_`) to follow its progress with
    /// `startProgress` meanwhile.
    async fn start_session(
        &self,
        ctx: &Context<'_>,
        game_id: i64,
        start_id: Option<String>,
    ) -> Result<SessionNode> {
        let user = user(ctx).await?;
        crate::api::start_session(state(ctx), &user, game_id, start_id.as_deref())
            .await
            .api()
            .map(SessionNode)
    }

    /// Ends a session; the worker writes the battery save and the auto save first.
    async fn stop_session(&self, ctx: &Context<'_>, id: ID) -> Result<bool> {
        let user = user(ctx).await?;
        crate::api::stop_session(state(ctx), &user, &id).await.api()?;
        Ok(true)
    }

    /// Answers a client's WebRTC offer (SDP) for a session with the worker's answer (SDP).
    async fn session_offer(&self, ctx: &Context<'_>, id: ID, sdp: String) -> Result<String> {
        let user = user(ctx).await?;
        crate::api::offer(state(ctx), &user, &id, sdp).await.api()
    }

    /// Presses the console's reset button.
    async fn reset_session(&self, ctx: &Context<'_>, id: ID) -> Result<bool> {
        let user = user(ctx).await?;
        crate::api::reset(state(ctx), &user, &id).await.api()?;
        Ok(true)
    }

    /// Adds a game to this user's favourites, or takes it out.
    async fn set_favourite(
        &self,
        ctx: &Context<'_>,
        game_id: i64,
        favourite: bool,
    ) -> Result<bool> {
        let user = user(ctx).await?;
        crate::collections::set_favourite(state(ctx), &user, game_id, favourite).await.api()?;
        Ok(favourite)
    }
    /// Plays a game with one of its soft patches from now on (`Game.patches`), or the original
    /// (`patchId` null). Its next session loads it so; a patched game keeps its own saves.
    async fn set_patch(
        &self,
        ctx: &Context<'_>,
        game_id: i64,
        patch_id: Option<i64>,
    ) -> Result<bool> {
        let user = user(ctx).await?;
        crate::patches::choose(state(ctx), &user, game_id, patch_id).await.api()?;
        Ok(true)
    }
    /// Creates a collection (CONFLICT if this user already has one by that name).
    async fn create_collection(&self, ctx: &Context<'_>, name: String) -> Result<CollectionNode> {
        let user = user(ctx).await?;
        crate::collections::create(state(ctx), &user, &name).await.api().map(CollectionNode)
    }
    async fn rename_collection(&self, ctx: &Context<'_>, id: i64, name: String) -> Result<bool> {
        let user = user(ctx).await?;
        crate::collections::rename(state(ctx), &user, id, &name).await.api()?;
        Ok(true)
    }
    async fn delete_collection(&self, ctx: &Context<'_>, id: i64) -> Result<bool> {
        let user = user(ctx).await?;
        crate::collections::delete(state(ctx), &user, id).await.api()?;
        Ok(true)
    }
    /// Puts a game in one of this user's collections, or takes it out.
    async fn set_in_collection(
        &self,
        ctx: &Context<'_>,
        collection_id: i64,
        game_id: i64,
        member: bool,
    ) -> Result<bool> {
        let user = user(ctx).await?;
        crate::collections::set_member(state(ctx), &user, collection_id, game_id, member)
            .await
            .api()?;
        Ok(member)
    }
    /// Changes a multi-disc game's disc (from 1), as when the game asks for the next one.
    async fn set_disc(&self, ctx: &Context<'_>, session_id: ID, disc: u32) -> Result<bool> {
        let user = user(ctx).await?;
        crate::api::set_disc(state(ctx), &user, &session_id, disc).await.api()?;
        Ok(true)
    }
    /// Saves the session's game to a slot (1–9), with a screenshot.
    async fn save_state(&self, ctx: &Context<'_>, session_id: ID, slot: u32) -> Result<bool> {
        let user = user(ctx).await?;
        crate::api::save_state(state(ctx), &user, &session_id, slot).await.api()?;
        Ok(true)
    }

    /// Loads a slot (0 is the auto save) into the session's game.
    async fn load_state(&self, ctx: &Context<'_>, session_id: ID, slot: u32) -> Result<bool> {
        let user = user(ctx).await?;
        crate::api::load_state(state(ctx), &user, &session_id, slot).await.api()?;
        Ok(true)
    }

    /// Deletes a save state (and its screenshot).
    async fn delete_save(&self, ctx: &Context<'_>, game_id: i64, slot: u32) -> Result<bool> {
        let user = user(ctx).await?;
        crate::api::delete_state(state(ctx), &user, game_id, slot).await.api()?;
        Ok(true)
    }

    /// Adds a cheat (on) for a game; returns the game's cheats. A running session gets it.
    async fn add_cheat(
        &self,
        ctx: &Context<'_>,
        game_id: i64,
        code: String,
        #[graphql(default)] description: String,
    ) -> Result<Vec<crate::cheats::Cheat>> {
        let user = user(ctx).await?;
        crate::cheats::add(state(ctx), &user, game_id, &code, &description).await.api()
    }

    /// Turns a cheat on or off; returns the game's cheats.
    async fn set_cheat_enabled(
        &self,
        ctx: &Context<'_>,
        game_id: i64,
        cheat_id: i64,
        enabled: bool,
    ) -> Result<Vec<crate::cheats::Cheat>> {
        let user = user(ctx).await?;
        crate::cheats::set_enabled(state(ctx), &user, game_id, cheat_id, enabled).await.api()
    }

    async fn delete_cheat(&self, ctx: &Context<'_>, game_id: i64, cheat_id: i64) -> Result<bool> {
        let user = user(ctx).await?;
        crate::cheats::remove(state(ctx), &user, game_id, cheat_id).await.api()?;
        Ok(true)
    }

    /// Saves this user's mapping for a controller model on a system, or on every system without
    /// one, replacing its defaults wherever they play.
    async fn save_controller_mapping(
        &self,
        ctx: &Context<'_>,
        controller: String,
        system: Option<ID>,
        #[graphql(default)] name: String,
        bindings: Vec<crate::controllers::Binding>,
    ) -> Result<crate::controllers::ControllerMapping> {
        let user = user(ctx).await?;
        let state = state(ctx);
        crate::controllers::save(
            state,
            &user,
            &controller,
            system.as_ref().map(|s| s.as_str()),
            &name,
            bindings,
        )
        .await
        .api()
    }

    /// Admins: sets the default mapping for a controller model on a system (or every system),
    /// for every user without their own. A model whose default is in the environment cannot be changed here (CONFLICT).
    async fn save_controller_default(
        &self,
        ctx: &Context<'_>,
        controller: String,
        system: Option<ID>,
        #[graphql(default)] name: String,
        bindings: Vec<crate::controllers::Binding>,
    ) -> Result<crate::controllers::ControllerMapping> {
        admin(ctx).await?;
        let state = state(ctx);
        crate::controllers::save_default(
            state,
            &controller,
            system.as_ref().map(|s| s.as_str()),
            &name,
            bindings,
        )
        .await
        .api()
    }

    /// Admins: forgets the admin's default for a controller model. Returns whether it had one.
    async fn reset_controller_default(
        &self,
        ctx: &Context<'_>,
        controller: String,
        system: Option<ID>,
    ) -> Result<bool> {
        admin(ctx).await?;
        crate::controllers::reset_default(
            state(ctx),
            &controller,
            system.as_ref().map(|s| s.as_str()),
        )
        .await
        .api()
    }

    /// Puts a controller model back on the defaults (an admin's, else the built-in ones).
    /// Returns whether it had a mapping of the user's own.
    async fn reset_controller_mapping(
        &self,
        ctx: &Context<'_>,
        controller: String,
        system: Option<ID>,
    ) -> Result<bool> {
        let user = user(ctx).await?;
        crate::controllers::reset(
            state(ctx),
            &user,
            &controller,
            system.as_ref().map(|s| s.as_str()),
        )
        .await
        .api()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_are_logged_by_how_much_they_matter() {
        assert_eq!(error_kind(None, false), ErrorKind::InvalidQuery);
        assert_eq!(
            error_kind(Some("UNAUTHENTICATED"), true),
            ErrorKind::Routine("UNAUTHENTICATED")
        );
        assert_eq!(error_kind(Some("CONFLICT"), true), ErrorKind::Refused("CONFLICT"));
        assert_eq!(error_kind(Some("UNAVAILABLE"), true), ErrorKind::Failed("UNAVAILABLE"));
        assert_eq!(error_kind(Some("INTERNAL"), true), ErrorKind::Failed("INTERNAL"));
        assert_eq!(
            error_kind(None, true),
            ErrorKind::Failed("INTERNAL"),
            "no code: never went through gql"
        );
    }

    #[test]
    fn an_error_path_reads_as_dotted_text() {
        use async_graphql::PathSegment::{Field, Index};
        let path = [Field("game".into()), Field("saves".into()), Index(0), Field("url".into())];
        assert_eq!(error_path(&path), "game.saves.0.url");
    }

    /// A query that fails to validate comes back as an error with no path or code: the case
    /// logged as an invalid query, so the classification above sees real responses' shape.
    #[tokio::test]
    async fn an_invalid_query_has_neither_path_nor_code() {
        let response = schema().execute("{ noSuchField }").await;
        let error = &response.errors[0];
        assert!(error.path.is_empty());
        assert!(error.extensions.as_ref().and_then(|e| e.get("code")).is_none());
    }

    /// The checked-in schema, which the clients build against, matches the server's. Regenerate
    /// it with `UPDATE_SCHEMA=1 cargo test -p sovemu-server schema_file`.
    #[test]
    fn schema_file_is_current() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("schema.graphql");
        let sdl = schema().sdl();
        if std::env::var_os("UPDATE_SCHEMA").is_some() {
            std::fs::write(&path, &sdl).unwrap();
        }
        let current = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(
            current == sdl,
            "schema.graphql is out of date: run UPDATE_SCHEMA=1 cargo test -p sovemu-server schema_file"
        );
    }
}
