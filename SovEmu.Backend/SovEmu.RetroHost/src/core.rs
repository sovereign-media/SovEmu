use std::ffi::{CString, c_char, c_uint, c_void};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Once;
use std::sync::atomic::{AtomicBool, Ordering};

use libloading::Library;
use sovemu_libretro_ffi as ffi;

use crate::MAX_PORTS;
use crate::callbacks::{self, HostState};
use crate::error::Error;
use crate::input::JoypadButtons;
use crate::video::VideoFrame;

/// Set while a [`Core`] exists; see the crate docs for why there can only be one.
static CORE_ACTIVE: AtomicBool = AtomicBool::new(false);
static LOG_SINK: Once = Once::new();

/// Where the core finds its files, and the core options to start it with.
#[derive(Debug, Clone, Default)]
pub struct CoreOptions {
    /// BIOS and other system files (`RETRO_ENVIRONMENT_GET_SYSTEM_DIRECTORY`).
    pub system_dir: PathBuf,
    /// Where the core may write its own save files (`RETRO_ENVIRONMENT_GET_SAVE_DIRECTORY`).
    pub save_dir: PathBuf,
    /// Core option values (`key`, `value`) that replace the core's defaults.
    pub variables: Vec<(String, String)>,
}

/// What a core says about itself, before any content is loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemInfo {
    pub library_name: String,
    pub library_version: String,
    /// File extensions the core accepts, lower-case, without the dot.
    pub valid_extensions: Vec<String>,
    /// The core reads the content from its path itself rather than from memory.
    pub need_fullpath: bool,
    pub block_extract: bool,
}

/// The video geometry and timing of the loaded content.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AvInfo {
    pub base_width: u32,
    pub base_height: u32,
    pub max_width: u32,
    pub max_height: u32,
    /// Display aspect ratio; derived from the base size when the core gives none.
    pub aspect_ratio: f32,
    /// Frames per second, e.g. 60.0988 for an NTSC NES.
    pub fps: f64,
    /// Audio sample rate in Hz, e.g. 32040.5 for a SNES.
    pub sample_rate: f64,
}

impl AvInfo {
    fn from_raw(raw: &ffi::retro_system_av_info) -> Self {
        let mut info = Self {
            base_width: 0,
            base_height: 0,
            max_width: 0,
            max_height: 0,
            aspect_ratio: 0.0,
            fps: raw.timing.fps,
            sample_rate: raw.timing.sample_rate,
        };
        info.apply_geometry(&raw.geometry);
        info
    }

    fn apply_geometry(&mut self, g: &ffi::retro_game_geometry) {
        self.base_width = g.base_width;
        self.base_height = g.base_height;
        self.max_width = g.max_width;
        self.max_height = g.max_height;
        self.aspect_ratio = if g.aspect_ratio > 0.0 {
            g.aspect_ratio
        } else if g.base_height > 0 {
            g.base_width as f32 / g.base_height as f32
        } else {
            0.0
        };
    }
}

/// A core option the core has declared, with its current value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreVariable {
    pub key: String,
    pub description: String,
    pub options: Vec<String>,
    pub value: String,
}

/// What one call to [`Core::run_frame`] produced.
#[derive(Debug)]
pub struct FrameOutput<'a> {
    /// The latest video frame. When `video_updated` is false the core duped (or skipped) this
    /// frame and this is the previous one.
    pub video: &'a VideoFrame,
    pub video_updated: bool,
    /// Interleaved stereo 16-bit samples, at [`AvInfo::sample_rate`].
    pub audio: &'a [i16],
    /// The core changed its geometry or timing during this frame; re-read [`Core::av_info`].
    pub av_changed: bool,
}

/// The libretro functions every core exports, resolved once at load.
struct CoreApi {
    api_version: unsafe extern "C" fn() -> c_uint,
    get_system_info: unsafe extern "C" fn(*mut ffi::retro_system_info),
    get_system_av_info: unsafe extern "C" fn(*mut ffi::retro_system_av_info),
    set_environment: unsafe extern "C" fn(ffi::retro_environment_t),
    set_video_refresh: unsafe extern "C" fn(ffi::retro_video_refresh_t),
    set_audio_sample: unsafe extern "C" fn(ffi::retro_audio_sample_t),
    set_audio_sample_batch: unsafe extern "C" fn(ffi::retro_audio_sample_batch_t),
    set_input_poll: unsafe extern "C" fn(ffi::retro_input_poll_t),
    set_input_state: unsafe extern "C" fn(ffi::retro_input_state_t),
    set_controller_port_device: unsafe extern "C" fn(c_uint, c_uint),
    init: unsafe extern "C" fn(),
    deinit: unsafe extern "C" fn(),
    reset: unsafe extern "C" fn(),
    run: unsafe extern "C" fn(),
    serialize_size: unsafe extern "C" fn() -> usize,
    serialize: unsafe extern "C" fn(*mut c_void, usize) -> bool,
    unserialize: unsafe extern "C" fn(*const c_void, usize) -> bool,
    cheat_reset: unsafe extern "C" fn(),
    cheat_set: unsafe extern "C" fn(c_uint, bool, *const c_char),
    load_game: unsafe extern "C" fn(*const ffi::retro_game_info) -> bool,
    unload_game: unsafe extern "C" fn(),
}

impl CoreApi {
    /// # Safety
    ///
    /// `lib` must be a libretro core, so each symbol has the signature libretro.h gives it.
    unsafe fn resolve(lib: &Library) -> Result<Self, Error> {
        macro_rules! sym {
            ($name:literal) => {
                // SAFETY: the caller guarantees the symbol has libretro's signature; the
                // function pointer is only used while `lib` is alive (`Core` drops it last).
                *unsafe { lib.get(concat!($name, "\0").as_bytes()) }
                    .map_err(|source| Error::MissingSymbol { name: $name, source })?
            };
        }
        Ok(Self {
            api_version: sym!("retro_api_version"),
            get_system_info: sym!("retro_get_system_info"),
            get_system_av_info: sym!("retro_get_system_av_info"),
            set_environment: sym!("retro_set_environment"),
            set_video_refresh: sym!("retro_set_video_refresh"),
            set_audio_sample: sym!("retro_set_audio_sample"),
            set_audio_sample_batch: sym!("retro_set_audio_sample_batch"),
            set_input_poll: sym!("retro_set_input_poll"),
            set_input_state: sym!("retro_set_input_state"),
            set_controller_port_device: sym!("retro_set_controller_port_device"),
            init: sym!("retro_init"),
            deinit: sym!("retro_deinit"),
            reset: sym!("retro_reset"),
            run: sym!("retro_run"),
            serialize_size: sym!("retro_serialize_size"),
            serialize: sym!("retro_serialize"),
            unserialize: sym!("retro_unserialize"),
            cheat_reset: sym!("retro_cheat_reset"),
            cheat_set: sym!("retro_cheat_set"),
            load_game: sym!("retro_load_game"),
            unload_game: sym!("retro_unload_game"),
        })
    }
}

/// A loaded libretro core. Only one may exist per process (see the crate docs).
///
/// Dropping it unloads the game, deinitialises the core and closes the library.
pub struct Core {
    api: CoreApi,
    info: SystemInfo,
    av_info: Option<AvInfo>,
    /// Content kept alive while loaded: the core may hold pointers into it.
    content: Option<(Option<CString>, Option<Vec<u8>>)>,
    /// This side of the double buffer swapped with the callback state each frame.
    video: VideoFrame,
    audio: Vec<i16>,
    initialized: bool,
    /// Declared last so it is dropped last: every function pointer in `api` points into it.
    _library: Library,
}

impl fmt::Debug for Core {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Core")
            .field("info", &self.info)
            .field("av_info", &self.av_info)
            .field("game_loaded", &self.content.is_some())
            .finish_non_exhaustive()
    }
}

fn c_path(path: &Path) -> Result<CString, Error> {
    CString::new(path.as_os_str().as_encoded_bytes())
        .map_err(|_| Error::InvalidPath(path.to_owned()))
}

impl Core {
    /// Opens the core at `path`, connects the callbacks and calls `retro_init`.
    pub fn load(path: impl AsRef<Path>, options: CoreOptions) -> Result<Self, Error> {
        let path = path.as_ref();
        if CORE_ACTIVE.swap(true, Ordering::AcqRel) {
            return Err(Error::AlreadyLoaded);
        }
        // From here on, failing must release CORE_ACTIVE again.
        let result = Self::load_inner(path, options);
        if result.is_err() {
            callbacks::clear();
            CORE_ACTIVE.store(false, Ordering::Release);
        }
        result
    }

    fn load_inner(path: &Path, options: CoreOptions) -> Result<Self, Error> {
        LOG_SINK.call_once(callbacks::install_log_sink);

        // SAFETY: loading a library runs its initialisers; a libretro core is trusted code
        // chosen by the server's administrator.
        let library = unsafe { Library::new(path) }
            .map_err(|source| Error::OpenLibrary { path: path.to_owned(), source })?;
        // SAFETY: the library is a libretro core.
        let api = unsafe { CoreApi::resolve(&library) }?;

        // SAFETY: a libretro core function with no preconditions.
        let found = unsafe { (api.api_version)() };
        if found != ffi::RETRO_API_VERSION {
            return Err(Error::ApiVersion { found });
        }

        callbacks::install(HostState {
            system_dir: c_path(&options.system_dir)?,
            save_dir: c_path(&options.save_dir)?,
            overrides: options.variables.into_iter().collect(),
            ..HostState::default()
        });

        let mut core = Self {
            api,
            info: SystemInfo {
                library_name: String::new(),
                library_version: String::new(),
                valid_extensions: Vec::new(),
                need_fullpath: false,
                block_extract: false,
            },
            av_info: None,
            content: None,
            video: VideoFrame::default(),
            audio: Vec::new(),
            initialized: false,
            _library: library,
        };

        // SAFETY: the callbacks match libretro's signatures and live for the whole process;
        // set_environment comes before retro_init, as libretro requires.
        unsafe {
            (core.api.set_environment)(Some(callbacks::environment));
            (core.api.set_video_refresh)(Some(callbacks::video_refresh));
            (core.api.set_audio_sample)(Some(callbacks::audio_sample));
            (core.api.set_audio_sample_batch)(Some(callbacks::audio_sample_batch));
            (core.api.set_input_poll)(Some(callbacks::input_poll));
            (core.api.set_input_state)(Some(callbacks::input_state));
            (core.api.init)();
        }
        core.initialized = true;
        core.info = core.read_system_info();
        Ok(core)
    }

    fn read_system_info(&self) -> SystemInfo {
        let mut raw = ffi::retro_system_info::default();
        // SAFETY: `raw` is a valid out-parameter; the strings it receives are static in the core.
        unsafe { (self.api.get_system_info)(&mut raw) };
        // SAFETY: libretro strings are NUL-terminated and owned by the core.
        let text = |p: *const c_char| {
            if p.is_null() {
                String::new()
            } else {
                // SAFETY: as above.
                unsafe { std::ffi::CStr::from_ptr(p) }.to_string_lossy().into_owned()
            }
        };
        SystemInfo {
            library_name: text(raw.library_name),
            library_version: text(raw.library_version),
            valid_extensions: text(raw.valid_extensions)
                .split('|')
                .filter(|e| !e.is_empty())
                .map(str::to_ascii_lowercase)
                .collect(),
            need_fullpath: raw.need_fullpath,
            block_extract: raw.block_extract,
        }
    }

    /// What the core says about itself.
    pub fn system_info(&self) -> &SystemInfo {
        &self.info
    }

    /// Geometry and timing of the loaded content, once a game is loaded.
    pub fn av_info(&self) -> Option<&AvInfo> {
        self.av_info.as_ref()
    }

    /// Whether the core can run with no content (`RETRO_ENVIRONMENT_SET_SUPPORT_NO_GAME`).
    pub fn supports_no_game(&self) -> bool {
        callbacks::with(|s| s.supports_no_game).unwrap_or(false)
    }

    /// Whether the core asked to be shut down (`RETRO_ENVIRONMENT_SHUTDOWN`).
    pub fn shutdown_requested(&self) -> bool {
        callbacks::with(|s| s.shutdown_requested).unwrap_or(false)
    }

    /// Loads content (a ROM) from `path`, or starts the core with no content when `None`.
    pub fn load_game(&mut self, path: Option<&Path>) -> Result<&AvInfo, Error> {
        if self.content.is_some() {
            return Err(Error::GameAlreadyLoaded);
        }
        let (c_path, data) = match path {
            None if !self.supports_no_game() => return Err(Error::ContentRequired),
            None => (None, None),
            Some(path) => {
                let extension = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase());
                if let Some(ext) = &extension {
                    if !self.info.valid_extensions.is_empty()
                        && !self.info.valid_extensions.contains(ext)
                    {
                        tracing::warn!(
                            "{} does not list .{ext} among its extensions {:?}",
                            self.info.library_name,
                            self.info.valid_extensions
                        );
                    }
                }
                let data =
                    if self.info.need_fullpath {
                        None
                    } else {
                        Some(std::fs::read(path).map_err(|source| Error::ReadContent {
                            path: path.to_owned(),
                            source,
                        })?)
                    };
                (Some(c_path(path)?), data)
            }
        };

        let game = ffi::retro_game_info {
            path: c_path.as_ref().map_or(std::ptr::null(), |p| p.as_ptr()),
            data: data.as_ref().map_or(std::ptr::null(), |d| d.as_ptr().cast()),
            size: data.as_ref().map_or(0, Vec::len),
            meta: std::ptr::null(),
        };
        let info_ptr = if path.is_some() { &game as *const _ } else { std::ptr::null() };
        // SAFETY: `game` and the buffers it points into stay alive in `self.content` until the
        // game is unloaded.
        if !unsafe { (self.api.load_game)(info_ptr) } {
            return Err(Error::LoadGameFailed);
        }
        self.content = Some((c_path, data));

        let mut raw = ffi::retro_system_av_info::default();
        // SAFETY: `raw` is a valid out-parameter.
        unsafe { (self.api.get_system_av_info)(&mut raw) };
        callbacks::with(|s| {
            s.geometry_update = None;
            s.av_update = None;
        });

        for port in 0..MAX_PORTS as c_uint {
            // SAFETY: plain values; the core ignores ports it does not have.
            unsafe { (self.api.set_controller_port_device)(port, ffi::RETRO_DEVICE_JOYPAD) };
        }

        Ok(self.av_info.insert(AvInfo::from_raw(&raw)))
    }

    /// Runs one frame with the given controller state (`input[n]` is port `n`; missing ports
    /// have nothing pressed).
    pub fn run_frame(&mut self, input: &[JoypadButtons]) -> Result<FrameOutput<'_>, Error> {
        if self.content.is_none() {
            return Err(Error::NoGameLoaded);
        }
        callbacks::with(|s| {
            s.input = [JoypadButtons::NONE; MAX_PORTS];
            for (port, buttons) in s.input.iter_mut().zip(input) {
                *port = *buttons;
            }
            s.video_updated = false;
            s.audio.clear();
        });

        // SAFETY: a game is loaded; the callbacks only take the state lock briefly.
        unsafe { (self.api.run)() };

        let mut av_changed = false;
        let mut video_updated = false;
        callbacks::with(|s| {
            video_updated = s.video_updated;
            if video_updated {
                // Swap rather than copy: next frame the callback reuses our old allocation.
                std::mem::swap(&mut self.video, &mut s.video);
            }
            std::mem::swap(&mut self.audio, &mut s.audio);
            if let Some(av) = s.av_update.take() {
                self.av_info = Some(AvInfo::from_raw(&av));
                av_changed = true;
            }
            if let (Some(geometry), Some(info)) = (s.geometry_update.take(), &mut self.av_info) {
                info.apply_geometry(&geometry);
                av_changed = true;
            }
        });

        Ok(FrameOutput { video: &self.video, video_updated, audio: &self.audio, av_changed })
    }

    /// Resets the running game, as the console's reset button would.
    pub fn reset(&mut self) {
        if self.content.is_some() {
            // SAFETY: a game is loaded.
            unsafe { (self.api.reset)() };
        }
    }

    /// Writes a save state of the running game.
    pub fn serialize(&mut self) -> Result<Vec<u8>, Error> {
        if self.content.is_none() {
            return Err(Error::NoGameLoaded);
        }
        // SAFETY: a game is loaded.
        let size = unsafe { (self.api.serialize_size)() };
        if size == 0 {
            return Err(Error::SerializeFailed);
        }
        let mut state = vec![0u8; size];
        // SAFETY: `state` is a writable buffer of exactly the size the core asked for.
        if unsafe { (self.api.serialize)(state.as_mut_ptr().cast(), size) } {
            Ok(state)
        } else {
            Err(Error::SerializeFailed)
        }
    }

    /// Restores a save state written by [`Core::serialize`].
    pub fn unserialize(&mut self, state: &[u8]) -> Result<(), Error> {
        if self.content.is_none() {
            return Err(Error::NoGameLoaded);
        }
        // SAFETY: `state` is a readable buffer of the given length.
        if unsafe { (self.api.unserialize)(state.as_ptr().cast(), state.len()) } {
            Ok(())
        } else {
            Err(Error::UnserializeFailed)
        }
    }

    /// Enables a cheat code at `index` (format depends on the core, e.g. Game Genie).
    pub fn set_cheat(&mut self, index: u32, enabled: bool, code: &str) {
        let Ok(code) = CString::new(code) else { return };
        // SAFETY: `code` is a valid C string for the duration of the call.
        unsafe { (self.api.cheat_set)(index, enabled, code.as_ptr()) };
    }

    /// Removes every cheat.
    pub fn reset_cheats(&mut self) {
        // SAFETY: no preconditions.
        unsafe { (self.api.cheat_reset)() };
    }

    /// The core options the core has declared, with their current values.
    pub fn variables(&self) -> Vec<CoreVariable> {
        callbacks::with(|s| {
            s.variables
                .iter()
                .map(|(key, v)| CoreVariable {
                    key: key.clone(),
                    description: v.description.clone(),
                    options: v.options.clone(),
                    value: v.value.to_string_lossy().into_owned(),
                })
                .collect()
        })
        .unwrap_or_default()
    }

    /// Changes a core option; the core picks it up at its next `GET_VARIABLE_UPDATE`.
    /// Returns false if the core has not declared the option.
    pub fn set_variable(&mut self, key: &str, value: &str) -> bool {
        callbacks::with(|s| s.set_variable(key, value)).unwrap_or(false)
    }
}

impl Drop for Core {
    fn drop(&mut self) {
        // SAFETY: tear down in the reverse order of setup; the library is still open.
        unsafe {
            if self.content.take().is_some() {
                (self.api.unload_game)();
            }
            if self.initialized {
                (self.api.deinit)();
            }
        }
        callbacks::clear();
        CORE_ACTIVE.store(false, Ordering::Release);
    }
}
