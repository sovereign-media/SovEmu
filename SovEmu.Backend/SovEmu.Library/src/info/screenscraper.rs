//! ScreenScraper (screenscraper.fr): the database retro frontends scrape from.
//!
//! `jeuInfos.php` identifies a game by MD5 + size + file name within a system and answers in
//! XML (the shape ES-DE parses). Requests carry developer credentials (`devid`/`devpassword`,
//! requested from ScreenScraper) and optionally a user account (`ssid`/`sspassword`), which raises
//! the daily quota and thread count.
//!
//! Its media URLs carry the same credentials. They are stripped from the box art URL a lookup
//! stores ([`strip_credentials`]) and added again when the box art job downloads it
//! ([`ScreenScraper::authorize`]), so no credential is ever written to the database.
//!
//! Every answer carries the account's quota (`ssuser`), kept for [`Provider::quota`], and the
//! game's own id (`jeu id`), which the server stores so a later lookup asks for that game
//! (`gameid`) instead of matching the file again. Requests are paced (`with_delay`; free
//! accounts get one at a time), and a busy or closed-to-guests server is backed off from.
//! ScreenScraper reports many errors as French text with a 200, so answers are sorted by their
//! text as well as their status ([`classify`]).

use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::{BoxFuture, Found, GameInfo, LookupInput, MetaError, Progress, Provider, Quota, Step};

pub const DEFAULT_BASE: &str = "https://api.screenscraper.fr/api2";

/// SovEmu system id → ScreenScraper `systemeid`.
pub(crate) fn system_id(system: &str) -> Option<u32> {
    Some(match system {
        "megadrive" => 1,
        "mastersystem" => 2,
        "nes" => 3,
        "snes" => 4,
        "gb" => 9,
        "gbc" => 10,
        "gba" => 12,
        "gamegear" => 21,
        "psx" => 57,
        "fbneo" => 75,
        "neogeo" => 142,
        "sg1000" => 109,
        "segacd" => 20,
        "sega32x" => 19,
        "pcengine" => 31,
        "pcenginecd" => 114,
        "atari2600" => 26,
        "atari7800" => 41,
        "lynx" => 28,
        "ngp" => 25,
        "ngpc" => 82,
        "wswan" => 45,
        "wswanc" => 46,
        "virtualboy" => 11,
        "intellivision" => 115,
        "colecovision" => 48,
        "vectrex" => 102,
        "n64" => 14,
        "dreamcast" => 23,
        // Not yet checked against systemesListe.php (as ES-DE and Batocera use them).
        "gamecube" => 13,
        "ps2" => 58,
        _ => return None,
    })
}

#[derive(Debug, Clone)]
pub struct Credentials {
    pub dev_id: String,
    pub dev_password: String,
    pub user: Option<(String, String)>,
}

#[derive(Debug)]
pub struct ScreenScraper {
    base: String,
    credentials: Credentials,
    http: reqwest::Client,
    regions: Vec<String>,
    languages: Vec<String>,
    /// The pause between requests.
    delay: Duration,
    /// When the last request finished; requests take turns on it.
    last: tokio::sync::Mutex<Option<Instant>>,
    quota: Mutex<Option<Quota>>,
}

/// How long to back off a busy server (HTTP 401 and 429, or its "maximum threads" text) before
/// each retry; after the last, the service is left alone for the rest of the job.
const BACKOFF: [Duration; 2] = [Duration::from_secs(30), Duration::from_secs(90)];

/// Query parameters that hold credentials.
const CREDENTIAL_PARAMS: &[&str] = &["devid", "devpassword", "ssid", "sspassword"];

/// `url` without its credential parameters.
pub(crate) fn strip_credentials(url: &str) -> String {
    let Ok(mut parsed) = reqwest::Url::parse(url) else { return url.to_owned() };
    if !parsed.query_pairs().any(|(k, _)| CREDENTIAL_PARAMS.contains(&k.as_ref())) {
        return url.to_owned();
    }
    let kept: Vec<(String, String)> = parsed
        .query_pairs()
        .filter(|(k, _)| !CREDENTIAL_PARAMS.contains(&k.as_ref()))
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    if kept.is_empty() {
        parsed.set_query(None);
    } else {
        parsed.query_pairs_mut().clear().extend_pairs(kept);
    }
    parsed.into()
}

/// Default region preference for names, dates and media: USA first, as most sets are.
pub(crate) const DEFAULT_REGIONS: &[&str] = &["us", "wor", "eu", "ss", "uk", "jp"];
pub(crate) const DEFAULT_LANGUAGES: &[&str] = &["en", "wor"];

fn pick<'a, S: AsRef<str>>(
    nodes: impl Iterator<Item = roxmltree::Node<'a, 'a>>,
    attr: &str,
    prefs: &[S],
) -> Option<String> {
    let all: Vec<_> =
        nodes.filter_map(|n| Some((n.attribute(attr).unwrap_or(""), n.text()?.trim()))).collect();
    prefs
        .iter()
        .find_map(|p| all.iter().find(|(a, _)| *a == p.as_ref()))
        .or_else(|| all.first())
        .map(|(_, t)| (*t).to_owned())
        .filter(|t| !t.is_empty())
}

/// Parses a `jeuInfos.php` XML response with the default preferences. `None` when it holds no
/// game.
#[cfg(test)]
pub fn parse(xml: &str) -> Option<GameInfo> {
    parse_with(xml, DEFAULT_REGIONS, DEFAULT_LANGUAGES)
}

/// The game's ScreenScraper id (`<jeu id="…">`).
pub(crate) fn game_id(xml: &str) -> Option<String> {
    let doc = roxmltree::Document::parse(xml).ok()?;
    let jeu = doc.descendants().find(|n| n.has_tag_name("jeu"))?;
    jeu.attribute("id").filter(|id| !id.is_empty() && *id != "0").map(str::to_owned)
}

/// The account's quota from an answer's `ssuser` block.
pub(crate) fn quota(xml: &str) -> Option<Quota> {
    let doc = roxmltree::Document::parse(xml).ok()?;
    let user = doc.descendants().find(|n| n.has_tag_name("ssuser"))?;
    let number = |tag: &str| {
        user.children()
            .find(|c| c.has_tag_name(tag))
            .and_then(|c| c.text())
            .and_then(|t| t.trim().parse::<u64>().ok())
    };
    let requests_today = number("requeststoday")?;
    Some(Quota {
        requests_today,
        max_requests_per_day: number("maxrequestsperday").filter(|n| *n > 0),
        misses_today: number("requestskotoday").unwrap_or(0),
        max_misses_per_day: number("maxrequestskoperday").filter(|n| *n > 0),
    })
}

/// What an answer means, from its status and its text.
#[derive(Debug, PartialEq)]
pub(crate) enum Answer {
    /// A game (or not-a-game) in XML.
    Xml,
    /// No game matches.
    NotFound,
    /// Busy for now: wait and ask again.
    Retry(String),
    Failed(MetaError),
}

/// Sorts an answer. The codes are the API doc's; the texts are what the service sends, often
/// with a 200, in French.
pub(crate) fn classify(status: reqwest::StatusCode, body: &str) -> Answer {
    let short = || {
        format!("HTTP {}: {}", status.as_u16(), body.trim().chars().take(160).collect::<String>())
    };
    let lower = body.to_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|n| lower.contains(n));
    if status.is_success() && body.trim_start().starts_with('<') {
        return Answer::Xml;
    }
    match status.as_u16() {
        // Closed to guests, or the server is overloaded.
        401 | 429 => return Answer::Retry(short()),
        403 | 426 => return Answer::Failed(MetaError::Unauthorized(short())),
        423 => return Answer::Failed(MetaError::Unavailable(short())),
        430 | 431 => return Answer::Failed(MetaError::Quota(short())),
        _ => {}
    }
    if status.as_u16() == 404 || has(&["non trouvée", "non trouvee"]) {
        return Answer::NotFound;
    }
    if has(&["totalement fermé", "totalement ferme", "fully closed"]) {
        Answer::Failed(MetaError::Unavailable(short()))
    } else if has(&["blacklist"]) {
        Answer::Failed(MetaError::Unauthorized(short()))
    } else if has(&["quota"]) {
        Answer::Failed(MetaError::Quota(short()))
    } else if has(&["fermé pour les non membres", "closed for non", "maximum threads", "trop"]) {
        Answer::Retry(short())
    } else if has(&["identifiant", "login"]) {
        Answer::Failed(MetaError::Unauthorized(short()))
    } else {
        Answer::Failed(MetaError::Other(short()))
    }
}

/// [`parse`] with region and language preferences (most preferred first).
pub(crate) fn parse_with<S: AsRef<str>>(
    xml: &str,
    regions: &[S],
    languages: &[S],
) -> Option<GameInfo> {
    let doc = roxmltree::Document::parse(xml).ok()?;
    let jeu = doc.descendants().find(|n| {
        n.has_tag_name("jeu")
            && n.parent().is_some_and(|p| p.has_tag_name("Data") || p.has_tag_name("jeux"))
    })?;
    let child = |tag: &str| jeu.children().find(|c| c.has_tag_name(tag));
    let text = |tag: &str| {
        child(tag).and_then(|c| c.text()).map(|t| t.trim().to_owned()).filter(|t| !t.is_empty())
    };
    let list = |outer: &str, inner: &'static str| {
        child(outer)
            .into_iter()
            .flat_map(move |o| o.children().filter(move |c| c.has_tag_name(inner)))
    };

    let title = pick(list("noms", "nom"), "region", regions).map(|t| t.replace("&nbsp;", " "));
    if title.as_deref().is_some_and(|t| t.starts_with("ZZZ(notgame)")) {
        return None;
    }
    let genre = child("genres").and_then(|g| {
        let names: Vec<String> = g
            .children()
            .filter(|c| c.has_tag_name("genre"))
            .filter(|c| c.attribute("langue").is_none_or(|l| l == "en"))
            .filter_map(|c| c.text().map(|t| t.trim().to_owned()))
            .filter(|t| !t.is_empty())
            .collect();
        (!names.is_empty()).then(|| names.join(", "))
    });
    let rating =
        text("note").and_then(|n| n.parse::<f64>().ok()).map(|n| (n / 20.0).clamp(0.0, 1.0));
    let box_art = list("medias", "media")
        .filter(|m| m.attribute("type") == Some("box-2D"))
        .filter_map(|m| {
            Some((m.attribute("region").unwrap_or(""), m.text()?.trim().replace(' ', "%20")))
        })
        .collect::<Vec<_>>();
    let box_art_url = regions
        .iter()
        .find_map(|r| box_art.iter().find(|(reg, _)| *reg == r.as_ref()))
        .or_else(|| box_art.first())
        .map(|(_, url)| strip_credentials(url));

    Some(GameInfo {
        title,
        description: pick(list("synopsis", "synopsis"), "langue", languages),
        release_date: pick(list("dates", "date"), "region", regions),
        developer: text("developpeur"),
        publisher: text("editeur"),
        genre,
        players: text("joueurs"),
        rating,
        box_art_url,
    })
}

impl ScreenScraper {
    pub fn new(base: impl Into<String>, credentials: Credentials, http: reqwest::Client) -> Self {
        let own = |list: &[&str]| list.iter().map(|s| (*s).to_owned()).collect();
        Self {
            base: base.into(),
            credentials,
            http,
            regions: own(DEFAULT_REGIONS),
            languages: own(DEFAULT_LANGUAGES),
            delay: Duration::from_millis(1200),
            last: tokio::sync::Mutex::new(None),
            quota: Mutex::new(None),
        }
    }

    /// The pause between requests (ScreenScraper asks for about a second on free accounts).
    pub fn with_delay(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }

    /// Region and language preferences, most preferred first (empty keeps the defaults).
    pub fn with_preferences(mut self, regions: Vec<String>, languages: Vec<String>) -> Self {
        let clean = |v: Vec<String>| -> Vec<String> {
            v.into_iter().map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty()).collect()
        };
        let (regions, languages) = (clean(regions), clean(languages));
        if !regions.is_empty() {
            self.regions = regions;
        }
        if !languages.is_empty() {
            self.languages = languages;
        }
        self
    }

    /// A media URL a lookup stored, with this account's credentials added again.
    pub fn authorize(&self, url: &str) -> String {
        let Ok(mut parsed) = reqwest::Url::parse(&strip_credentials(url)) else {
            return url.to_owned();
        };
        {
            let c = &self.credentials;
            let mut query = parsed.query_pairs_mut();
            query.append_pair("devid", &c.dev_id).append_pair("devpassword", &c.dev_password);
            if let Some((user, password)) = &c.user {
                query.append_pair("ssid", user).append_pair("sspassword", password);
            }
        }
        parsed.into()
    }

    /// The query for `input`: by the game's id once known, else by the file.
    fn query(&self, system: u32, input: &LookupInput) -> Vec<(&'static str, String)> {
        let c = &self.credentials;
        let mut query: Vec<(&str, String)> = vec![
            ("devid", c.dev_id.clone()),
            ("devpassword", c.dev_password.clone()),
            ("softname", concat!("SovEmu ", env!("CARGO_PKG_VERSION")).to_owned()),
            ("output", "xml".to_owned()),
            ("systemeid", system.to_string()),
        ];
        if let Some(id) = input.known_ids.get("screenscraper") {
            query.push(("gameid", id.clone()));
        } else {
            query.push(("romtype", "rom".to_owned()));
            query.push(("romnom", input.file_name.clone()));
            query.push(("romtaille", input.size.to_string()));
            if let Some(md5) = &input.md5 {
                query.push(("md5", md5.clone()));
            }
        }
        if let Some((user, password)) = &c.user {
            query.push(("ssid", user.clone()));
            query.push(("sspassword", password.clone()));
        }
        query
    }

    /// One request, paced after the last one.
    async fn ask(
        &self,
        query: &[(&str, String)],
        progress: Progress<'_>,
    ) -> Result<(reqwest::StatusCode, String), MetaError> {
        let mut last = self.last.lock().await;
        if let Some(wait) = last.map(|t| self.delay.saturating_sub(t.elapsed()))
            && !wait.is_zero()
        {
            progress(Step::Waiting);
            tokio::time::sleep(wait).await;
        }
        progress(Step::Asking);
        let result = async {
            let response =
                self.http.get(format!("{}/jeuInfos.php", self.base)).query(query).send().await?;
            let status = response.status();
            Ok::<_, reqwest::Error>((status, response.text().await?))
        }
        .await;
        *last = Some(Instant::now());
        Ok(result?)
    }

    pub async fn lookup(
        &self,
        input: &LookupInput,
        progress: Progress<'_>,
    ) -> Result<Option<Found>, MetaError> {
        let Some(system) = system_id(&input.system) else { return Ok(None) };
        let query = self.query(system, input);
        let mut backoff = BACKOFF.iter();
        loop {
            let (status, body) = self.ask(&query, progress).await?;
            match classify(status, &body) {
                Answer::Xml => {
                    if let Some(q) = quota(&body) {
                        *self.quota.lock().unwrap_or_else(|e| e.into_inner()) = Some(q);
                    }
                    return Ok(parse_with(&body, &self.regions, &self.languages)
                        .map(|info| Found { info, id: game_id(&body) }));
                }
                Answer::NotFound => return Ok(None),
                Answer::Failed(e) => return Err(e),
                Answer::Retry(why) => match backoff.next() {
                    Some(wait) => {
                        tracing::debug!(wait_s = wait.as_secs(), reason = %why, "ScreenScraper busy");
                        progress(Step::Waiting);
                        tokio::time::sleep(*wait).await;
                    }
                    None => return Err(MetaError::Unavailable(why)),
                },
            }
        }
    }
}

impl Provider for ScreenScraper {
    fn id(&self) -> &'static str {
        "screenscraper"
    }

    fn name(&self) -> &'static str {
        "ScreenScraper"
    }

    fn wants_md5(&self) -> bool {
        true
    }

    fn lookup<'a>(
        &'a self,
        input: &'a LookupInput,
        progress: Progress<'a>,
    ) -> BoxFuture<'a, Result<Option<Found>, MetaError>> {
        Box::pin(ScreenScraper::lookup(self, input, progress))
    }

    fn quota(&self) -> Option<Quota> {
        *self.quota.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn box_art_url(&self, stored: &str) -> String {
        self.authorize(stored)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A ScreenScraper answer, shared with the server's tests.
    pub(crate) const SAMPLE: &str = include_str!("../../tests/fixtures/screenscraper.xml");

    #[test]
    fn parses_a_game_preferring_us_and_english() {
        let info = parse(SAMPLE).unwrap();
        assert_eq!(info.title.as_deref(), Some("Castlevania"));
        assert_eq!(info.description.as_deref(), Some("Simon Belmont sets out to defeat Dracula."));
        assert_eq!(info.release_date.as_deref(), Some("1987-05-01"));
        assert_eq!(info.genre.as_deref(), Some("Platform, Action"));
        assert_eq!(
            (info.developer.as_deref(), info.publisher.as_deref()),
            (Some("Konami"), Some("Konami"))
        );
        assert_eq!(info.players.as_deref(), Some("1"));
        assert_eq!(info.rating, Some(0.8), "ScreenScraper rates out of 20");
        assert_eq!(info.box_art_url.as_deref(), Some("https://example.invalid/box%20us.png"));
    }

    #[test]
    fn honours_region_and_language_preferences() {
        let info = parse_with(SAMPLE, &["jp"], &["fr"]).unwrap();
        assert_eq!(info.title.as_deref(), Some("Akumajou Dracula"));
        assert_eq!(info.release_date.as_deref(), Some("1986-09-26"));
        assert_eq!(info.description.as_deref(), Some("Simon Belmont part affronter Dracula."));
        assert_eq!(info.box_art_url.as_deref(), Some("https://example.invalid/box%20jp.png"));
    }

    #[test]
    fn no_game_and_not_a_game() {
        assert!(parse("<Data><ssuser/></Data>").is_none());
        assert!(
            parse(
                r#"<Data><jeu><noms><nom region="wor">ZZZ(notgame) BIOS</nom></noms></jeu></Data>"#
            )
            .is_none()
        );
        assert!(parse("not xml").is_none());
    }

    #[test]
    fn media_urls_are_stored_without_credentials() {
        let url = "https://neoclone.screenscraper.fr/api2/mediaJeu.php?devid=dev&devpassword=pw\
                   &softname=SovEmu&ssid=me&sspassword=secret&systemeid=3&jeuid=1234\
                   &media=box-2D(us)";
        let stored = strip_credentials(url);
        assert!(!stored.contains("pw") && !stored.contains("secret") && !stored.contains("ssid"));
        assert!(stored.contains("jeuid=1234") && stored.contains("softname=SovEmu"), "{stored}");
        assert_eq!(
            strip_credentials("https://x.invalid/box%20us.png"),
            "https://x.invalid/box%20us.png"
        );

        let ss = ScreenScraper::new(
            DEFAULT_BASE,
            Credentials {
                dev_id: "dev2".into(),
                dev_password: "pw2".into(),
                user: Some(("me2".into(), "secret2".into())),
            },
            reqwest::Client::new(),
        );
        let signed = reqwest::Url::parse(&ss.authorize(&stored)).unwrap();
        let q: std::collections::HashMap<_, _> = signed.query_pairs().into_owned().collect();
        assert_eq!(q["devid"], "dev2");
        assert_eq!(q["devpassword"], "pw2");
        assert_eq!(q["ssid"], "me2");
        assert_eq!(q["sspassword"], "secret2");
        assert_eq!(q["jeuid"], "1234");
        assert_eq!(q["media"], "box-2D(us)");
    }

    #[test]
    fn maps_every_system() {
        assert_eq!(system_id("snes"), Some(4));
        assert_eq!(system_id("psx"), Some(57));
        assert_eq!(system_id("nope"), None);
        for system in crate::systems::SYSTEMS {
            assert!(system_id(system.id).is_some(), "{} has no ScreenScraper id", system.id);
        }
    }

    #[test]
    fn reads_the_game_id_and_the_quota() {
        assert_eq!(game_id(SAMPLE).as_deref(), Some("1234"));
        assert_eq!(game_id("<Data><jeu id=\"0\"/></Data>"), None);
        assert_eq!(
            quota(SAMPLE),
            Some(Quota {
                requests_today: 3,
                max_requests_per_day: Some(20000),
                misses_today: 0,
                max_misses_per_day: None
            })
        );
        assert_eq!(quota("<Data><jeu/></Data>"), None);
    }

    #[test]
    fn sorts_answers_by_status_and_text() {
        use reqwest::StatusCode;
        let code = |c| StatusCode::from_u16(c).unwrap();
        assert_eq!(classify(StatusCode::OK, "<Data/>"), Answer::Xml);
        assert_eq!(
            classify(StatusCode::NOT_FOUND, "Erreur : Rom/Iso/Dossier non trouvée !"),
            Answer::NotFound
        );
        assert_eq!(classify(StatusCode::OK, "Erreur : Jeu non trouvée !"), Answer::NotFound);
        assert!(matches!(classify(code(401), "API fermé pour les non membres"), Answer::Retry(_)));
        assert!(matches!(classify(code(429), ""), Answer::Retry(_)));
        assert!(matches!(
            classify(StatusCode::OK, "Le nombre maximum threads est atteint"),
            Answer::Retry(_)
        ));
        assert!(matches!(
            classify(code(403), "Erreur de login"),
            Answer::Failed(MetaError::Unauthorized(_))
        ));
        assert!(matches!(
            classify(StatusCode::OK, "Le logiciel de scrape utilisé a été blacklisté"),
            Answer::Failed(MetaError::Unauthorized(_))
        ));
        assert!(matches!(
            classify(code(423), "API totalement fermé"),
            Answer::Failed(MetaError::Unavailable(_))
        ));
        assert!(matches!(
            classify(code(430), "Votre quota de scrape est dépassé"),
            Answer::Failed(MetaError::Quota(_))
        ));
        assert!(matches!(classify(code(431), ""), Answer::Failed(MetaError::Quota(_))));
        assert!(matches!(
            classify(StatusCode::BAD_GATEWAY, "<html>"),
            Answer::Failed(MetaError::Other(_))
        ));
    }

    #[test]
    fn asks_by_game_id_once_known() {
        let ss = ScreenScraper::new(
            DEFAULT_BASE,
            Credentials { dev_id: "d".into(), dev_password: "p".into(), user: None },
            reqwest::Client::new(),
        );
        let mut input = LookupInput {
            system: "nes".into(),
            title: "Castlevania".into(),
            file_name: "Castlevania (USA).nes".into(),
            size: 131088,
            md5: Some("abc".into()),
            known_ids: Default::default(),
        };
        let keys =
            |q: Vec<(&'static str, String)>| q.into_iter().map(|(k, _)| k).collect::<Vec<_>>();
        let by_file = keys(ss.query(3, &input));
        assert!(by_file.contains(&"md5") && by_file.contains(&"romnom"), "{by_file:?}");
        assert!(!by_file.contains(&"gameid"));
        input.known_ids.insert("screenscraper".into(), "1234".into());
        let by_id = keys(ss.query(3, &input));
        assert!(by_id.contains(&"gameid") && !by_id.contains(&"md5"), "{by_id:?}");
    }
}
