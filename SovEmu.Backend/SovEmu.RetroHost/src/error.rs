use std::fmt;
use std::io;
use std::path::PathBuf;

/// Everything that can go wrong loading or driving a core.
#[derive(Debug)]
pub enum Error {
    /// The core library could not be opened.
    OpenLibrary { path: PathBuf, source: libloading::Error },
    /// The core library lacks a function every libretro core must export.
    MissingSymbol { name: &'static str, source: libloading::Error },
    /// The core implements a libretro API version this frontend does not speak.
    ApiVersion { found: u32 },
    /// Another core is already loaded in this process (see the crate docs).
    AlreadyLoaded,
    /// No content was given and the core cannot run without any.
    ContentRequired,
    /// The content file could not be read.
    ReadContent { path: PathBuf, source: io::Error },
    /// A path contains an interior NUL byte and cannot be passed to C.
    InvalidPath(PathBuf),
    /// `retro_load_game` returned false.
    LoadGameFailed,
    /// A game is already loaded into this core.
    GameAlreadyLoaded,
    /// The operation needs a loaded game.
    NoGameLoaded,
    /// The core does not support save states, or refused to write one.
    SerializeFailed,
    /// The core refused the save state.
    UnserializeFailed,
    /// The core renders with the GPU and no GL context could be made for it.
    Gpu(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OpenLibrary { path, source } => {
                write!(f, "cannot open core {}: {source}", path.display())
            }
            Self::MissingSymbol { name, source } => {
                write!(f, "core does not export {name}: {source}")
            }
            Self::ApiVersion { found } => write!(
                f,
                "core speaks libretro API version {found}, expected {}",
                sovemu_libretro_ffi::RETRO_API_VERSION
            ),
            Self::AlreadyLoaded => f.write_str("a core is already loaded in this process"),
            Self::ContentRequired => f.write_str("this core cannot run without content"),
            Self::ReadContent { path, source } => {
                write!(f, "cannot read content {}: {source}", path.display())
            }
            Self::InvalidPath(path) => write!(f, "path contains a NUL byte: {}", path.display()),
            Self::LoadGameFailed => f.write_str("the core failed to load the content"),
            Self::GameAlreadyLoaded => f.write_str("a game is already loaded"),
            Self::NoGameLoaded => f.write_str("no game is loaded"),
            Self::SerializeFailed => f.write_str("the core could not write a save state"),
            Self::UnserializeFailed => f.write_str("the core rejected the save state"),
            Self::Gpu(e) => write!(f, "no GPU context for this core: {e}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::OpenLibrary { source, .. } | Self::MissingSymbol { source, .. } => Some(source),
            Self::ReadContent { source, .. } => Some(source),
            _ => None,
        }
    }
}
