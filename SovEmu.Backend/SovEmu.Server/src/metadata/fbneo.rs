//! FinalBurn Neo's DATs: arcade and Neo Geo set names (`mslug`) to titles, years and makers.
//!
//! The DATs come from FBNeo's repository at the commit SovEmu builds the core from (so names
//! match the sets that core plays), downloaded once and kept in `<data>/metadata/fbneo/`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tokio::sync::OnceCell;

use super::{GameInfo, LookupInput};

/// Must match the FBNeo commit in scripts/cores.list.
const FBNEO_COMMIT: &str = "e6c7ee3d3a34b12f03054d669123ebe813397a8c";
const ARCADE_DAT: &str = "FinalBurn Neo (ClrMame Pro XML, Arcade only).dat";
const NEOGEO_DAT: &str = "FinalBurn Neo (ClrMame Pro XML, Neogeo only).dat";

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub description: String,
    pub year: Option<String>,
    pub manufacturer: Option<String>,
}

pub type Dat = HashMap<String, Entry>;

#[derive(Debug)]
pub struct FbNeoDats {
    dir: PathBuf,
    arcade: OnceCell<Dat>,
    neogeo: OnceCell<Dat>,
}

/// Parses a ClrMame Pro XML DAT into set name → entry.
pub fn parse(xml: &str) -> Dat {
    let Ok(doc) = roxmltree::Document::parse_with_options(
        xml,
        roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() },
    ) else {
        return Dat::new();
    };
    let text = |node: roxmltree::Node, tag: &str| {
        node.children()
            .find(|c| c.has_tag_name(tag))
            .and_then(|c| c.text())
            .map(|t| t.trim().to_owned())
            .filter(|t| !t.is_empty())
    };
    doc.root_element()
        .children()
        .filter(|n| n.has_tag_name("game"))
        .filter_map(|game| {
            let name = game.attribute("name")?.to_owned();
            let description = text(game, "description")?;
            Some((
                name,
                Entry {
                    description,
                    year: text(game, "year"),
                    manufacturer: text(game, "manufacturer"),
                },
            ))
        })
        .collect()
}

impl FbNeoDats {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir, arcade: OnceCell::new(), neogeo: OnceCell::new() }
    }

    async fn load<'a>(
        &'a self,
        http: &reqwest::Client,
        cell: &'a OnceCell<Dat>,
        file: &str,
    ) -> &'a Dat {
        cell.get_or_init(|| async {
            let path = self.dir.join(file);
            if !path.is_file() {
                let url = format!(
                    "https://raw.githubusercontent.com/libretro/FBNeo/{FBNEO_COMMIT}/dats/{}",
                    file.replace(' ', "%20")
                );
                match fetch(http, &url, &path).await {
                    Ok(()) => tracing::info!("metadata: downloaded {file}"),
                    Err(e) => {
                        tracing::warn!("metadata: cannot download {file}: {e}");
                        return Dat::new();
                    }
                }
            }
            let path2 = path.clone();
            tokio::task::spawn_blocking(move || {
                std::fs::read_to_string(path2).map(|x| parse(&x)).unwrap_or_default()
            })
            .await
            .unwrap_or_default()
        })
        .await
    }

    /// Looks an arcade or Neo Geo set up by its file name. `None` for other systems.
    pub async fn lookup(&self, http: &reqwest::Client, input: &LookupInput) -> Option<GameInfo> {
        if input.system != "fbneo" && input.system != "neogeo" {
            return None;
        }
        let set = Path::new(&input.file_name).file_stem()?.to_string_lossy().to_lowercase();
        // Arcade folders often hold Neo Geo sets too, so check both lists for either system.
        let neogeo = self.load(http, &self.neogeo, NEOGEO_DAT).await;
        let arcade = self.load(http, &self.arcade, ARCADE_DAT).await;
        let entry = neogeo.get(&set).or_else(|| arcade.get(&set))?;
        Some(to_info(entry))
    }
}

pub fn to_info(entry: &Entry) -> GameInfo {
    // "Art of Fighting / Ryuuko no Ken (NGM-044 ~ NGH-044)": the parenthesised board, region
    // and set codes are not part of the name.
    let (title, _) = crate::systems::parse_title(&entry.description);
    GameInfo {
        title: Some(title),
        release_date: entry.year.clone().filter(|y| y.chars().all(|c| c.is_ascii_digit())),
        developer: entry.manufacturer.clone(),
        ..GameInfo::default()
    }
}

async fn fetch(http: &reqwest::Client, url: &str, path: &Path) -> Result<(), String> {
    let response = http.get(url).send().await.map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }
    let bytes = response.bytes().await.map_err(|e| e.to_string())?;
    if let Some(dir) = path.parent() {
        tokio::fs::create_dir_all(dir).await.map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("part");
    tokio::fs::write(&tmp, &bytes).await.map_err(|e| e.to_string())?;
    tokio::fs::rename(&tmp, path).await.map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0"?>
<!DOCTYPE datafile PUBLIC "-//FinalBurn Neo//DTD ClrMamePro XML//EN" "http://www.logiqx.com/Dats/datafile.dtd">
<datafile>
	<header><name>FinalBurn Neo</name></header>
	<game name="mslug" romof="neogeo" sourcefile="neogeo/d_neogeo.cpp">
		<description>Metal Slug - Super Vehicle-001</description>
		<year>1996</year>
		<manufacturer>Nazca</manufacturer>
		<rom name="201-p1.p1" size="2097152" crc="08d8daa5"/>
	</game>
	<game name="1943kai"><description>1943 Kai: Midway Kaisen (Japan)</description><year>1987?</year></game>
	<game name="nodesc"><year>1990</year></game>
</datafile>"#;

    #[test]
    fn parses_sets() {
        let dat = parse(SAMPLE);
        assert_eq!(dat.len(), 2, "sets without a description are skipped");
        let info = to_info(&dat["mslug"]);
        assert_eq!(info.title.as_deref(), Some("Metal Slug - Super Vehicle-001"));
        assert_eq!(
            to_info(&dat["1943kai"]).title.as_deref(),
            Some("1943 Kai: Midway Kaisen"),
            "codes stripped"
        );
        assert_eq!(info.release_date.as_deref(), Some("1996"));
        assert_eq!(info.developer.as_deref(), Some("Nazca"));
        assert_eq!(to_info(&dat["1943kai"]).release_date, None, "uncertain years are dropped");
    }

    #[tokio::test]
    async fn looks_up_cached_dats_by_file_name() {
        let dir = std::env::temp_dir().join(format!("sovemu-fbneo-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(NEOGEO_DAT), SAMPLE).unwrap();
        std::fs::write(dir.join(ARCADE_DAT), "<datafile/>").unwrap();
        let dats = FbNeoDats::new(dir);
        let http = reqwest::Client::new();
        let input = |system: &str, file: &str| LookupInput {
            system: system.into(),
            title: String::new(),
            file_name: file.into(),
            size: 0,
            md5: None,
        };
        let found = dats.lookup(&http, &input("fbneo", "MSLUG.zip")).await.unwrap();
        assert_eq!(found.developer.as_deref(), Some("Nazca"));
        assert!(dats.lookup(&http, &input("neogeo", "unknown.zip")).await.is_none());
        assert!(dats.lookup(&http, &input("nes", "mslug.nes")).await.is_none(), "arcade only");
    }
}
