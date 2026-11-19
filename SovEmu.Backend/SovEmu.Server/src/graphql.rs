//! The API: GraphQL at `POST /graphql` (GraphiQL at `GET /graphql`), the only API the clients
//! use. Binary files (artwork, videos, save screenshots) are plain HTTP under `/media/`, whose
//! URLs the API hands out; `/health` and `/metrics` stay plain HTTP for probes and Prometheus.
//!
//! Resolvers call the same functions the rest of the server does (`crate::api`, `crate::auth`,
//! …). Sign-in is the `sovemu_session` cookie, as before: `login` and `setup` set it, `logout`
//! clears it. Errors carry a `code` in their extensions (`UNAUTHENTICATED`, `FORBIDDEN`,
//! `NOT_FOUND`, `BAD_REQUEST`, `CONFLICT`, `GONE`, `TOO_MANY_REQUESTS`, `UNAVAILABLE`, …).
//!
//! Numbers: GraphQL's `Int` is 32-bit, so byte counts and Unix times are `Float` (exact to
//! 2^53); ids and counts are `Int`. The schema is checked in as `schema.graphql` (see
//! [`tests`]), for the web, Android and iOS clients.

use std::collections::BTreeMap;

use async_graphql::http::GraphiQLSource;
use async_graphql::{
    Context, EmptySubscription, Enum, ErrorExtensions, ID, InputObject, Json, Object, Schema,
    SimpleObject,
};
use async_graphql_axum::{GraphQLRequest, GraphQLResponse};
use axum::extract::State;
use axum::http::header::SET_COOKIE;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse};
use tokio::sync::OnceCell;

use crate::AppState;
use crate::auth::{self, Credentials, NewUser, User};
use crate::error::ApiError;

pub type ApiSchema = Schema<Query, Mutation, EmptySubscription>;

/// The schema, without per-request data. Queries deeper than 12 levels or costlier than 2000
/// fields are refused.
pub fn schema() -> ApiSchema {
    Schema::build(Query, Mutation, EmptySubscription)
        .limit_depth(12)
        .limit_complexity(2000)
        .finish()
}

/// Per request: the sign-in token from the cookie, and the user it names, looked up once.
struct Auth {
    token: Option<String>,
    user: OnceCell<Option<User>>,
}

pub async fn handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    request: GraphQLRequest,
) -> GraphQLResponse {
    let auth = Auth { token: auth::cookie_token(&headers), user: OnceCell::new() };
    let request = request.into_inner().data(state.clone()).data(auth);
    state.schema.execute(request).await.into()
}

/// GraphiQL, for trying queries in a browser (signed in, the cookie comes along).
pub async fn graphiql() -> impl IntoResponse {
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
        .get_or_try_init(|| auth::user_for_token(&state(ctx).db, auth.token.as_deref()))
        .await
        .map_err(gql)?;
    Ok(user.clone())
}

/// The signed-in user, or `UNAUTHENTICATED`.
async fn user(ctx: &Context<'_>) -> Result<User> {
    viewer(ctx).await?.ok_or_else(|| gql(ApiError::unauthorized()))
}

/// The signed-in user, who must be an admin, or `FORBIDDEN`.
async fn admin(ctx: &Context<'_>) -> Result<User> {
    let user = user(ctx).await?;
    if user.is_admin { Ok(user) } else { Err(gql(ApiError::forbidden())) }
}

fn set_cookie(ctx: &Context<'_>, cookie: HeaderValue) {
    ctx.insert_http_header(SET_COOKIE, cookie);
}

// --- Types -----------------------------------------------------------------------------------

#[derive(SimpleObject)]
struct Viewer {
    /// No account exists yet: the client should offer first-run setup.
    setup_required: bool,
    /// The signed-in user, if any.
    user: Option<User>,
}

#[derive(SimpleObject)]
struct ClientConfig {
    /// `RTCIceServer` objects (STUN/TURN) for WebRTC, from `SOVEMU_ICE_SERVERS`.
    ice_servers: Json<serde_json::Value>,
}

/// Where a 3D system renders on this server.
#[derive(Enum, Clone, Copy, PartialEq, Eq)]
enum Rendering {
    Gpu,
    Software,
    Off,
}

impl From<crate::gpu::Mode> for Rendering {
    fn from(m: crate::gpu::Mode) -> Self {
        match m {
            crate::gpu::Mode::Gpu => Self::Gpu,
            crate::gpu::Mode::Software => Self::Software,
            crate::gpu::Mode::Off => Self::Off,
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
        tokio::task::spawn_blocking(move || crate::bios::missing_required(&dir, games, id))
            .await
            .unwrap_or(false)
    }
    /// Its BIOS files, checked by name and MD5; null for a system that uses none.
    async fn bios(&self, ctx: &Context<'_>) -> Option<SystemBios> {
        let (dir, games) = bios_places(state(ctx), self.0.system);
        let id = self.0.system.id;
        let report = tokio::task::spawn_blocking(move || crate::bios::check(&dir, &games(), id))
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
    system: crate::systems::System,
) -> (std::path::PathBuf, impl Fn() -> Vec<std::path::PathBuf> + Send + 'static) {
    let roms = (state.config.library_mode != crate::LibraryMode::Cache)
        .then(|| state.config.roms_dir.clone());
    let games =
        move || roms.as_deref().map(|r| crate::bios::game_dirs(r, &system)).unwrap_or_default();
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
    fn from(r: crate::bios::Report, system: &str) -> Self {
        use crate::bios::{FileState, Importance, Kind, State};
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
    /// Where to load a kind of artwork from, if the game has it.
    async fn media_url(&self, kind: MediaKind) -> Option<String> {
        self.0
            .media
            .split(',')
            .any(|k| k == kind.name())
            .then(|| format!("/media/games/{}/{}", self.0.id, kind.name()))
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
    /// The screen when it was saved, if there is one (the URL changes with each save).
    async fn screenshot_url(&self) -> Option<String> {
        self.slot.screenshot.then(|| {
            format!(
                "/media/games/{}/saves/{}?t={}",
                self.game_id, self.slot.slot, self.slot.saved_at
            )
        })
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
struct SessionNode(crate::workers::SessionInfo);

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

impl From<crate::starts::StartStatus> for StartStatus {
    fn from(s: crate::starts::StartStatus) -> Self {
        use crate::starts::Phase;
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
}

impl From<crate::settings::SystemSettings> for EmulatorSettings {
    fn from(s: crate::settings::SystemSettings) -> Self {
        use crate::settings::Source;
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
                    source: match x.source {
                        Source::Core => SettingSource::Core,
                        Source::Sovemu => SettingSource::Sovemu,
                        Source::Admin => SettingSource::Admin,
                        Source::Environment => SettingSource::Environment,
                    },
                })
                .collect(),
            categories: s
                .categories
                .into_iter()
                .map(|c| SettingCategory { key: c.key, description: c.description, info: c.info })
                .collect(),
            needs_game: s.needs_game,
            environment: s.environment,
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
    #[graphql(default)]
    is_admin: bool,
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
    /// Who is signed in, and whether first-run setup is needed. Works signed out.
    async fn viewer(&self, ctx: &Context<'_>) -> Result<Viewer> {
        let users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
            .fetch_one(&state(ctx).db)
            .await
            .map_err(|e| gql(e.into()))?;
        Ok(Viewer { setup_required: users == 0, user: viewer(ctx).await? })
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
    async fn users(&self, ctx: &Context<'_>) -> Result<Vec<User>> {
        admin(ctx).await?;
        auth::list_users(state(ctx)).await.api()
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
    /// First run: creates the admin account and signs it in (sets the cookie). Only while no
    /// account exists.
    async fn setup(&self, ctx: &Context<'_>, username: String, password: String) -> Result<User> {
        let (user, cookie) =
            auth::setup(state(ctx), Credentials { username, password }).await.api()?;
        set_cookie(ctx, cookie);
        Ok(user)
    }

    /// Signs in (sets the cookie). Repeated failures for a username are slowed down.
    async fn login(&self, ctx: &Context<'_>, username: String, password: String) -> Result<User> {
        let (user, cookie) =
            auth::login(state(ctx), Credentials { username, password }).await.api()?;
        set_cookie(ctx, cookie);
        Ok(user)
    }

    /// Signs out (clears the cookie).
    async fn logout(&self, ctx: &Context<'_>) -> Result<bool> {
        let token = ctx.data_unchecked::<Auth>().token.clone();
        let cookie = auth::logout(state(ctx), token.as_deref()).await.api()?;
        set_cookie(ctx, cookie);
        Ok(true)
    }

    /// Admins: adds an account.
    async fn add_user(&self, ctx: &Context<'_>, input: NewUserInput) -> Result<User> {
        let me = admin(ctx).await?;
        let new = NewUser {
            username: input.username,
            password: input.password,
            is_admin: input.is_admin,
        };
        auth::add_user(state(ctx), &me, &new).await.api()
    }

    /// Admins: rescans the library.
    async fn scan_library(&self, ctx: &Context<'_>) -> Result<ScanReport> {
        admin(ctx).await?;
        let r = crate::api::scan(state(ctx)).await.api()?;
        Ok(ScanReport { found: r.found as i64, added: r.added as i64, removed: r.removed as i64 })
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

    /// Saves the session's game to a slot (1–9), with a screenshot.
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
