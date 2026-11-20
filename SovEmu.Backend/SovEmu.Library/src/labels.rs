//! Telling apart games that show the same title on the same system: a USA release beside a fan
//! translation (`Final Fantasy VII (USA)` / `(T)`), two revisions, or files that game information
//! matched to one name (a compilation's games, a sequel). Each such game gets a short label,
//! `Game.release`, that the clients show with the title; a game whose title is its own gets none.
//!
//! The label is worked out over the whole library, not the list being shown, so a game keeps the
//! same label in every row it appears in; and the server stores it (`games.release`), so listing
//! games never works it out again.

use std::collections::{HashMap, HashSet};

/// A game whose title another game on its system shares.
#[derive(Debug, Clone)]
pub struct Twin {
    pub id: i64,
    pub system: String,
    /// The title shown (game information's, else the file's).
    pub title: String,
    /// The title from the file name, without its tags.
    pub file_title: String,
    /// The file name's tags, as stored (`"USA, Rev 1"`).
    pub tags: String,
    /// The file, for its format when nothing else tells two apart (`.z64` beside `.v64`).
    pub path: String,
}

/// Each twin's label: the tags its twins do not all share (a translation's `T` reads
/// "Translation"); where those leave two the same (or empty), its file's title instead, less
/// the shared title it starts with ("Arc the Lad Collection - Arc the Lad II" → "Arc the Lad
/// II"). A twin with neither has no label.
pub fn labels(twins: &[Twin]) -> HashMap<i64, String> {
    let mut groups: HashMap<(&str, String), Vec<&Twin>> = HashMap::new();
    for t in twins {
        groups.entry((t.system.as_str(), t.title.to_lowercase())).or_default().push(t);
    }
    let mut out = HashMap::new();
    for group in groups.values().filter(|g| g.len() > 1) {
        let tags: Vec<Vec<&str>> = group.iter().map(|t| split_tags(&t.tags)).collect();
        let shared: HashSet<&str> = tags[0]
            .iter()
            .copied()
            .filter(|tag| tags.iter().all(|other| other.contains(tag)))
            .collect();
        let by_tags: Vec<String> = tags
            .iter()
            .map(|own| {
                own.iter()
                    .filter(|tag| !shared.contains(*tag))
                    .map(|tag| readable(tag))
                    .collect::<Vec<_>>()
                    .join(" · ")
            })
            .collect();
        let mut chosen: Vec<Option<String>> = group
            .iter()
            .enumerate()
            .map(|(i, twin)| {
                let label = &by_tags[i];
                let unique =
                    !label.is_empty() && by_tags.iter().filter(|l| *l == label).count() == 1;
                if unique {
                    Some(label.clone())
                } else {
                    // A file named just the title adds nothing: it stays unlabelled beside the
                    // labelled others ("Rev 1" beside the plain release).
                    file_label(twin).or_else(|| (!label.is_empty()).then(|| label.clone()))
                }
            })
            .collect();
        // The same game in two formats (`.z64`, `.v64`) still looks alike: say which file.
        let alike: Vec<bool> = chosen
            .iter()
            .map(|label| chosen.iter().filter(|other| *other == label).count() > 1)
            .collect();
        for (i, twin) in group.iter().enumerate() {
            if alike[i]
                && let Some(ext) = extension(&twin.path)
            {
                chosen[i] = Some(match &chosen[i] {
                    Some(label) => format!("{label} · {ext}"),
                    None => ext,
                });
            }
        }
        for (twin, label) in group.iter().zip(chosen) {
            if let Some(label) = label {
                out.insert(twin.id, label);
            }
        }
    }
    out
}

fn split_tags(tags: &str) -> Vec<&str> {
    tags.split(", ").map(str::trim).filter(|t| !t.is_empty()).collect()
}

/// A tag as a person reads it: the No-Intro and GoodTools translation tags (`T`, `T+Eng`,
/// `T-En by …`) say so.
fn readable(tag: &str) -> String {
    if tag == "T" || tag.starts_with("T+") || tag.starts_with("T-") {
        "Translation".into()
    } else {
        tag.to_owned()
    }
}

/// A file's format, as its extension in capitals (`Z64`).
fn extension(path: &str) -> Option<String> {
    let name = path.rsplit('/').next()?;
    let (_, ext) = name.rsplit_once('.')?;
    (!ext.is_empty()).then(|| ext.to_uppercase())
}

/// The file's own title, less the shown title when it starts with it (` - `, `: `); none when
/// it is just the shown title (case and accents aside: "Pokemon" is "Pokémon").
fn file_label(twin: &Twin) -> Option<String> {
    let file = twin.file_title.trim();
    let shown = twin.title.trim();
    if crate::text::fold(file) == crate::text::fold(shown) {
        return None;
    }
    let rest = file
        .get(..shown.len())
        .filter(|start| start.to_lowercase() == shown.to_lowercase())
        .map(|_| file[shown.len()..].trim_start_matches([' ', '-', ':']))
        .filter(|rest| !rest.is_empty());
    Some(rest.unwrap_or(file).to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn twin(id: i64, title: &str, file_title: &str, tags: &str) -> Twin {
        Twin {
            id,
            system: "psx".into(),
            title: title.into(),
            file_title: file_title.into(),
            tags: tags.into(),
            path: format!("psx/{file_title}.chd"),
        }
    }

    #[test]
    fn differing_tags_tell_releases_apart() {
        let labels = labels(&[
            twin(1, "Final Fantasy VII", "Final Fantasy VII", "USA"),
            twin(2, "Final Fantasy VII", "Final Fantasy VII", "T"),
            twin(3, "Gran Turismo 2", "Gran Turismo 2", "USA, Arcade Mode, Rev 1"),
            twin(4, "Gran Turismo 2", "Gran Turismo 2", "USA, Simulation Mode, Rev 2"),
        ]);
        assert_eq!(labels[&1], "USA");
        assert_eq!(labels[&2], "Translation");
        assert_eq!(labels[&3], "Arcade Mode · Rev 1", "the tags they share are left out");
        assert_eq!(labels[&4], "Simulation Mode · Rev 2");
        let labels = super::labels(&[
            twin(5, "Metroid", "Metroid", "USA"),
            twin(6, "Metroid", "Metroid", "USA, Rev 1"),
        ]);
        assert!(!labels.contains_key(&5), "the plain release stays plain");
        assert_eq!(labels[&6], "Rev 1");
    }

    #[test]
    fn the_same_game_in_two_formats_says_which_file() {
        let mut v64 = twin(1, "Pokémon Puzzle League", "Pokemon Puzzle League", "U, !");
        v64.path = "n64/Pokemon Puzzle League (U) [!].v64".into();
        let mut z64 = twin(2, "Pokémon Puzzle League", "Pokemon Puzzle League", "U, !");
        z64.path = "n64/Pokemon Puzzle League (U) [!].z64".into();
        let labels = labels(&[v64, z64]);
        assert_eq!(labels[&1], "V64", "the accent-folded file title is the title: no label");
        assert_eq!(labels[&2], "Z64");
    }

    #[test]
    fn the_file_title_tells_apart_what_tags_cannot() {
        let labels = labels(&[
            twin(1, "Arc the Lad Collection", "Arc the Lad Collection - Arc the Lad II", "USA"),
            twin(2, "Arc the Lad Collection", "Arc the Lad Collection - Arc Arena", "USA"),
            twin(3, "ESPN Extreme Games", "1Xtreme", "USA"),
            twin(4, "ESPN Extreme Games", "ESPN Extreme Games", "USA"),
        ]);
        assert_eq!(labels[&1], "Arc the Lad II", "less the shared title");
        assert_eq!(labels[&2], "Arc Arena");
        assert_eq!(labels[&3], "1Xtreme");
        assert!(!labels.contains_key(&4), "named just the title: the other one is labelled");
    }

    #[test]
    fn titles_only_collide_on_one_system() {
        let mut snes = twin(2, "Tetris", "Tetris", "USA");
        snes.system = "snes".into();
        assert!(labels(&[twin(1, "Tetris", "Tetris", "USA"), snes]).is_empty());
        assert_eq!(readable("T+Eng"), "Translation");
        assert_eq!(readable("Rev 1"), "Rev 1");
    }
}
