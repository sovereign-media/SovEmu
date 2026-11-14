//! TheGamesDB (thegamesdb.net), by game name within a platform.
//!
//! `GET /v1/Games/ByGameName` returns candidates with developer, publisher and genre *ids*;
//! the names come from `/Developers`, `/Publishers` and `/Genres`, fetched once and kept in
//! memory. Box art comes from `/Games/Images` (`base_url.large` + the front boxart's filename).

use std::collections::HashMap;

use serde_json::Value;
use tokio::sync::OnceCell;

use super::{GameInfo, LookupInput, MetaError, normalize, status_error};

pub const DEFAULT_BASE: &str = "https://api.thegamesdb.net/v1";

/// SovEmu system id → TheGamesDB platform id.
pub fn platform_id(system: &str) -> Option<u32> {
    Some(match system {
        "nes" => 7,
        "snes" => 6,
        "gb" => 4,
        "gbc" => 41,
        "gba" => 5,
        // TheGamesDB lists Mega Drive (36) and Genesis (18) separately; search both.
        "megadrive" => 18,
        "mastersystem" => 35,
        "gamegear" => 20,
        "psx" => 10,
        "neogeo" => 24,
        "fbneo" => 23,
        _ => return None,
    })
}

fn platform_filter(system: &str) -> Option<String> {
    let id = platform_id(system)?;
    Some(if system == "megadrive" { format!("{id},36") } else { id.to_string() })
}

#[derive(Debug, Default)]
struct Names {
    developers: HashMap<i64, String>,
    publishers: HashMap<i64, String>,
    genres: HashMap<i64, String>,
}

#[derive(Debug)]
pub struct TheGamesDb {
    base: String,
    api_key: String,
    http: reqwest::Client,
    names: OnceCell<Names>,
}

/// The best candidate for `title` among `data.games`: an exact (normalised) title match, else
/// one whose title contains the other, else none (a wrong match is worse than no match).
pub fn best_match<'a>(games: &'a [Value], title: &str) -> Option<&'a Value> {
    let want = normalize(title);
    let name = |g: &Value| normalize(g["game_title"].as_str().unwrap_or(""));
    games.iter().find(|g| name(g) == want).or_else(|| {
        games
            .iter()
            .find(|g| !want.is_empty() && (name(g).contains(&want) || want.contains(&name(g))))
    })
}

fn join_names(ids: &Value, names: &HashMap<i64, String>) -> Option<String> {
    let joined: Vec<&str> = ids
        .as_array()?
        .iter()
        .filter_map(|i| names.get(&i.as_i64()?))
        .map(String::as_str)
        .collect();
    (!joined.is_empty()).then(|| joined.join(", "))
}

/// Converts one game from `data.games` (names resolved through `names`).
fn to_info(game: &Value, names: &Names) -> GameInfo {
    let s = |k: &str| {
        game[k].as_str().map(|t| t.replace('\r', "").trim().to_owned()).filter(|t| !t.is_empty())
    };
    GameInfo {
        title: s("game_title"),
        description: s("overview"),
        release_date: s("release_date"),
        developer: join_names(&game["developers"], &names.developers),
        publisher: join_names(&game["publishers"], &names.publishers),
        genre: join_names(&game["genres"], &names.genres),
        players: game["players"].as_i64().map(|p| p.to_string()),
        rating: None,
        box_art_url: None,
    }
}

/// `base_url.large` + the front boxart's filename, from a `/Games/Images` response.
pub fn front_box_art(images: &Value, game_id: i64) -> Option<String> {
    let base = images["data"]["base_url"]["large"].as_str()?;
    let entries = images["data"]["images"][game_id.to_string()].as_array()?;
    let front = entries.iter().find(|e| e["type"] == "boxart" && e["side"] == "front")?;
    Some(format!("{base}{}", front["filename"].as_str()?))
}

impl TheGamesDb {
    pub fn new(base: impl Into<String>, api_key: String, http: reqwest::Client) -> Self {
        Self { base: base.into(), api_key, http, names: OnceCell::new() }
    }

    async fn get(&self, path: &str, query: &[(&str, String)]) -> Result<Value, MetaError> {
        let response = self
            .http
            .get(format!("{}{path}", self.base))
            .query(&[("apikey", self.api_key.as_str())])
            .query(query)
            .send()
            .await?;
        let status = response.status();
        let body = response.text().await?;
        if !status.is_success() {
            return Err(status_error("TheGamesDB", status, &body));
        }
        serde_json::from_str(&body)
            .map_err(|e| MetaError::Other(format!("TheGamesDB: bad JSON: {e}")))
    }

    async fn names(&self) -> &Names {
        self.names
            .get_or_init(|| async {
                let mut names = Names::default();
                for (path, key, into) in [
                    ("/Developers", "developers", &mut names.developers),
                    ("/Publishers", "publishers", &mut names.publishers),
                    ("/Genres", "genres", &mut names.genres),
                ] {
                    if let Ok(v) = self.get(path, &[]).await
                        && let Some(entries) = v["data"][key].as_object()
                    {
                        into.extend(entries.values().filter_map(|e| {
                            Some((e["id"].as_i64()?, e["name"].as_str()?.to_owned()))
                        }));
                    }
                }
                names
            })
            .await
    }

    pub async fn lookup(&self, input: &LookupInput) -> Result<Option<GameInfo>, MetaError> {
        let Some(platforms) = platform_filter(&input.system) else { return Ok(None) };
        let found = self
            .get(
                "/Games/ByGameName",
                &[
                    ("name", input.title.clone()),
                    ("filter[platform]", platforms),
                    ("fields", "players,publishers,genres,overview,platform".to_owned()),
                ],
            )
            .await?;
        let games = found["data"]["games"].as_array().cloned().unwrap_or_default();
        let Some(game) = best_match(&games, &input.title) else { return Ok(None) };
        let mut info = to_info(game, self.names().await);
        if let Some(id) = game["id"].as_i64()
            && let Ok(images) = self.get("/Games/Images", &[("games_id", id.to_string())]).await
        {
            info.box_art_url = front_box_art(&images, id);
        }
        Ok(Some(info))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn picks_the_matching_game_and_resolves_names() {
        let games = json!([
            {"id": 1, "game_title": "Super Mario Bros. 3: Mario Remix"},
            {"id": 2, "game_title": "Super Mario Bros. 3", "overview": "Mario returns.\r\n",
             "release_date": "1990-02-12", "players": 2, "developers": [10], "publishers": [20], "genres": [1, 99]}
        ]);
        let games = games.as_array().unwrap();
        let game = best_match(games, "Super Mario Bros 3").unwrap();
        assert_eq!(game["id"], 2);
        let names = Names {
            developers: HashMap::from([(10, "Nintendo EAD".to_owned())]),
            publishers: HashMap::from([(20, "Nintendo".to_owned())]),
            genres: HashMap::from([(1, "Platform".to_owned())]),
        };
        let info = to_info(game, &names);
        assert_eq!(info.description.as_deref(), Some("Mario returns."));
        assert_eq!(info.developer.as_deref(), Some("Nintendo EAD"));
        assert_eq!(info.genre.as_deref(), Some("Platform"), "unknown ids are skipped");
        assert_eq!(info.players.as_deref(), Some("2"));
        assert!(best_match(games, "Zelda").is_none(), "no match rather than a wrong one");
    }

    #[test]
    fn finds_front_box_art() {
        let images = json!({"data": {"base_url": {"large": "https://cdn.example/large/"},
            "images": {"2": [{"type": "boxart", "side": "back", "filename": "b.jpg"},
                             {"type": "boxart", "side": "front", "filename": "boxart/front/2-1.jpg"}]}}});
        assert_eq!(
            front_box_art(&images, 2).as_deref(),
            Some("https://cdn.example/large/boxart/front/2-1.jpg")
        );
        assert_eq!(front_box_art(&images, 3), None);
    }

    #[test]
    fn maps_platforms() {
        assert_eq!(platform_filter("megadrive").as_deref(), Some("18,36"));
        assert_eq!(platform_filter("gbc").as_deref(), Some("41"));
        assert_eq!(platform_filter("n64"), None);
    }
}
