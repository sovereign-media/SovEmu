//! The LaunchBox Games Database (gamesdb.launchbox-app.com), from the dump LaunchBox publishes.
//!
//! LaunchBox has no lookup API: it publishes the whole database as `Metadata.zip` (about 110 MB,
//! rebuilt daily; `Metadata.xml` inside is about 500 MB). It needs no account and has no quota,
//! so it is the default source. The dump is downloaded into `<data>/metadata/launchbox/`,
//! checked again every `SOVEMU_LAUNCHBOX_REFRESH_HOURS` (a conditional GET on its ETag, so an
//! unchanged dump is not downloaded twice), and read in one streaming pass into an index of the
//! games on the platforms SovEmu plays: a few megabytes, rebuilt only when the file changes.
//!
//! Games are matched by normalised name, or one of LaunchBox's alternate names, within the
//! system's platforms. Ratings are LaunchBox's community rating (0 to 5, scaled to 0 to 1);
//! box art is the front box image in the preferred region, from LaunchBox's image CDN.

use std::collections::HashMap;
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use quick_xml::events::Event;
use tokio::io::AsyncWriteExt;

use super::{GameInfo, normalize};

pub const DEFAULT_URL: &str = "https://gamesdb.launchbox-app.com/Metadata.zip";
pub const DEFAULT_IMAGES_URL: &str = "https://images.launchbox-app.com";

/// The LaunchBox platforms SovEmu reads, by position (an index's `platform`).
const PLATFORMS: &[&str] = &[
    "Nintendo Entertainment System",
    "Super Nintendo Entertainment System",
    "Nintendo Game Boy",
    "Nintendo Game Boy Color",
    "Nintendo Game Boy Advance",
    "Nintendo 64",
    "Nintendo GameCube",
    "Sega Genesis",
    "Sega Master System",
    "Sega Game Gear",
    "Sega Dreamcast",
    "Sony Playstation",
    "Sony Playstation 2",
    "SNK Neo Geo AES",
    "SNK Neo Geo MVS",
    "Arcade",
    "Sega SG-1000",
    "Sega CD",
    "Sega 32X",
    "NEC TurboGrafx-16",
    "NEC TurboGrafx-CD",
    "Atari 2600",
    "Atari 7800",
    "Atari Lynx",
    "SNK Neo Geo Pocket",
    "SNK Neo Geo Pocket Color",
    "WonderSwan",
    "WonderSwan Color",
    "Nintendo Virtual Boy",
    "Mattel Intellivision",
    "ColecoVision",
    "GCE Vectrex",
];

/// SovEmu system id → LaunchBox platforms, most likely first.
pub(crate) fn platforms(system: &str) -> &'static [&'static str] {
    match system {
        "nes" => &["Nintendo Entertainment System"],
        "snes" => &["Super Nintendo Entertainment System"],
        "gb" => &["Nintendo Game Boy"],
        "gbc" => &["Nintendo Game Boy Color"],
        "gba" => &["Nintendo Game Boy Advance"],
        "n64" => &["Nintendo 64"],
        "gamecube" => &["Nintendo GameCube"],
        "megadrive" => &["Sega Genesis"],
        "mastersystem" => &["Sega Master System"],
        "gamegear" => &["Sega Game Gear"],
        "dreamcast" => &["Sega Dreamcast"],
        "psx" => &["Sony Playstation"],
        "ps2" => &["Sony Playstation 2"],
        "neogeo" => &["SNK Neo Geo AES", "SNK Neo Geo MVS", "Arcade"],
        "fbneo" => &["Arcade", "SNK Neo Geo MVS", "SNK Neo Geo AES"],
        "sg1000" => &["Sega SG-1000"],
        "segacd" => &["Sega CD"],
        "sega32x" => &["Sega 32X"],
        "pcengine" => &["NEC TurboGrafx-16"],
        "pcenginecd" => &["NEC TurboGrafx-CD"],
        "atari2600" => &["Atari 2600"],
        "atari7800" => &["Atari 7800"],
        "lynx" => &["Atari Lynx"],
        "ngp" => &["SNK Neo Geo Pocket"],
        "ngpc" => &["SNK Neo Geo Pocket Color"],
        "wswan" => &["WonderSwan"],
        "wswanc" => &["WonderSwan Color"],
        "virtualboy" => &["Nintendo Virtual Boy"],
        "intellivision" => &["Mattel Intellivision"],
        "colecovision" => &["ColecoVision"],
        "vectrex" => &["GCE Vectrex"],
        _ => &[],
    }
}

fn platform_index(name: &str) -> Option<u8> {
    PLATFORMS.iter().position(|p| *p == name).map(|i| i as u8)
}

/// A LaunchBox region → the region code `SOVEMU_METADATA_REGIONS` uses for it.
fn region_code(region: Option<&str>) -> &'static str {
    match region {
        None | Some("World") => "wor",
        Some("North America" | "United States" | "Canada") => "us",
        Some("United Kingdom") => "uk",
        Some("Japan") => "jp",
        Some(
            "Europe" | "Germany" | "France" | "Spain" | "Italy" | "The Netherlands" | "Sweden",
        ) => "eu",
        Some(_) => "",
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
struct Game {
    platform: u8,
    name: String,
    overview: Option<String>,
    release_date: Option<String>,
    developer: Option<String>,
    publisher: Option<String>,
    genres: Option<String>,
    players: Option<String>,
    rating: Option<f64>,
    rating_count: u32,
    /// (rank, file name): the lowest rank wins (front box first, then region preference).
    box_art: Option<(u32, String)>,
}

/// The games on SovEmu's platforms, by LaunchBox id and by normalised name.
#[derive(Debug, Default)]
pub struct Index {
    games: HashMap<u32, Game>,
    names: HashMap<(u8, String), Vec<u32>>,
    images_url: String,
}

impl Index {
    pub fn len(&self) -> usize {
        self.games.len()
    }

    pub fn is_empty(&self) -> bool {
        self.games.is_empty()
    }

    /// The game called `title` (or known by it) on one of `system`'s platforms. Several games
    /// of one name on a platform: the most rated. No fuzzy matching: a wrong match is worse than
    /// none.
    pub fn lookup(&self, system: &str, title: &str) -> Option<GameInfo> {
        let key = normalize(title);
        if key.is_empty() {
            return None;
        }
        let game = platforms(system).iter().find_map(|p| {
            let ids = self.names.get(&(platform_index(p)?, key.clone()))?;
            ids.iter().filter_map(|id| self.games.get(id)).max_by_key(|g| g.rating_count)
        })?;
        Some(GameInfo {
            title: Some(game.name.clone()),
            description: game.overview.clone(),
            release_date: game.release_date.clone(),
            developer: game.developer.clone(),
            publisher: game.publisher.clone(),
            genre: game.genres.clone(),
            players: game.players.clone(),
            rating: game.rating,
            box_art_url: game
                .box_art
                .as_ref()
                .map(|(_, file)| format!("{}/{file}", self.images_url.trim_end_matches('/'))),
        })
    }

    fn add_name(&mut self, platform: u8, name: &str, id: u32) {
        let ids = self.names.entry((platform, normalize(name))).or_default();
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
}

/// Reads `Metadata.xml`, keeping the games on SovEmu's platforms. `regions` is the region
/// preference (`SOVEMU_METADATA_REGIONS`) for box art.
pub fn parse(xml: impl BufRead, regions: &[String], images_url: &str) -> Result<Index, String> {
    let mut index = Index { images_url: images_url.to_owned(), ..Index::default() };
    let rank_region = |region: Option<&str>| {
        let code = region_code(region);
        regions.iter().position(|r| r == code).unwrap_or(regions.len()) as u32
    };
    read_records(xml, |record, fields| {
        let get = |name: &str| {
            fields.iter().find(|(k, _)| k == name).map(|(_, v)| v.trim()).filter(|v| !v.is_empty())
        };
        let Some(id) = get("DatabaseID").and_then(|v| v.parse::<u32>().ok()) else { return };
        match record {
            "Game" => {
                let Some(platform) = get("Platform").and_then(platform_index) else { return };
                let Some(name) = get("Name") else { return };
                let release_date = get("ReleaseDate")
                    .and_then(|d| d.get(..10))
                    .filter(|d| d.as_bytes()[4] == b'-')
                    .or_else(|| get("ReleaseYear"))
                    .map(str::to_owned);
                let genres = get("Genres").map(|g| {
                    g.split(';').map(str::trim).filter(|s| !s.is_empty()).collect::<Vec<_>>()
                });
                let game = Game {
                    platform,
                    name: name.to_owned(),
                    overview: get("Overview").map(|o| o.replace('\r', "")),
                    release_date,
                    developer: get("Developer").map(str::to_owned),
                    publisher: get("Publisher").map(str::to_owned),
                    genres: genres.filter(|g| !g.is_empty()).map(|g| g.join(", ")),
                    players: get("MaxPlayers").filter(|p| *p != "0").map(str::to_owned),
                    rating: get("CommunityRating")
                        .and_then(|r| r.parse::<f64>().ok())
                        .filter(|r| *r > 0.0)
                        .map(|r| (r / 5.0).clamp(0.0, 1.0)),
                    rating_count: get("CommunityRatingCount")
                        .and_then(|c| c.parse().ok())
                        .unwrap_or(0),
                    box_art: None,
                };
                index.add_name(platform, name, id);
                index.games.insert(id, game);
            }
            "GameAlternateName" => {
                if let (Some(platform), Some(name)) =
                    (index.games.get(&id).map(|g| g.platform), get("AlternateName"))
                {
                    index.add_name(platform, name, id);
                }
            }
            "GameImage" => {
                let kind = match get("Type") {
                    Some("Box - Front") => 0,
                    Some("Box - Front - Reconstructed") => 1,
                    _ => return,
                };
                let (Some(game), Some(file)) = (index.games.get_mut(&id), get("FileName")) else {
                    return;
                };
                let rank = kind * 100 + rank_region(get("Region"));
                if game.box_art.as_ref().is_none_or(|(r, _)| rank < *r) {
                    game.box_art = Some((rank, file.to_owned()));
                }
            }
            _ => {}
        }
    })?;
    Ok(index)
}

/// Streams the dump's top-level records (`<LaunchBox><Game>…</Game>…`), calling `on` with
/// each one's name and its child elements' text.
fn read_records(
    xml: impl BufRead,
    mut on: impl FnMut(&str, &[(String, String)]),
) -> Result<(), String> {
    let mut reader = quick_xml::Reader::from_reader(xml);
    let mut buf = Vec::new();
    let (mut depth, mut record) = (0usize, String::new());
    let mut fields: Vec<(String, String)> = Vec::new();
    let (mut field, mut text) = (String::new(), String::new());
    let err = |e: &dyn std::fmt::Display| format!("LaunchBox dump: {e}");
    loop {
        match reader.read_event_into(&mut buf).map_err(|e| err(&e))? {
            Event::Start(e) => {
                depth += 1;
                let name = e.name().as_ref().to_owned();
                match depth {
                    2 => {
                        record = name;
                        fields.clear();
                    }
                    3 => {
                        field = name;
                        text.clear();
                    }
                    _ => {}
                }
            }
            Event::End(_) => {
                match depth {
                    3 => fields.push((std::mem::take(&mut field), std::mem::take(&mut text))),
                    2 => on(&record, &fields),
                    _ => {}
                }
                depth = depth.saturating_sub(1);
            }
            // `<Genres />` and the like: an empty field.
            Event::Empty(_) => {}
            Event::Text(t) if depth == 3 => text.push_str(&t.xml10_content()),
            Event::CData(t) if depth == 3 => text.push_str(&t.xml10_content()),
            Event::GeneralRef(r) if depth == 3 => {
                if let Some(c) = r.resolve_char_ref().map_err(|e| err(&e))? {
                    text.push(c);
                } else {
                    let name = r.xml10_content();
                    if let Some(s) = quick_xml::escape::resolve_predefined_entity(&name) {
                        text.push_str(s);
                    }
                }
            }
            Event::Eof => return Ok(()),
            _ => {}
        }
        buf.clear();
    }
}

/// Reads `Metadata.xml` out of a downloaded `Metadata.zip`.
pub(crate) fn read_zip(path: &Path, regions: &[String], images_url: &str) -> Result<Index, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut archive = zip::ZipArchive::new(std::io::BufReader::new(file))
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let entry = archive.by_name("Metadata.xml").map_err(|e| format!("Metadata.xml: {e}"))?;
    parse(std::io::BufReader::with_capacity(1 << 20, entry), regions, images_url)
}

#[derive(Debug)]
pub struct LaunchBox {
    url: String,
    images_url: String,
    dir: PathBuf,
    /// How often to look for a newer dump; `None`: download once, never again.
    refresh: Option<Duration>,
    regions: Vec<String>,
    http: reqwest::Client,
    /// The index, and the dump's modification time it was read from.
    loaded: tokio::sync::Mutex<Option<(SystemTime, Arc<Index>)>>,
}

fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

impl LaunchBox {
    pub fn new(
        url: String,
        images_url: String,
        dir: PathBuf,
        refresh_hours: u64,
        regions: Vec<String>,
    ) -> Self {
        // The dump is 110 MB: no overall timeout (the shared client's 30 s), only for
        // connecting and for each read.
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(30))
            .read_timeout(Duration::from_secs(120))
            .user_agent(concat!("SovEmu/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("a TLS backend is compiled in");
        Self {
            url,
            images_url,
            dir,
            refresh: (refresh_hours > 0).then(|| Duration::from_secs(refresh_hours * 3600)),
            regions,
            http,
            loaded: tokio::sync::Mutex::new(None),
        }
    }

    fn zip_path(&self) -> PathBuf {
        self.dir.join("Metadata.zip")
    }

    /// Holds the dump's ETag; its modification time is when the dump was last checked.
    fn etag_path(&self) -> PathBuf {
        self.dir.join("Metadata.zip.etag")
    }

    /// The index for a metadata job: downloads the dump if there is none, looks for a newer one
    /// if the last check is older than the refresh interval, and reads it again if it changed.
    /// A failed refresh keeps the dump already on disk.
    pub async fn prepare(&self) -> Result<Arc<Index>, String> {
        let zip = self.zip_path();
        let last_check = modified(&self.etag_path()).or_else(|| modified(&zip));
        let due = match (zip.is_file(), last_check) {
            (false, _) | (true, None) => true,
            (true, Some(t)) => self.refresh.is_some_and(|r| t.elapsed().unwrap_or_default() >= r),
        };
        if due {
            match self.download().await {
                Ok(true) => tracing::info!("LaunchBox dump downloaded"),
                Ok(false) => tracing::debug!("LaunchBox dump up to date"),
                Err(e) if zip.is_file() => {
                    tracing::warn!(error = %e, "cannot refresh the LaunchBox dump (keeping it)")
                }
                Err(e) => return Err(e),
            }
        }
        let mtime = modified(&zip).ok_or("no LaunchBox dump")?;
        let mut loaded = self.loaded.lock().await;
        if let Some((t, index)) = &*loaded
            && *t == mtime
        {
            return Ok(index.clone());
        }
        let (regions, images_url) = (self.regions.clone(), self.images_url.clone());
        let started = std::time::Instant::now();
        let index = tokio::task::spawn_blocking(move || read_zip(&zip, &regions, &images_url))
            .await
            .map_err(|e| e.to_string())??;
        tracing::info!(
            count = index.len(),
            secs = started.elapsed().as_secs_f64(),
            "LaunchBox games read"
        );
        let index = Arc::new(index);
        *loaded = Some((mtime, index.clone()));
        Ok(index)
    }

    /// Downloads the dump unless the server says the one on disk is current. `Ok(true)` when a
    /// new dump was written (to a temporary file, checked, then renamed over the old one).
    async fn download(&self) -> Result<bool, String> {
        let (zip, etag_path) = (self.zip_path(), self.etag_path());
        let mut request = self.http.get(&self.url);
        if zip.is_file()
            && let Ok(etag) = tokio::fs::read_to_string(&etag_path).await
            && !etag.trim().is_empty()
        {
            request = request.header(reqwest::header::IF_NONE_MATCH, etag.trim());
        }
        let mut response = request.send().await.map_err(|e| format!("LaunchBox: {e}"))?;
        let status = response.status();
        let etag = response
            .headers()
            .get(reqwest::header::ETAG)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_owned();
        if status == reqwest::StatusCode::NOT_MODIFIED {
            // Rewriting the ETag marks when the dump was last checked.
            let etag = tokio::fs::read_to_string(&etag_path).await.unwrap_or_default();
            tokio::fs::write(&etag_path, etag).await.map_err(|e| e.to_string())?;
            return Ok(false);
        }
        if !status.is_success() {
            return Err(format!("LaunchBox HTTP {status}"));
        }
        tokio::fs::create_dir_all(&self.dir).await.map_err(|e| e.to_string())?;
        let tmp = self.dir.join("Metadata.zip.part");
        let mut file = tokio::fs::File::create(&tmp).await.map_err(|e| e.to_string())?;
        while let Some(chunk) = response.chunk().await.map_err(|e| format!("LaunchBox: {e}"))? {
            file.write_all(&chunk).await.map_err(|e| e.to_string())?;
        }
        file.sync_all().await.map_err(|e| e.to_string())?;
        drop(file);
        let check = tmp.clone();
        let valid = tokio::task::spawn_blocking(move || {
            let file = std::fs::File::open(&check).map_err(|e| e.to_string())?;
            let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
            archive.by_name("Metadata.xml").map(|_| ()).map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| e.to_string())?;
        if let Err(e) = valid {
            let _ = tokio::fs::remove_file(&tmp).await;
            return Err(format!("LaunchBox: the download is not a dump: {e}"));
        }
        tokio::fs::rename(&tmp, &zip).await.map_err(|e| e.to_string())?;
        tokio::fs::write(&etag_path, etag).await.map_err(|e| e.to_string())?;
        Ok(true)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0" standalone="yes"?>
<LaunchBox>
  <Game>
    <Name>Super Mario Bros. 3</Name>
    <ReleaseDate>1990-02-12T00:00:00-08:00</ReleaseDate>
    <Overview>Mario &amp; Luigi return.&#13;
Again.</Overview>
    <MaxPlayers>2</MaxPlayers>
    <DatabaseID>10</DatabaseID>
    <CommunityRating>4.5</CommunityRating>
    <Platform>Nintendo Entertainment System</Platform>
    <CommunityRatingCount>300</CommunityRatingCount>
    <Genres>Platform; Action</Genres>
    <Developer>Nintendo EAD</Developer>
    <Publisher>Nintendo</Publisher>
  </Game>
  <Game>
    <Name>Super Mario Bros. 3</Name>
    <DatabaseID>11</DatabaseID>
    <Platform>Nintendo Entertainment System</Platform>
    <CommunityRatingCount>2</CommunityRatingCount>
    <Genres />
  </Game>
  <Game>
    <Name>The Legend of Zelda: A Link to the Past</Name>
    <ReleaseYear>1991</ReleaseYear>
    <DatabaseID>20</DatabaseID>
    <Platform>Super Nintendo Entertainment System</Platform>
    <MaxPlayers>0</MaxPlayers>
  </Game>
  <Game>
    <Name>Halo</Name>
    <DatabaseID>1</DatabaseID>
    <Platform>Microsoft Xbox</Platform>
  </Game>
  <Platform><Name>Nintendo Entertainment System</Name></Platform>
  <GameAlternateName>
    <AlternateName>Zelda no Densetsu: Kamigami no Triforce</AlternateName>
    <DatabaseID>20</DatabaseID>
    <Region>Japan</Region>
  </GameAlternateName>
  <GameAlternateName><AlternateName>Halo 1</AlternateName><DatabaseID>1</DatabaseID></GameAlternateName>
  <GameImage><DatabaseID>10</DatabaseID><FileName>jp.jpg</FileName><Type>Box - Front</Type><Region>Japan</Region></GameImage>
  <GameImage><DatabaseID>10</DatabaseID><FileName>flyer.jpg</FileName><Type>Advertisement Flyer - Front</Type><Region>North America</Region></GameImage>
  <GameImage><DatabaseID>10</DatabaseID><FileName>na.jpg</FileName><Type>Box - Front</Type><Region>North America</Region></GameImage>
  <GameImage><DatabaseID>10</DatabaseID><FileName>eu.jpg</FileName><Type>Box - Front</Type><Region>Europe</Region></GameImage>
  <GameImage><DatabaseID>20</DatabaseID><FileName>recon.png</FileName><Type>Box - Front - Reconstructed</Type></GameImage>
</LaunchBox>"#;

    fn regions() -> Vec<String> {
        ["us", "wor", "eu", "ss", "uk", "jp"].map(String::from).to_vec()
    }

    fn index() -> Index {
        parse(SAMPLE.as_bytes(), &regions(), "https://img.example/").unwrap()
    }

    #[test]
    fn reads_games_on_sovemu_platforms() {
        let index = index();
        assert_eq!(index.len(), 3, "the Xbox game is skipped");
        let info = index.lookup("nes", "Super Mario Bros 3").unwrap();
        assert_eq!(info.title.as_deref(), Some("Super Mario Bros. 3"));
        assert_eq!(info.description.as_deref(), Some("Mario & Luigi return.\nAgain."));
        assert_eq!(info.release_date.as_deref(), Some("1990-02-12"));
        assert_eq!(info.genre.as_deref(), Some("Platform, Action"));
        assert_eq!(info.players.as_deref(), Some("2"));
        assert_eq!(info.developer.as_deref(), Some("Nintendo EAD"));
        assert_eq!(info.rating, Some(0.9), "4.5 of 5");
        assert_eq!(
            info.box_art_url.as_deref(),
            Some("https://img.example/na.jpg"),
            "the preferred region's front box, and the most rated of two games of one name"
        );
    }

    #[test]
    fn matches_alternate_names_within_the_platform() {
        let index = index();
        let zelda = index.lookup("snes", "Zelda no Densetsu - Kamigami no Triforce").unwrap();
        assert_eq!(zelda.title.as_deref(), Some("The Legend of Zelda: A Link to the Past"));
        assert_eq!(zelda.release_date.as_deref(), Some("1991"), "the year when no date");
        assert_eq!(zelda.players, None, "0 players is unknown");
        assert_eq!(zelda.rating, None);
        assert_eq!(zelda.box_art_url.as_deref(), Some("https://img.example/recon.png"));
        assert!(index.lookup("nes", "The Legend of Zelda: A Link to the Past").is_none());
        assert!(index.lookup("snes", "Zelda").is_none(), "no partial matches");
        assert!(index.lookup("n64", "").is_none());
    }

    #[test]
    fn maps_every_system_to_known_platforms() {
        for system in crate::systems::SYSTEMS.iter().map(|s| s.id) {
            let mapped = platforms(system);
            assert!(!mapped.is_empty(), "{system} has no LaunchBox platform");
            assert!(mapped.iter().all(|p| platform_index(p).is_some()), "{system}");
        }
    }

    /// A `Metadata.zip` holding `xml` as its `Metadata.xml`.
    pub(crate) fn zip_bytes(xml: &str) -> Vec<u8> {
        use std::io::Write;
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        zip.start_file("Metadata.xml", options).unwrap();
        zip.write_all(xml.as_bytes()).unwrap();
        zip.finish().unwrap().into_inner()
    }

    /// Against a real dump: `SOVEMU_LAUNCHBOX_DUMP=/path/to/Metadata.zip cargo test -p
    /// sovemu-library real_dump -- --ignored --nocapture`.
    #[test]
    #[ignore = "needs a downloaded Metadata.zip"]
    fn real_dump() {
        let path = std::env::var("SOVEMU_LAUNCHBOX_DUMP").expect("SOVEMU_LAUNCHBOX_DUMP");
        let started = std::time::Instant::now();
        let index = read_zip(Path::new(&path), &regions(), DEFAULT_IMAGES_URL).unwrap();
        println!("{} games in {:.1}s", index.len(), started.elapsed().as_secs_f64());
        for (system, title) in [
            ("nes", "Super Mario Bros. 3"),
            ("snes", "The Legend of Zelda - A Link to the Past"),
            ("megadrive", "Sonic the Hedgehog 2"),
            ("psx", "Castlevania - Symphony of the Night"),
            ("gba", "Metroid - Zero Mission"),
            ("fbneo", "Metal Slug - Super Vehicle-001"),
            ("neogeo", "The King of Fighters '98 - The Slugfest"),
        ] {
            let info = index.lookup(system, title);
            println!("{system} {title}: {info:?}");
            assert!(info.is_some_and(|i| i.rating.is_some()), "{system} {title}");
        }
    }

    fn write_zip(path: &Path, xml: &str) {
        std::fs::write(path, zip_bytes(xml)).unwrap();
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("sovemu-launchbox-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Serves `zip` with an ETag, answering 304 to a matching `If-None-Match`; counts the full
    /// downloads.
    async fn serve(zip: Vec<u8>) -> (String, Arc<std::sync::atomic::AtomicUsize>) {
        use axum::http::{HeaderMap, StatusCode, header};
        use std::sync::atomic::{AtomicUsize, Ordering};
        let count = Arc::new(AtomicUsize::new(0));
        let served = count.clone();
        let app = axum::Router::new().route(
            "/Metadata.zip",
            axum::routing::get(move |headers: HeaderMap| {
                let (zip, served) = (zip.clone(), served.clone());
                async move {
                    if headers.get(header::IF_NONE_MATCH).is_some_and(|v| v == "\"v1\"") {
                        return (StatusCode::NOT_MODIFIED, [(header::ETAG, "\"v1\"")], Vec::new());
                    }
                    served.fetch_add(1, Ordering::SeqCst);
                    (StatusCode::OK, [(header::ETAG, "\"v1\"")], zip)
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (format!("http://{addr}/Metadata.zip"), count)
    }

    #[tokio::test]
    async fn downloads_once_then_checks_with_the_etag() {
        use std::sync::atomic::Ordering;
        let dir = temp_dir("download");
        write_zip(&dir.join("source.zip"), SAMPLE);
        let (url, downloads) = serve(std::fs::read(dir.join("source.zip")).unwrap()).await;
        let cache = dir.join("cache");

        // Refresh every hour: the first job downloads, the next reuses the dump and index.
        let lb =
            LaunchBox::new(url.clone(), DEFAULT_IMAGES_URL.into(), cache.clone(), 1, regions());
        let index = lb.prepare().await.unwrap();
        assert!(index.lookup("nes", "Super Mario Bros. 3").is_some());
        assert_eq!(downloads.load(Ordering::SeqCst), 1);
        let again = lb.prepare().await.unwrap();
        assert!(Arc::ptr_eq(&index, &again), "unchanged dump: same index");
        assert_eq!(downloads.load(Ordering::SeqCst), 1);

        // A check that is due sends the ETag and gets 304: no second download.
        let old = SystemTime::now() - Duration::from_secs(2 * 3600);
        std::fs::File::options()
            .write(true)
            .open(cache.join("Metadata.zip.etag"))
            .unwrap()
            .set_modified(old)
            .unwrap();
        let lb = LaunchBox::new(url, DEFAULT_IMAGES_URL.into(), cache.clone(), 1, regions());
        assert_eq!(lb.prepare().await.unwrap().len(), 3);
        assert_eq!(downloads.load(Ordering::SeqCst), 1);
        assert!(modified(&cache.join("Metadata.zip.etag")).unwrap() > old, "check recorded");
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn keeps_the_dump_it_has_when_the_server_is_unreachable() {
        let dir = temp_dir("offline");
        write_zip(&dir.join("Metadata.zip"), SAMPLE);
        std::fs::File::options()
            .write(true)
            .open(dir.join("Metadata.zip"))
            .unwrap()
            .set_modified(SystemTime::now() - Duration::from_secs(2 * 3600))
            .unwrap();
        // Nothing listens on port 9; refresh 0 never checks, refresh 1 tries and keeps the dump.
        for refresh in [0, 1] {
            let lb = LaunchBox::new(
                "http://127.0.0.1:9/Metadata.zip".into(),
                DEFAULT_IMAGES_URL.into(),
                dir.clone(),
                refresh,
                regions(),
            );
            assert_eq!(lb.prepare().await.unwrap().len(), 3);
        }
        let empty = temp_dir("none");
        let lb = LaunchBox::new(
            "http://127.0.0.1:9/Metadata.zip".into(),
            DEFAULT_IMAGES_URL.into(),
            empty.clone(),
            0,
            regions(),
        );
        assert!(lb.prepare().await.is_err(), "no dump and no server");
        std::fs::remove_dir_all(dir).ok();
        std::fs::remove_dir_all(empty).ok();
    }
}
