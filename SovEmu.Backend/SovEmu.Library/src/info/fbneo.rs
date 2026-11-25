//! FinalBurn Neo's DATs: arcade and Neo Geo set names (`mslug`) to titles, years and makers.
//!
//! The DATs come from FBNeo's repository at the commit of the core SovEmu ships (so names match
//! the sets that core plays), downloaded once and kept in `<data>/metadata/fbneo/`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tokio::sync::OnceCell;

use super::{BoxFuture, Found, GameInfo, LookupInput, MetaError, Progress, Provider};

/// The FBNeo in the libretro stable release scripts/cores.list names (1.22.2); its probe
/// version ends in this commit. Bump it with the release.
pub const FBNEO_COMMIT: &str = "01e29d5020880990cff42ac43bc6f016fc048761";
const ARCADE_DAT: &str = "FinalBurn Neo (ClrMame Pro XML, Arcade only).dat";
const NEOGEO_DAT: &str = "FinalBurn Neo (ClrMame Pro XML, Neogeo only).dat";

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub description: String,
    pub year: Option<String>,
    pub manufacturer: Option<String>,
}

pub(crate) type Dat = HashMap<String, Entry>;

#[derive(Debug)]
pub struct FbNeoDats {
    dir: PathBuf,
    http: reqwest::Client,
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

/// Where a DAT is kept: under the commit it came from, so a new core release brings new DATs
/// instead of reusing the old core's.
fn cached(dir: &Path, file: &str) -> PathBuf {
    dir.join(&FBNEO_COMMIT[..12]).join(file)
}

impl FbNeoDats {
    /// The DATs kept in `dir`, downloaded with `http` on first use.
    pub fn new(dir: PathBuf, http: reqwest::Client) -> Self {
        Self { dir, http, arcade: OnceCell::new(), neogeo: OnceCell::new() }
    }

    async fn load<'a>(&'a self, cell: &'a OnceCell<Dat>, file: &str) -> &'a Dat {
        cell.get_or_init(|| async {
            let path = cached(&self.dir, file);
            if !path.is_file() {
                let url = format!(
                    "https://raw.githubusercontent.com/libretro/FBNeo/{FBNEO_COMMIT}/dats/{}",
                    file.replace(' ', "%20")
                );
                match fetch(&self.http, &url, &path).await {
                    Ok(()) => tracing::info!(file, "metadata file downloaded"),
                    Err(e) => {
                        tracing::warn!(file, error = %e, "cannot download metadata file");
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
    pub(crate) async fn find(&self, input: &LookupInput) -> Option<GameInfo> {
        if input.system != "fbneo" && input.system != "neogeo" {
            return None;
        }
        let set = Path::new(&input.file_name).file_stem()?.to_string_lossy().to_lowercase();
        // Arcade folders often hold Neo Geo sets too, so check both lists for either system.
        let neogeo = self.load(&self.neogeo, NEOGEO_DAT).await;
        let arcade = self.load(&self.arcade, ARCADE_DAT).await;
        let entry = neogeo.get(&set).or_else(|| arcade.get(&set))?;
        Some(to_info(entry))
    }
}

impl Provider for FbNeoDats {
    fn id(&self) -> &'static str {
        "fbneo"
    }

    fn name(&self) -> &'static str {
        "FinalBurn Neo"
    }

    fn lookup<'a>(
        &'a self,
        input: &'a LookupInput,
        progress: Progress<'a>,
    ) -> BoxFuture<'a, Result<Option<Found>, MetaError>> {
        Box::pin(async move {
            progress(super::Step::Asking);
            // The set name is the id: nothing to remember.
            Ok(self.find(input).await.map(|info| Found { info, id: None }))
        })
    }
}

pub(crate) fn to_info(entry: &Entry) -> GameInfo {
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
        std::fs::create_dir_all(cached(&dir, "")).unwrap();
        std::fs::write(cached(&dir, NEOGEO_DAT), SAMPLE).unwrap();
        std::fs::write(cached(&dir, ARCADE_DAT), "<datafile/>").unwrap();
        let dats = FbNeoDats::new(dir, reqwest::Client::new());
        let input = |system: &str, file: &str| LookupInput {
            system: system.into(),
            title: String::new(),
            file_name: file.into(),
            size: 0,
            md5: None,
            known_ids: Default::default(),
        };
        let quiet = |_| {};
        let found = dats.lookup(&input("fbneo", "MSLUG.zip"), &quiet).await.unwrap().unwrap();
        assert_eq!(found.info.developer.as_deref(), Some("Nazca"));
        assert_eq!(found.id, None);
        assert!(dats.find(&input("neogeo", "unknown.zip")).await.is_none());
        assert!(dats.find(&input("nes", "mslug.nes")).await.is_none(), "arcade only");
    }
}
