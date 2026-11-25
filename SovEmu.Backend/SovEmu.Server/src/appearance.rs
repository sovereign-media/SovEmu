//! How the server looks in every client: the background behind every screen, and a photo of
//! each system's hardware for its card. Both are served to the clients (`/media/backgrounds/…`,
//! `/media/systems/…`), so the web, Android and iOS show the same, and the admin's choice reaches
//! them all.
//!
//! The built-in images (`assets/`: SovEmu's own backgrounds, public-domain photos, see
//! `assets/README.md`) are written into the built-in artwork folder ([`write_builtin`]: the
//! image's `/opt/sovemu/artwork/backgrounds` and `systems`) and served from there, else from the
//! copies compiled in. The admin's own sit at the same paths in `<data>/artwork` (and
//! `SovEmu__Appearance__BackgroundDirectories`); an image there wins over a built-in one of the same name, and the
//! folders are listed again after each library scan. One background is shown
//! to everyone: `SovEmu__Appearance__Background`, else the admin's choice, else a different one each visit.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use sqlx::SqlitePool;

use sovemu_library::layout::ArtworkDir;

use crate::AppState;
use crate::error::{ApiError, ApiResult};

/// Each visit gets one of the backgrounds, at random.
pub(crate) const RANDOM: &str = "random";
/// No background: the surface colour alone.
pub(crate) const NONE: &str = "none";

/// The built-in backgrounds: transparent WebP, 1920×1080, laid over the surface colour.
const BUILTIN_BACKGROUNDS: &[(&str, &str, &[u8])] = &[
    ("dither", "Dither", include_bytes!("../assets/backgrounds/dither.webp")),
    ("lattice", "Lattice", include_bytes!("../assets/backgrounds/lattice.webp")),
    ("screens", "Screens", include_bytes!("../assets/backgrounds/screens.webp")),
];

/// The built-in system photos, by system id: WebP cut-outs on transparency.
const BUILTIN_PHOTOS: &[(&str, &[u8])] = &[
    ("atari2600", include_bytes!("../assets/systems/atari2600.webp")),
    ("atari7800", include_bytes!("../assets/systems/atari7800.webp")),
    ("colecovision", include_bytes!("../assets/systems/colecovision.webp")),
    ("dreamcast", include_bytes!("../assets/systems/dreamcast.webp")),
    ("fbneo", include_bytes!("../assets/systems/fbneo.webp")),
    ("gamecube", include_bytes!("../assets/systems/gamecube.webp")),
    ("gamegear", include_bytes!("../assets/systems/gamegear.webp")),
    ("gb", include_bytes!("../assets/systems/gb.webp")),
    ("gba", include_bytes!("../assets/systems/gba.webp")),
    ("gbc", include_bytes!("../assets/systems/gbc.webp")),
    ("intellivision", include_bytes!("../assets/systems/intellivision.webp")),
    ("lynx", include_bytes!("../assets/systems/lynx.webp")),
    ("mastersystem", include_bytes!("../assets/systems/mastersystem.webp")),
    ("megadrive", include_bytes!("../assets/systems/megadrive.webp")),
    ("n64", include_bytes!("../assets/systems/n64.webp")),
    ("neogeo", include_bytes!("../assets/systems/neogeo.webp")),
    ("nes", include_bytes!("../assets/systems/nes.webp")),
    ("ngp", include_bytes!("../assets/systems/ngp.webp")),
    ("ngpc", include_bytes!("../assets/systems/ngpc.webp")),
    ("pcengine", include_bytes!("../assets/systems/pcengine.webp")),
    ("pcenginecd", include_bytes!("../assets/systems/pcenginecd.webp")),
    ("ps2", include_bytes!("../assets/systems/ps2.webp")),
    ("psx", include_bytes!("../assets/systems/psx.webp")),
    ("sega32x", include_bytes!("../assets/systems/sega32x.webp")),
    ("segacd", include_bytes!("../assets/systems/segacd.webp")),
    ("sg1000", include_bytes!("../assets/systems/sg1000.webp")),
    ("snes", include_bytes!("../assets/systems/snes.webp")),
    ("vectrex", include_bytes!("../assets/systems/vectrex.webp")),
    ("virtualboy", include_bytes!("../assets/systems/virtualboy.webp")),
    ("wswan", include_bytes!("../assets/systems/wswan.webp")),
    ("wswanc", include_bytes!("../assets/systems/wswanc.webp")),
];

/// The image formats every client can show.
const EXTENSIONS: &[&str] = &["webp", "png", "jpg", "jpeg"];

/// Where an image comes from.
#[derive(Debug, Clone)]
pub(crate) enum Source {
    /// A built-in image as compiled in (the built-in folder does not hold it).
    Builtin(&'static [u8]),
    /// A built-in image's file in the built-in artwork folder.
    BuiltinFile(PathBuf),
    File(PathBuf),
}

#[derive(Debug, Clone)]
pub(crate) struct Image {
    /// Its name in URLs and settings: the file's name without its extension.
    pub(crate) id: String,
    /// Its name as shown: "Lattice", "Sunset beach" from `sunset-beach.png`.
    pub(crate) name: String,
    pub(crate) source: Source,
}

impl Image {
    pub(crate) fn builtin(&self) -> bool {
        matches!(self.source, Source::Builtin(_) | Source::BuiltinFile(_))
    }

    /// Its content type, from its extension (the built-in ones are WebP).
    pub(crate) fn content_type(&self) -> &'static str {
        match &self.source {
            Source::Builtin(_) => "image/webp",
            Source::File(path) | Source::BuiltinFile(path) => match extension(path).as_deref() {
                Some("png") => "image/png",
                Some("jpg" | "jpeg") => "image/jpeg",
                _ => "image/webp",
            },
        }
    }

    /// Its bytes: compiled in, or read from its file.
    pub(crate) async fn bytes(&self) -> std::io::Result<Cow<'static, [u8]>> {
        match &self.source {
            Source::Builtin(bytes) => Ok(Cow::Borrowed(bytes)),
            Source::File(path) | Source::BuiltinFile(path) => {
                tokio::fs::read(path).await.map(Cow::Owned)
            }
        }
    }
}

fn extension(path: &Path) -> Option<String> {
    path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase())
}

/// An id is a plain name: it goes into URLs and settings.
fn plain_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// "sunset-beach" → "Sunset beach".
fn display_name(id: &str) -> String {
    let words = id.replace(['-', '_'], " ");
    let mut chars = words.chars();
    chars.next().map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect())
}

/// The images in `dir` with a plain name and a format clients show, by id.
fn list_images(dir: &Path) -> Vec<(String, PathBuf)> {
    let mut found: Vec<(String, PathBuf)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && extension(p).is_some_and(|e| EXTENSIONS.contains(&e.as_str())))
        .filter_map(|p| {
            let id = p.file_stem()?.to_str()?.to_owned();
            plain_id(&id).then_some((id, p))
        })
        .collect();
    found.sort();
    found
}

/// The backgrounds and system photos there are.
#[derive(Debug, Clone, Default)]
pub(crate) struct Appearance {
    /// The built-in ones first, then the folders' (a folder's image of the same id replaces the
    /// built-in one, in its place).
    pub(crate) backgrounds: Vec<Image>,
    pub(crate) photos: BTreeMap<String, Image>,
}

impl Appearance {
    /// Lists the folders: `background_dirs` (first wins), the built-in ones in `builtin`, and
    /// `photo_dir` (`<system id>.webp`).
    pub(crate) fn load(
        background_dirs: &[PathBuf],
        builtin: &ArtworkDir,
        photo_dir: &Path,
    ) -> Self {
        // A built-in image from its file in the built-in folder, else as compiled in.
        let source = |dir: PathBuf, id: &str, bytes: &'static [u8]| {
            let file = dir.join(format!("{id}.webp"));
            if file.is_file() { Source::BuiltinFile(file) } else { Source::Builtin(bytes) }
        };
        let mut backgrounds: Vec<Image> = BUILTIN_BACKGROUNDS
            .iter()
            .map(|(id, name, bytes)| Image {
                id: (*id).to_owned(),
                name: (*name).to_owned(),
                source: source(builtin.backgrounds(), id, bytes),
            })
            .collect();
        let mut taken: Vec<String> = Vec::new();
        for dir in background_dirs {
            for (id, path) in list_images(dir) {
                if taken.contains(&id) || id == RANDOM || id == NONE {
                    continue;
                }
                let image =
                    Image { name: display_name(&id), id: id.clone(), source: Source::File(path) };
                match backgrounds.iter_mut().find(|b| b.id == id) {
                    Some(builtin) => *builtin = image,
                    None => backgrounds.push(image),
                }
                taken.push(id);
            }
        }
        let mut photos: BTreeMap<String, Image> = BUILTIN_PHOTOS
            .iter()
            .map(|(id, bytes)| {
                let image = Image {
                    id: (*id).to_owned(),
                    name: (*id).to_owned(),
                    source: source(builtin.systems(), id, bytes),
                };
                ((*id).to_owned(), image)
            })
            .collect();
        for (id, path) in list_images(photo_dir) {
            photos.insert(id.clone(), Image { name: id.clone(), id, source: Source::File(path) });
        }
        Self { backgrounds, photos }
    }

    pub(crate) fn background(&self, id: &str) -> Option<&Image> {
        self.backgrounds.iter().find(|b| b.id == id)
    }

    pub(crate) fn photo(&self, system: &str) -> Option<&Image> {
        self.photos.get(system)
    }
}

/// Writes the built-in backgrounds and system photos into `dir`'s `backgrounds/` and
/// `systems/`, as `<id>.webp`.
pub(crate) fn write_builtin(dir: &ArtworkDir) -> std::io::Result<()> {
    std::fs::create_dir_all(dir.backgrounds())?;
    for (id, _, bytes) in BUILTIN_BACKGROUNDS {
        std::fs::write(dir.backgrounds().join(format!("{id}.webp")), bytes)?;
    }
    std::fs::create_dir_all(dir.systems())?;
    for (id, bytes) in BUILTIN_PHOTOS {
        std::fs::write(dir.systems().join(format!("{id}.webp")), bytes)?;
    }
    Ok(())
}

/// The background setting: the environment's, else the admin's, else [`RANDOM`]; and whether
/// the environment set it (then the admin page shows it locked).
pub(crate) async fn chosen(state: &AppState) -> ApiResult<(String, bool)> {
    if let Some(id) = state.config.background.as_deref().filter(|id| !id.is_empty()) {
        return Ok((id.to_owned(), true));
    }
    let stored: Option<String> =
        sqlx::query_scalar("SELECT value FROM server_settings WHERE key = 'background'")
            .fetch_optional(&state.db)
            .await?;
    Ok((stored.unwrap_or_else(|| RANDOM.to_owned()), false))
}

/// The background to show now: the one chosen, one at random, or none. A chosen one that is not
/// there (its file was removed) shows as random.
pub(crate) async fn current(state: &AppState) -> ApiResult<Option<Image>> {
    let (id, _) = chosen(state).await?;
    let appearance = state.appearance();
    if id == NONE {
        return Ok(None);
    }
    if let Some(image) = appearance.background(&id) {
        return Ok(Some(image.clone()));
    }
    let all = &appearance.backgrounds;
    if all.is_empty() {
        return Ok(None);
    }
    Ok(Some(all[random_index(all.len())].clone()))
}

/// An index below `len`, different from one call to the next.
fn random_index(len: usize) -> usize {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos()),
    );
    (h.finish() % len as u64) as usize
}

/// The admin's choice: a background's id, [`RANDOM`] or [`NONE`]. Refused while the environment
/// sets it.
pub(crate) async fn choose(state: &AppState, id: &str) -> ApiResult<()> {
    if state.config.background.as_deref().is_some_and(|v| !v.is_empty()) {
        return Err(ApiError::conflict("the background is set by SovEmu__Appearance__Background"));
    }
    if id != RANDOM && id != NONE && state.appearance().background(id).is_none() {
        return Err(ApiError::not_found("background"));
    }
    set_setting(&state.db, "background", id).await
}

async fn set_setting(db: &SqlitePool, key: &str, value: &str) -> ApiResult<()> {
    sqlx::query(
        "INSERT INTO server_settings (key, value) VALUES (?, ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(key)
    .bind(value)
    .execute(db)
    .await?;
    Ok(())
}

/// `/media/backgrounds/{id}` and `/media/systems/{id}`: an image, for a signed URL.
pub(crate) async fn serve(image: Option<Image>, what: &str) -> ApiResult<axum::response::Response> {
    use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE};
    use axum::response::IntoResponse;
    let image = image.ok_or_else(|| ApiError::not_found(what))?;
    let bytes = image.bytes().await.map_err(|_| ApiError::not_found(what))?;
    // They change only when the admin's folders do; let clients keep them a day.
    Ok((
        [(CONTENT_TYPE, image.content_type()), (CACHE_CONTROL, "private, max-age=86400")],
        bytes.into_owned(),
    )
        .into_response())
}

pub(crate) async fn background_file(
    axum::extract::State(state): axum::extract::State<AppState>,
    _user: crate::assets::SignedUser,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> ApiResult<axum::response::Response> {
    serve(state.appearance().background(&id).cloned(), "background").await
}

pub(crate) async fn photo_file(
    axum::extract::State(state): axum::extract::State<AppState>,
    _user: crate::assets::SignedUser,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> ApiResult<axum::response::Response> {
    serve(state.appearance().photo(&id).cloned(), "photo").await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("sovemu-appearance-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn every_photo_in_assets_is_built_in() {
        let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/systems");
        let on_disk: Vec<String> = list_images(&assets).into_iter().map(|(id, _)| id).collect();
        let built_in: Vec<&str> = BUILTIN_PHOTOS.iter().map(|(id, _)| *id).collect();
        assert_eq!(on_disk, built_in, "a photo in assets/systems is missing from BUILTIN_PHOTOS");
        for (id, _) in BUILTIN_PHOTOS {
            assert!(sovemu_library::systems::by_id(id).is_some(), "{id} is not a system");
        }
    }

    #[test]
    fn folders_add_backgrounds_and_replace_built_in_ones_by_name() {
        let (first, second, photos) = (dir("first"), dir("second"), dir("photos"));
        std::fs::write(first.join("sunset-beach.png"), b"png").unwrap();
        std::fs::write(first.join("lattice.jpg"), b"jpg").unwrap();
        std::fs::write(second.join("sunset-beach.webp"), b"later folder: loses").unwrap();
        std::fs::write(first.join("notes.txt"), b"not an image").unwrap();
        std::fs::write(first.join("random.png"), b"a reserved name").unwrap();
        std::fs::write(first.join("bad name!.png"), b"not a plain name").unwrap();
        std::fs::write(photos.join("nes.png"), b"my nes").unwrap();
        std::fs::write(photos.join("vectrex.webp"), b"a system with no built-in photo").unwrap();

        let none = ArtworkDir::new("/nonexistent");
        let a = Appearance::load(&[first.clone(), second.clone()], &none, &photos);
        let ids: Vec<&str> = a.backgrounds.iter().map(|b| b.id.as_str()).collect();
        assert_eq!(ids, ["dither", "lattice", "screens", "sunset-beach"]);
        let lattice = a.background("lattice").unwrap();
        assert!(!lattice.builtin(), "the folder's replaces the built-in one");
        assert_eq!(lattice.content_type(), "image/jpeg");
        let beach = a.background("sunset-beach").unwrap();
        assert_eq!((beach.name.as_str(), beach.content_type()), ("Sunset beach", "image/png"));
        assert!(matches!(&beach.source, Source::File(p) if p.starts_with(&first)), "first wins");
        assert!(!a.photo("nes").unwrap().builtin());
        assert!(a.photo("snes").unwrap().builtin());
        assert!(a.photo("vectrex").is_some());
        for d in [first, second, photos] {
            std::fs::remove_dir_all(d).ok();
        }
    }

    #[test]
    fn names_read_well_and_ids_stay_plain() {
        assert_eq!(display_name("sunset-beach"), "Sunset beach");
        assert_eq!(display_name("8_bit"), "8 bit");
        assert!(plain_id("lattice") && plain_id("my-bg_2"));
        assert!(
            !plain_id("") && !plain_id("../etc") && !plain_id("a b") && !plain_id(&"x".repeat(65))
        );
    }

    #[tokio::test]
    async fn built_in_images_are_served_from_the_binary() {
        let none = ArtworkDir::new("/nonexistent");
        let a = Appearance::load(&[], &none, Path::new("/nonexistent"));
        let bytes = a.background("dither").unwrap().bytes().await.unwrap();
        assert_eq!(&bytes[..4], b"RIFF", "WebP");
        assert_eq!(&bytes[8..12], b"WEBP");
    }

    #[tokio::test]
    async fn built_in_images_are_served_from_the_built_in_folder_when_it_has_them() {
        let dir = std::env::temp_dir().join(format!("sovemu-builtin-art-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let builtin = ArtworkDir::new(&dir);
        write_builtin(&builtin).unwrap();
        assert!(builtin.backgrounds().join("lattice.webp").is_file());
        assert!(builtin.systems().join("snes.webp").is_file());
        let a = Appearance::load(&[], &builtin, Path::new("/nonexistent"));
        let lattice = a.background("lattice").unwrap();
        assert!(lattice.builtin(), "still built in");
        assert!(matches!(&lattice.source, Source::BuiltinFile(p) if p.starts_with(&dir)));
        assert!(matches!(&a.photo("snes").unwrap().source, Source::BuiltinFile(_)));
        assert_eq!(&lattice.bytes().await.unwrap()[8..12], b"WEBP");
        std::fs::remove_dir_all(dir).ok();
    }
}
