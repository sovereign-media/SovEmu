//! ScreenScraper (screenscraper.fr): the database retro frontends scrape from.
//!
//! `jeuInfos.php` identifies a game by MD5 + size + file name within a system and answers in
//! XML (the shape ES-DE parses). Requests carry developer credentials (`devid`/`devpassword`,
//! requested from ScreenScraper) and optionally a user account (`ssid`/`sspassword`), which raises
//! the daily quota and thread count.

use super::{GameInfo, LookupInput, MetaError, status_error};

pub const DEFAULT_BASE: &str = "https://api.screenscraper.fr/api2";

/// SovEmu system id → ScreenScraper `systemeid`.
pub fn system_id(system: &str) -> Option<u32> {
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
}

/// Region preference for names, dates and media: USA first, as the library's sets mostly are.
const REGIONS: &[&str] = &["us", "wor", "eu", "ss", "uk", "jp"];
const LANGUAGES: &[&str] = &["en", "wor"];

fn pick<'a>(
    nodes: impl Iterator<Item = roxmltree::Node<'a, 'a>>,
    attr: &str,
    prefs: &[&str],
) -> Option<String> {
    let all: Vec<_> =
        nodes.filter_map(|n| Some((n.attribute(attr).unwrap_or(""), n.text()?.trim()))).collect();
    prefs
        .iter()
        .find_map(|p| all.iter().find(|(a, _)| a == p))
        .or_else(|| all.first())
        .map(|(_, t)| (*t).to_owned())
        .filter(|t| !t.is_empty())
}

/// Parses a `jeuInfos.php` XML response. `None` when it holds no game.
pub fn parse(xml: &str) -> Option<GameInfo> {
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

    let title = pick(list("noms", "nom"), "region", REGIONS).map(|t| t.replace("&nbsp;", " "));
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
    let box_art_url = REGIONS
        .iter()
        .find_map(|r| box_art.iter().find(|(reg, _)| reg == r))
        .or_else(|| box_art.first())
        .map(|(_, url)| url.clone());

    Some(GameInfo {
        title,
        description: pick(list("synopsis", "synopsis"), "langue", LANGUAGES),
        release_date: pick(list("dates", "date"), "region", REGIONS),
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
        Self { base: base.into(), credentials, http }
    }

    pub async fn lookup(&self, input: &LookupInput) -> Result<Option<GameInfo>, MetaError> {
        let Some(system) = system_id(&input.system) else { return Ok(None) };
        let c = &self.credentials;
        let mut query: Vec<(&str, String)> = vec![
            ("devid", c.dev_id.clone()),
            ("devpassword", c.dev_password.clone()),
            ("softname", concat!("SovEmu ", env!("CARGO_PKG_VERSION")).to_owned()),
            ("output", "xml".to_owned()),
            ("romtype", "rom".to_owned()),
            ("systemeid", system.to_string()),
            ("romnom", input.file_name.clone()),
            ("romtaille", input.size.to_string()),
        ];
        if let Some(md5) = &input.md5 {
            query.push(("md5", md5.clone()));
        }
        if let Some((user, password)) = &c.user {
            query.push(("ssid", user.clone()));
            query.push(("sspassword", password.clone()));
        }
        let response =
            self.http.get(format!("{}/jeuInfos.php", self.base)).query(&query).send().await?;
        let status = response.status();
        let body = response.text().await?;
        if status == reqwest::StatusCode::NOT_FOUND || body.starts_with("Erreur") {
            return Ok(None);
        }
        if !status.is_success() {
            return Err(status_error("ScreenScraper", status, &body));
        }
        // Some quota and maintenance messages arrive as plain text with a 200.
        if !body.trim_start().starts_with('<') {
            let lower = body.to_lowercase();
            return Err(
                if lower.contains("quota") || lower.contains("maximum") || lower.contains("trop") {
                    MetaError::Quota(body.chars().take(160).collect())
                } else if lower.contains("identifiant") || lower.contains("login") {
                    MetaError::Unauthorized(body.chars().take(160).collect())
                } else {
                    MetaError::Other(body.chars().take(160).collect())
                },
            );
        }
        Ok(parse(&body))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub const SAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Data>
  <ssuser><id>someone</id><niveau>1</niveau><requeststoday>3</requeststoday><maxrequestsperday>20000</maxrequestsperday></ssuser>
  <jeu id="1234">
    <noms>
      <nom region="jp">Akumajou Dracula</nom>
      <nom region="us">Castlevania</nom>
    </noms>
    <systeme id="3" parentid="0">NES</systeme>
    <editeur id="1">Konami</editeur>
    <developpeur id="1">Konami</developpeur>
    <joueurs>1</joueurs>
    <note>16</note>
    <synopsis>
      <synopsis langue="fr">Simon Belmont part affronter Dracula.</synopsis>
      <synopsis langue="en">Simon Belmont sets out to defeat Dracula.</synopsis>
    </synopsis>
    <dates><date region="jp">1986-09-26</date><date region="us">1987-05-01</date></dates>
    <genres>
      <genre id="1" langue="fr">Plateforme</genre>
      <genre id="1" langue="en">Platform</genre>
      <genre id="2" langue="en">Action</genre>
    </genres>
    <medias>
      <media type="ss" region="wor" format="png">https://example.invalid/ss.png</media>
      <media type="box-2D" region="jp" format="png">https://example.invalid/box jp.png</media>
      <media type="box-2D" region="us" format="png">https://example.invalid/box us.png</media>
    </medias>
  </jeu>
</Data>"#;

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
    fn maps_systems() {
        assert_eq!(system_id("snes"), Some(4));
        assert_eq!(system_id("psx"), Some(57));
        assert_eq!(system_id("n64"), None);
    }
}
