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
//! Every answer carries the account (`ssuser`): its level, its limits and today's counts, kept
//! for [`Provider::account`] and [`Provider::quota`]. `ssuserInfos.php` reads it without
//! spending a lookup ([`Provider::prepare`]). The account's `maxthreads` sizes the pool of
//! requests open at once (capped by `with_threads`), and its `maxrequestspermin` spaces their
//! starts (`with_delay` when it gives none). Each answer also carries the game's own id
//! (`jeu id`), which the server stores so a later lookup asks for that game (`gameid`) instead of
//! matching the file again. A busy or closed-to-guests server is backed off from.
//!
//! ScreenScraper reports errors in French, often as text with a 200: [`classify`] sorts them by
//! status and text, and [`MESSAGES`] gives each its English words (the API doc's table, and the
//! texts other scrapers match), so an admin never sees the French.

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use super::{
    Account, BoxFuture, Found, GameInfo, LookupInput, MetaError, Progress, Provider, Quota, Step,
};

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
    /// The pause between request starts when the account gives no per-minute limit.
    delay: Duration,
    /// Requests open at once: at most this many (0: as many as the account allows).
    thread_cap: usize,
    /// One permit per request the account may have open; resized as the account is learnt.
    slots: tokio::sync::Semaphore,
    slot_count: AtomicUsize,
    /// When the next request may start; each request reserves its start in turn.
    next_start: tokio::sync::Mutex<Option<Instant>>,
    quota: Mutex<Option<Quota>>,
    account: Mutex<Option<Account>>,
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

/// The account from an answer's `ssuser` block.
pub(crate) fn account(xml: &str) -> Option<Account> {
    let doc = roxmltree::Document::parse(xml).ok()?;
    let user = doc.descendants().find(|n| n.has_tag_name("ssuser"))?;
    let text = |tag: &str| {
        user.children()
            .find(|c| c.has_tag_name(tag))
            .and_then(|c| c.text())
            .map(str::trim)
            .filter(|t| !t.is_empty())
    };
    let number = |tag: &str| text(tag).and_then(|t| t.parse::<u64>().ok());
    let positive = |tag: &str| number(tag).filter(|n| *n > 0);
    Some(Account {
        user: text("id").map(str::to_owned),
        level: number("niveau").map(|n| n as u32),
        contribution: number("contribution").map(|n| n as u32),
        max_threads: positive("maxthreads").map(|n| n as u32),
        // The doc spells it both ways.
        max_requests_per_minute: positive("maxrequestspermin")
            .or_else(|| positive("maxrequestsperdmin"))
            .map(|n| n as u32),
        max_requests_per_day: positive("maxrequestsperday"),
        max_misses_per_day: positive("maxrequestskoperday"),
        max_download_kbps: positive("maxdownloadspeed").map(|n| n as u32),
    })
}

/// What a kind of answer means for the lookup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    NotFound,
    /// Busy: wait and ask again.
    Retry,
    Unauthorized,
    Unavailable,
    Quota,
    /// The request was wrong: this game fails, the job goes on.
    Other,
}

/// ScreenScraper's messages, by a piece of their French text (lower case, matched with and
/// without accents), with what they mean and their English words. From the API doc's error
/// table (webapi2.php) and the texts Skyscraper, ES-DE and Batocera match; most specific first.
pub(crate) const MESSAGES: &[(&[&str], Kind, &str)] = &[
    (&["rom/iso/dossier non trouv"], Kind::NotFound, "No ROM, ISO or folder matches this file."),
    (&["jeu non trouv"], Kind::NotFound, "No game matches."),
    (&["non trouv"], Kind::NotFound, "Not found."),
    (&["contient un chemin d'acc"], Kind::Other, "The ROM file name contains a path."),
    (&["champ crc, md5 ou sha1 erron"], Kind::Other, "The CRC, MD5 or SHA-1 field is malformed."),
    (&["nom du fichier rom"], Kind::Other, "The ROM file name is not valid."),
    (
        &["il manque des champs obligatoires", "il manque"],
        Kind::Other,
        "Required fields are missing from the request.",
    ),
    (&["problème avec l'url", "probleme avec l'url"], Kind::Other, "The request is empty."),
    (
        &["identifiants développeur", "identifiants developpeur"],
        Kind::Unauthorized,
        "Login error: the developer credentials were refused (SOVEMU_SCREENSCRAPER_DEVID, \
         SOVEMU_SCREENSCRAPER_DEVPASSWORD).",
    ),
    (&["erreur de login"], Kind::Unauthorized, "Login error: the credentials were refused."),
    (
        &["blacklist"],
        Kind::Unauthorized,
        "This software has been blacklisted by ScreenScraper (not compliant, or an outdated \
         version).",
    ),
    (
        &["totalement fermé", "totalement ferme"],
        Kind::Unavailable,
        "The API is closed: the server has serious problems.",
    ),
    (
        &["fermé pour les non membres", "ferme pour les non membres", "closed for non"],
        Kind::Retry,
        "The API is closed to non-members and inactive members for now: the server is busy.",
    ),
    (
        &["threads par minute", "threads per minute"],
        Kind::Retry,
        "The account's threads per minute are used up: requests must slow down.",
    ),
    (&["leecher"], Kind::Retry, "The threads allowed to non-contributing users are all in use."),
    (
        &["nombre de threads autoris", "maximum threads"],
        Kind::Retry,
        "The account's threads are all in use: requests must slow down.",
    ),
    (
        &["votre quota de scrape", "quota"],
        Kind::Quota,
        "The daily scrape quota is used up; it starts again tomorrow.",
    ),
    (
        &["faite du tri", "faites du tri"],
        Kind::Quota,
        "Too many unrecognised ROMs today; ScreenScraper asks to tidy the files and try again \
         tomorrow.",
    ),
];

/// What a status means when its text says nothing more (the API doc's table).
fn by_status(code: u16) -> Option<(Kind, &'static str)> {
    Some(match code {
        400 => (Kind::Other, "The request was malformed."),
        401 => (Kind::Retry, "The API is closed to non-members for now: the server is busy."),
        403 => (Kind::Unauthorized, "Login error: the developer credentials were refused."),
        404 => (Kind::NotFound, "Not found."),
        423 => (Kind::Unavailable, "The API is closed: the server has serious problems."),
        426 => (Kind::Unauthorized, "This software has been blacklisted by ScreenScraper."),
        429 => (Kind::Retry, "The account's threads are all in use: requests must slow down."),
        430 => (Kind::Quota, "The daily scrape quota is used up; it starts again tomorrow."),
        431 => (Kind::Quota, "Too many unrecognised ROMs today; try again tomorrow."),
        _ => return None,
    })
}

/// The kind and English words of a message, from its text, else its status.
pub(crate) fn translate(status: u16, body: &str) -> (Kind, String) {
    let lower = body.to_lowercase();
    if let Some((_, kind, english)) =
        MESSAGES.iter().find(|(needles, _, _)| needles.iter().any(|n| lower.contains(n)))
    {
        return (*kind, (*english).to_owned());
    }
    if let Some((kind, english)) = by_status(status) {
        return (kind, english.to_owned());
    }
    let shown: String = body.trim().chars().take(120).collect();
    let english = if shown.starts_with('<') || shown.is_empty() {
        format!("Unexpected answer (HTTP {status}).")
    } else {
        format!("Unexpected answer (HTTP {status}): {shown}")
    };
    (Kind::Other, english)
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

/// Sorts an answer, in English: by its text when ScreenScraper sent one it is known to send
/// (often with a 200), else by its status.
pub(crate) fn classify(status: reqwest::StatusCode, body: &str) -> Answer {
    if status.is_success() && body.trim_start().starts_with('<') {
        return Answer::Xml;
    }
    let (kind, english) = translate(status.as_u16(), body);
    if kind == Kind::Other && status.is_success() {
        // A 200 with text no table knows: not an error ScreenScraper reports, so a failure.
        return Answer::Failed(MetaError::Other(english));
    }
    if !body.trim().is_empty() && !body.trim_start().starts_with('<') {
        tracing::debug!(status = status.as_u16(), text = %body.trim().chars().take(200).collect::<String>(), "ScreenScraper said");
    }
    match kind {
        Kind::NotFound => Answer::NotFound,
        Kind::Retry => Answer::Retry(english),
        Kind::Unauthorized => Answer::Failed(MetaError::Unauthorized(english)),
        Kind::Unavailable => Answer::Failed(MetaError::Unavailable(english)),
        Kind::Quota => Answer::Failed(MetaError::Quota(english)),
        Kind::Other => Answer::Failed(MetaError::Other(english)),
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
            thread_cap: 0,
            slots: tokio::sync::Semaphore::new(1),
            slot_count: AtomicUsize::new(1),
            next_start: tokio::sync::Mutex::new(None),
            quota: Mutex::new(None),
            account: Mutex::new(None),
        }
    }

    /// The pause between request starts when the account gives no per-minute limit
    /// (ScreenScraper asks for about a second on free accounts).
    pub fn with_delay(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }

    /// At most this many requests open at once, below what the account allows (0: no cap).
    pub fn with_threads(mut self, cap: usize) -> Self {
        self.thread_cap = cap;
        self
    }

    /// Requests open at once: the account's threads (one until it is known), capped.
    fn threads(&self) -> usize {
        let allowed = self.account().and_then(|a| a.max_threads).unwrap_or(1).max(1) as usize;
        match self.thread_cap {
            0 => allowed,
            cap => allowed.min(cap),
        }
    }

    /// The pause between request starts: the account's per-minute limit spread evenly, else
    /// `with_delay`'s.
    fn interval(&self) -> Duration {
        match self.account().and_then(|a| a.max_requests_per_minute) {
            Some(n) if n > 0 => Duration::from_secs(60) / n,
            _ => self.delay,
        }
    }

    /// Keeps the account and quota an answer carries, and resizes the request pool to fit.
    fn learn(&self, xml: &str) {
        if let Some(q) = quota(xml) {
            *self.quota.lock().unwrap_or_else(|e| e.into_inner()) = Some(q);
        }
        let Some(new) = account(xml) else { return };
        {
            // Answers carry more or less of the account: keep what an answer leaves out.
            let mut known = self.account.lock().unwrap_or_else(|e| e.into_inner());
            let old = known.take().unwrap_or_default();
            *known = Some(Account {
                user: new.user.or(old.user),
                level: new.level.or(old.level),
                contribution: new.contribution.or(old.contribution),
                max_threads: new.max_threads.or(old.max_threads),
                max_requests_per_minute: new
                    .max_requests_per_minute
                    .or(old.max_requests_per_minute),
                max_requests_per_day: new.max_requests_per_day.or(old.max_requests_per_day),
                max_misses_per_day: new.max_misses_per_day.or(old.max_misses_per_day),
                max_download_kbps: new.max_download_kbps.or(old.max_download_kbps),
            });
        }
        let want = self.threads();
        let have = self.slot_count.swap(want, Ordering::SeqCst);
        if want > have {
            self.slots.add_permits(want - have);
        } else if want < have {
            // Permits in use now are not forgotten: the pool shrinks as they come back.
            let gone = self.slots.forget_permits(have - want);
            if gone < have - want {
                self.slot_count.fetch_add(have - want - gone, Ordering::SeqCst);
            }
        }
    }

    /// The credentials and software name every request carries.
    fn identity(&self) -> Vec<(&'static str, String)> {
        let c = &self.credentials;
        let mut query: Vec<(&str, String)> = vec![
            ("devid", c.dev_id.clone()),
            ("devpassword", c.dev_password.clone()),
            ("softname", concat!("SovEmu ", env!("CARGO_PKG_VERSION")).to_owned()),
            ("output", "xml".to_owned()),
        ];
        if let Some((user, password)) = &c.user {
            query.push(("ssid", user.clone()));
            query.push(("sspassword", password.clone()));
        }
        query
    }

    /// Reads the account (`ssuserInfos.php`), which spends no lookup. A user account that
    /// ScreenScraper does not accept (level 0) is noted: lookups go on as a guest.
    async fn check_account(&self) -> Result<Vec<String>, MetaError> {
        let response = self
            .http
            .get(format!("{}/ssuserInfos.php", self.base))
            .query(&self.identity())
            .send()
            .await?;
        let status = response.status();
        let body = response.text().await?;
        match classify(status, &body) {
            Answer::Xml => {
                self.learn(&body);
                let mut notes = Vec::new();
                if self.credentials.user.is_some()
                    && self.account().is_some_and(|a| a.level == Some(0))
                {
                    notes.push(
                        "ScreenScraper did not accept the user account (SOVEMU_SCREENSCRAPER_USER, \
                         SOVEMU_SCREENSCRAPER_PASSWORD): looking games up as a guest, with a \
                         guest's limits"
                            .to_owned(),
                    );
                }
                Ok(notes)
            }
            // Not every server answers it (mirrors, older ones): the first lookup tells instead.
            Answer::NotFound | Answer::Retry(_) | Answer::Failed(MetaError::Other(_)) => {
                Ok(Vec::new())
            }
            Answer::Failed(e) => Err(e),
        }
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
        let mut query = self.identity();
        query.push(("systemeid", system.to_string()));
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
        query
    }

    /// One request: in one of the account's slots, started no sooner than the pacing allows.
    async fn ask(
        &self,
        query: &[(&str, String)],
        progress: Progress<'_>,
    ) -> Result<(reqwest::StatusCode, String), MetaError> {
        progress(Step::Waiting);
        let _slot = self.slots.acquire().await.map_err(|e| MetaError::Other(e.to_string()))?;
        // Reserve a start time, then wait for it outside the lock, so several slots can wait
        // for their turns at once.
        let start = {
            let mut next = self.next_start.lock().await;
            let now = Instant::now();
            let start = next.map_or(now, |t| t.max(now));
            *next = Some(start + self.interval());
            start
        };
        tokio::time::sleep_until(start.into()).await;
        progress(Step::Asking);
        let response =
            self.http.get(format!("{}/jeuInfos.php", self.base)).query(query).send().await?;
        let status = response.status();
        Ok((status, response.text().await?))
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
                    self.learn(&body);
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

    fn prepare(&self) -> BoxFuture<'_, Result<Vec<String>, MetaError>> {
        Box::pin(self.check_account())
    }

    fn account(&self) -> Option<Account> {
        self.account.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn concurrency(&self) -> usize {
        self.threads()
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
    fn every_documented_message_is_understood_in_english() {
        // The API doc's error table (webapi2.php), verbatim, and the texts other scrapers match.
        let cases: &[(u16, &str, Kind)] = &[
            (400, "problème avec l'url", Kind::Other),
            (400, "Il manque des champs obligatoires dans l'url", Kind::Other),
            (
                400,
                "Erreur dans le nom du fichier rom : celui-ci contient un chemin d'accés",
                Kind::Other,
            ),
            (400, "Champ crc, md5 ou sha1 erroné", Kind::Other),
            (400, "Problème dans le nom du fichier rom", Kind::Other),
            (401, "API fermé pour les non membres ou les membres inactifs", Kind::Retry),
            (200, "API closed for non-registered members", Kind::Retry),
            (403, "Erreur de login : Vérifier vos identifiants développeur !", Kind::Unauthorized),
            (404, "Erreur : Jeu non trouvée !", Kind::NotFound),
            (404, "Erreur : Rom/Iso/Dossier non trouvée !", Kind::NotFound),
            (423, "API totalement fermé", Kind::Unavailable),
            (
                426,
                "Le logiciel de scrape utilisé a été blacklisté (non conforme / version obsolète)",
                Kind::Unauthorized,
            ),
            (429, "Le nombre de threads autorisé pour le membre est atteint", Kind::Retry),
            (
                429,
                "Le nombre de threads par minute autorisé pour le membre est atteint",
                Kind::Retry,
            ),
            (429, "The maximum threads allowed to leecher users is already used", Kind::Retry),
            (429, "The maximum threads is already used", Kind::Retry),
            (430, "Votre quota de scrape est dépassé pour aujourd'hui !", Kind::Quota),
            (431, "Faite du tri dans vos fichiers roms et repassez demain !", Kind::Quota),
        ];
        let french = ["é", "è", "erreur", "fermé", "dépassé", "trouvée", "vos ", " du "];
        for (status, text, kind) in cases {
            let (got, english) = translate(*status, text);
            assert_eq!(got, *kind, "{text}");
            let lower = english.to_lowercase();
            assert!(!french.iter().any(|f| lower.contains(f)), "{text} → {english}");
            // The same text with a 200 means the same (ScreenScraper often sends one).
            assert_eq!(translate(200, text).0, *kind, "{text} with a 200");
        }
        // Bare statuses, and text no table knows.
        assert_eq!(translate(430, "").0, Kind::Quota);
        assert_eq!(translate(429, "").0, Kind::Retry);
        assert_eq!(translate(502, "<html>").1, "Unexpected answer (HTTP 502).");
    }

    #[test]
    fn reads_the_account_and_sizes_the_pool_from_it() {
        let xml = "<Data><ssuser><id>someone</id><niveau>1</niveau><contribution>2</contribution>\
                   <maxthreads>2</maxthreads><maxdownloadspeed>128</maxdownloadspeed>\
                   <requeststoday>5</requeststoday><requestskotoday>1</requestskotoday>\
                   <maxrequestsperdmin>40</maxrequestsperdmin>\
                   <maxrequestsperday>20000</maxrequestsperday>\
                   <maxrequestskoperday>2000</maxrequestskoperday></ssuser></Data>";
        assert_eq!(
            account(xml),
            Some(Account {
                user: Some("someone".into()),
                level: Some(1),
                contribution: Some(2),
                max_threads: Some(2),
                max_requests_per_minute: Some(40),
                max_requests_per_day: Some(20000),
                max_misses_per_day: Some(2000),
                max_download_kbps: Some(128),
            })
        );
        let ss = || {
            ScreenScraper::new(
                DEFAULT_BASE,
                Credentials { dev_id: "d".into(), dev_password: "p".into(), user: None },
                reqwest::Client::new(),
            )
            .with_delay(Duration::from_millis(1200))
        };
        let fresh = ss();
        assert_eq!((fresh.concurrency(), fresh.interval()), (1, Duration::from_millis(1200)));
        let learnt = ss();
        learnt.learn(xml);
        assert_eq!(learnt.concurrency(), 2, "the account's threads");
        assert_eq!(learnt.slots.available_permits(), 2);
        assert_eq!(learnt.interval(), Duration::from_millis(1500), "40 a minute, evenly");
        assert_eq!(learnt.quota().unwrap().misses_today, 1);
        // A later answer with less of the account keeps what was learnt.
        learnt.learn("<Data><ssuser><requeststoday>6</requeststoday></ssuser></Data>");
        assert_eq!(learnt.concurrency(), 2);
        assert_eq!(learnt.account().unwrap().level, Some(1));
        let capped = ss().with_threads(1);
        capped.learn(xml);
        assert_eq!(capped.concurrency(), 1, "capped by the setting");
        assert_eq!(capped.slots.available_permits(), 1);
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
