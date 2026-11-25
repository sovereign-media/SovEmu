//! The library as a player sees it: systems, games, Home, search, collections, favourites, patches, cheats and saves.

use super::*;
use crate::collections::Owner;

#[derive(Enum, Clone, Copy, PartialEq, Eq)]
pub(super) enum Rendering {
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
pub(super) struct SystemNode(pub(super) crate::api::SystemSummary);

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
    /// A photo of its hardware for its card (a transparent cut-out): SovEmu's, or the admin's own
    /// from `<data>/systems/<id>.webp`. A URL signed for this user; null when it has none.
    async fn photo_url(&self, ctx: &Context<'_>) -> Result<Option<String>> {
        let user = user(ctx).await?;
        let id = self.0.system.id;
        Ok(state(ctx)
            .appearance()
            .photo(id)
            .map(|_| state(ctx).assets.sign(user.id, &format!("/media/systems/{id}"))))
    }
    async fn game_count(&self) -> i64 {
        self.0.game_count
    }
    /// A core that plays it is installed (and, for a 3D system, there is a renderer).
    async fn playable(&self) -> bool {
        self.0.playable
    }
    /// A 3D system: its cores render with OpenGL and read analog sticks.
    /// The shape of its games' boxes, width over height (US and European releases): game cards
    /// take it so box art fills them.
    async fn box_aspect(&self) -> f64 {
        self.0.system.box_aspect()
    }

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
pub(super) fn bios_places(
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
pub(super) enum BiosImportance {
    /// Games will not start without it.
    Required,
    /// The core has a built-in replacement that plays most games.
    Recommended,
    /// A boot logo, an add-on.
    Optional,
}

/// One BIOS file.
#[derive(Enum, Clone, Copy, PartialEq, Eq)]
pub(super) enum BiosFileState {
    /// A known good dump (found by its checksum or its name, anywhere in the BIOS folder).
    Ok,
    /// Found by its name, but not a dump the list knows: a bad dump, or a revision it lacks.
    /// Used all the same.
    Unverified,
    /// No longer reported: BIOS files are found wherever they are in the BIOS folder.
    #[graphql(deprecation = "BIOS files are found in any layout; nothing is misplaced")]
    Misplaced,
    Missing,
}

/// A system's BIOS files as a whole.
#[derive(Enum, Clone, Copy, PartialEq, Eq)]
pub(super) enum BiosState {
    /// Everything required and recommended is there and verified.
    Ready,
    /// Playable, but a file is unverified.
    Warning,
    /// Playable, without a recommended file.
    Incomplete,
    /// A required file is missing: games will not start.
    Missing,
}

#[derive(SimpleObject)]
pub(super) struct SystemBios {
    pub(super) state: BiosState,
    /// The folder SovEmu recommends for the system's files, relative to the server's BIOS folder
    /// (`SOVEMU_BIOS_DIR`). Files are found in any layout; this one keeps them tidy.
    pub(super) folder: String,
    pub(super) groups: Vec<BiosGroup>,
}

/// A BIOS the system uses: one file, or any one of several.
#[derive(SimpleObject)]
pub(super) struct BiosGroup {
    pub(super) label: String,
    pub(super) importance: BiosImportance,
    /// What it does, and what happens without it.
    pub(super) help: String,
    /// Any one of `files` is enough.
    pub(super) any_one: bool,
    pub(super) satisfied: bool,
    pub(super) files: Vec<BiosFile>,
}

#[derive(SimpleObject)]
pub(super) struct BiosFile {
    /// The name the core reads it by (in the recommended folder: `USA/IPL.bin`).
    pub(super) name: String,
    pub(super) label: String,
    pub(super) state: BiosFileState,
    /// Where it was found, relative to the BIOS folder (`roms/<folder>/…` beside the games).
    pub(super) found_at: Option<String>,
    /// Of the file found.
    pub(super) md5: Option<String>,
    /// The good dumps' MD5s, any one of which is right. Empty when any content is fine, or
    /// for a system that takes any number of dumps (PS2), whose list is too long to show.
    pub(super) expected_md5: Vec<String>,
    /// Bytes expected.
    pub(super) size: f64,
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
                            expected_md5: f.expected_md5,
                            size: f.size as f64,
                        })
                        .collect(),
                })
                .collect(),
        }
    }
}

/// The ESRB's age ratings.
#[derive(Enum, Clone, Copy, PartialEq, Eq)]
pub(super) enum EsrbRating {
    /// EC: Early Childhood (until 2018).
    EarlyChildhood,
    /// E: Everyone (K-A, Kids to Adults, until 1998).
    Everyone,
    /// E10+: Everyone 10+.
    Everyone10Plus,
    /// T: Teen.
    Teen,
    /// M: Mature 17+.
    Mature,
    /// AO: Adults Only 18+.
    AdultsOnly,
    /// RP: Rating Pending.
    RatingPending,
}

impl From<sovemu_library::info::Esrb> for EsrbRating {
    fn from(r: sovemu_library::info::Esrb) -> Self {
        use sovemu_library::info::Esrb;
        match r {
            Esrb::EarlyChildhood => Self::EarlyChildhood,
            Esrb::Everyone => Self::Everyone,
            Esrb::Everyone10Plus => Self::Everyone10Plus,
            Esrb::Teen => Self::Teen,
            Esrb::Mature => Self::Mature,
            Esrb::AdultsOnly => Self::AdultsOnly,
            Esrb::RatingPending => Self::RatingPending,
        }
    }
}

/// A comma-separated list of names as a list.
fn split_names(names: Option<&str>) -> Vec<&str> {
    names.unwrap_or("").split(',').map(str::trim).filter(|n| !n.is_empty()).collect()
}

/// Artwork and video kinds a game can have.
#[derive(Enum, Clone, Copy, PartialEq, Eq)]
pub(super) enum MediaKind {
    /// A screenshot, shown behind the game.
    Image,
    /// Box art.
    Thumb,
    /// The title's logo.
    Marquee,
    /// A short gameplay video.
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
pub(super) struct GameNode(pub(super) crate::api::Game);

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
        crate::collections::of_game(state(ctx), Owner::User(user.id), self.0.id).await.api()
    }
    /// The ids of the shared collections that hold it.
    async fn shared_collection_ids(&self, ctx: &Context<'_>) -> Result<Vec<i64>> {
        user(ctx).await?;
        crate::collections::of_game(state(ctx), Owner::Server, self.0.id).await.api()
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
    /// (valid `SOVEMU_ASSET_URL_HOURS`; ask again for a fresh one). With `width` (the box it is
    /// shown in, in device pixels), a lossy WebP copy at most that wide: the width is rounded up
    /// to the grid copies are made on (16 px steps, from 144), and a picture narrower than it is
    /// not enlarged. Without it, the original. Videos are always the original.
    async fn media_url(
        &self,
        ctx: &Context<'_>,
        kind: MediaKind,
        width: Option<i32>,
    ) -> Result<Option<String>> {
        if !self.0.media.split(',').any(|k| k == kind.name()) {
            return Ok(None);
        }
        let user = user(ctx).await?;
        let mut path = format!("/media/games/{}/{}", self.0.id, kind.name());
        if let Some(w) = width.filter(|_| kind != MediaKind::Video) {
            let w = sovemu_library::artwork::grid_width(w.max(1) as u32);
            path.push_str(&format!("/w{w}"));
        }
        Ok(Some(state(ctx).assets.sign(user.id, &path)))
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
    /// The ESRB's age rating, if the game information has one.
    async fn esrb(&self) -> Option<EsrbRating> {
        self.0.esrb.as_deref().and_then(sovemu_library::info::Esrb::parse).map(EsrbRating::from)
    }
    /// The ESRB rating's icon (PNG), if the game has a rating and the decoration folders an icon
    /// for it: the built-in set's `ratings/esrb/<rating>.png`, or an admin's own at that path in
    /// an earlier decorations folder. A URL signed for this user.
    async fn esrb_badge_url(&self, ctx: &Context<'_>) -> Result<Option<String>> {
        let Some(rating) = self.0.esrb.as_deref().and_then(sovemu_library::info::Esrb::parse)
        else {
            return Ok(None);
        };
        let state = state(ctx);
        let Some(path) = state.decorations().esrb_badge(rating) else { return Ok(None) };
        let user = user(ctx).await?;
        Ok(Some(state.assets.sign(user.id, &path)))
    }
    /// Ways to play: "Single player", "Multiplayer (simultaneous)" …
    async fn modes(&self) -> Vec<&str> {
        split_names(self.0.modes.as_deref())
    }
    /// The series and families it belongs to: "Castlevania" …
    async fn series(&self) -> Vec<&str> {
        split_names(self.0.series.as_deref())
    }
    /// Its themes: "Horror", "Fantasy" …
    async fn themes(&self) -> Vec<&str> {
        split_names(self.0.themes.as_deref())
    }
    /// Its genres, as `genre` lists them.
    async fn genres(&self) -> Vec<&str> {
        split_names(self.0.genre.as_deref())
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
pub(super) struct SaveSlotNode {
    pub(super) game_id: i64,
    pub(super) slot: crate::api::SaveSlot,
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
pub(super) struct Home {
    /// The hero: games with artwork and a description first, a new pick each day.
    pub(super) featured: Vec<GameNode>,
    /// This user's games, most recently played first.
    pub(super) recent: Vec<GameNode>,
    /// The library's newest games.
    pub(super) added: Vec<GameNode>,
    /// This user's favourites, by title.
    pub(super) favourites: Vec<GameNode>,
    /// The library's best rated games (`Game.rating`), best first.
    pub(super) top_rated: Vec<GameNode>,
}

/// A named list of this user's games.
pub(super) struct CollectionNode(pub(super) crate::collections::Collection);

/// What a multi-part game's files are.
#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub(super) enum PartKind {
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
pub(super) struct PatchNode(pub(super) crate::patches::Patch);

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
pub(super) struct DecorationNode(pub(super) sovemu_library::decorations::Decoration);

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
    /// Kept by an admin for everyone: everyone sees it, only an admin changes it.
    async fn shared(&self) -> bool {
        self.0.shared
    }
    /// Declared in the server's environment (`SOVEMU_COLLECTIONS__…`): changed only there.
    async fn declared(&self) -> bool {
        self.0.declared
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

/// One owner's collections, as nodes.
pub(super) async fn collections(ctx: &Context<'_>, owner: Owner) -> Result<Vec<CollectionNode>> {
    let list = crate::collections::list(state(ctx), owner).await.api()?;
    Ok(list.into_iter().map(CollectionNode).collect())
}

#[derive(Default)]
pub(super) struct LibraryQuery;

#[Object]
impl LibraryQuery {
    /// The home screen's feed.
    async fn home(&self, ctx: &Context<'_>) -> Result<Home> {
        let user = user(ctx).await?;
        let home = crate::home::home(state(ctx), &user).await.api()?;
        Ok(Home {
            featured: home.featured.into_iter().map(GameNode).collect(),
            recent: home.recent.into_iter().map(GameNode).collect(),
            added: home.added.into_iter().map(GameNode).collect(),
            favourites: home.favourites.into_iter().map(GameNode).collect(),
            top_rated: home.top_rated.into_iter().map(GameNode).collect(),
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
        collections(ctx, Owner::User(user.id)).await
    }

    /// The collections an admin keeps for everyone, by name.
    async fn shared_collections(&self, ctx: &Context<'_>) -> Result<Vec<CollectionNode>> {
        user(ctx).await?;
        collections(ctx, Owner::Server).await
    }

    /// The systems with games (hidden ones and those with none left out), in SovEmu's order.
    async fn systems(&self, ctx: &Context<'_>) -> Result<Vec<SystemNode>> {
        user(ctx).await?;
        let systems = crate::api::systems_with_games(state(ctx)).await.api()?;
        Ok(systems.into_iter().map(SystemNode).collect())
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
}

#[derive(Default)]
pub(super) struct LibraryMutation;

#[Object]
impl LibraryMutation {
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
        crate::collections::create(state(ctx), Owner::User(user.id), &name)
            .await
            .api()
            .map(CollectionNode)
    }

    async fn rename_collection(&self, ctx: &Context<'_>, id: i64, name: String) -> Result<bool> {
        let user = user(ctx).await?;
        crate::collections::rename(state(ctx), Owner::User(user.id), id, &name).await.api()?;
        Ok(true)
    }

    async fn delete_collection(&self, ctx: &Context<'_>, id: i64) -> Result<bool> {
        let user = user(ctx).await?;
        crate::collections::delete(state(ctx), Owner::User(user.id), id).await.api()?;
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
        crate::collections::set_member(
            state(ctx),
            Owner::User(user.id),
            collection_id,
            game_id,
            member,
        )
        .await
        .api()?;
        Ok(member)
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
}
