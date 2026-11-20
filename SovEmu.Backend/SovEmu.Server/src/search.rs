//! Search across every system, for Home's search: the same matching as the clients' per-system
//! search (every word of the query, folded for case and accents, in the title, the file's title,
//! the developer, the publisher or the genre), ranked so that titles that start with the query
//! come first, then titles that hold every word, then the rest, each by title.
//!
//! The matching runs here, over a light projection of every game, rather than in SQL: SQLite's
//! `LIKE` folds ASCII case only, so "pokemon" would never find "Pokémon".

use sovemu_library::text::fold;
use sqlx::FromRow;

use crate::AppState;
use crate::api::Game;
use crate::auth::User;
use crate::error::ApiResult;
use crate::home;

/// Results when the caller does not say; and the most it may ask for.
pub const DEFAULT_LIMIT: u32 = 48;
pub const MAX_LIMIT: u32 = 200;
/// Words past this many are ignored (a pasted paragraph is not a search).
const MAX_WORDS: usize = 8;

#[derive(FromRow)]
struct Candidate {
    id: i64,
    title: String,
    file_title: String,
    developer: Option<String>,
    publisher: Option<String>,
    genre: Option<String>,
}

/// The games matching every word of `text`, best first; nothing for a blank query.
pub async fn search(state: &AppState, user: &User, text: &str, limit: u32) -> ApiResult<Vec<Game>> {
    let folded = fold(text);
    let words: Vec<&str> = folded.split_whitespace().take(MAX_WORDS).collect();
    let limit = limit.clamp(1, MAX_LIMIT) as usize;
    if words.is_empty() {
        return Ok(Vec::new());
    }
    let (hidden, hidden_ids) = home::hidden_systems(state);
    let sql = format!(
        "SELECT g.id, COALESCE(m.title, g.title) AS title, g.title AS file_title,
                m.developer, m.publisher, m.genre
         FROM games g LEFT JOIN game_metadata m ON m.game_id = g.id
         WHERE 1{hidden}"
    );
    let mut q = sqlx::query_as::<_, Candidate>(&sql);
    for id in &hidden_ids {
        q = q.bind(id);
    }
    let candidates = q.fetch_all(&state.db).await?;
    let phrase = words.join(" ");
    let mut ranked: Vec<(u8, String, i64)> = candidates
        .into_iter()
        .filter_map(|c| rank(&c, &words, &phrase).map(|r| (r, fold(&c.title), c.id)))
        .collect();
    ranked.sort();
    ranked.truncate(limit);
    if ranked.is_empty() {
        return Ok(Vec::new());
    }

    let ids: Vec<i64> = ranked.iter().map(|(_, _, id)| *id).collect();
    let filter = format!("g.id IN ({})", vec!["?"; ids.len()].join(", "));
    let mut games =
        home::query(state, user.id, &filter, &ids, "g.id", ids.len() as u32, None).await?;
    games.sort_by_key(|g| ids.iter().position(|id| *id == g.id));
    Ok(games)
}

/// How well a game matches (lower is better), or None when a word is missing.
fn rank(c: &Candidate, words: &[&str], phrase: &str) -> Option<u8> {
    let title = fold(&c.title);
    let rest = fold(&format!(
        "{} {} {} {}",
        c.file_title,
        c.developer.as_deref().unwrap_or(""),
        c.publisher.as_deref().unwrap_or(""),
        c.genre.as_deref().unwrap_or(""),
    ));
    if !words.iter().all(|w| title.contains(w) || rest.contains(w)) {
        return None;
    }
    Some(if title.starts_with(phrase) {
        0
    } else if words.iter().all(|w| title.contains(w)) {
        1
    } else {
        2
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(title: &str, developer: Option<&str>) -> Candidate {
        Candidate {
            id: 1,
            title: title.into(),
            file_title: title.into(),
            developer: developer.map(Into::into),
            publisher: None,
            genre: None,
        }
    }

    #[test]
    fn ranks_prefix_then_title_then_elsewhere() {
        let rank_of = |title: &str, dev: Option<&str>, q: &str| {
            let words: Vec<&str> = q.split_whitespace().collect();
            rank(&candidate(title, dev), &words, &words.join(" "))
        };
        assert_eq!(rank_of("Mega Man 2", None, "mega man"), Some(0));
        assert_eq!(rank_of("Rockman & Mega Man", None, "mega man"), Some(1));
        assert_eq!(rank_of("Mega Man 2", Some("Capcom"), "mega capcom"), Some(2));
        assert_eq!(rank_of("Mega Man 2", None, "mega zelda"), None);
        assert_eq!(rank_of("Pokémon Red", None, "pokemon"), Some(0));
    }
}
