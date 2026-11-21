//! The callbacks handed to the core, and the process-global state they read and write.
//!
//! libretro callbacks carry no user-data pointer, so their state has to be global. It sits
//! behind one mutex that is only ever held *inside* a callback or for a short read/write from
//! [`crate::Core`], never across a call into the core: a core calls back into us from inside
//! `retro_run`, `retro_load_game` and friends, and holding the lock there would deadlock.

use std::collections::{BTreeMap, HashSet};
use std::ffi::{CStr, CString, c_char, c_int, c_uint, c_void};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};

use sovemu_libretro_ffi as ffi;

use crate::MAX_PORTS;
use crate::gl::{ContextRequest, GlApi};
use crate::input::{AnalogSticks, JoypadButtons};
use crate::video::{PixelFormat, VideoFrame};

/// `RETRO_HW_FRAME_BUFFER_VALID`, a pointer-valued macro bindgen cannot translate.
const HW_FRAME_BUFFER_VALID: *const c_void = usize::MAX as *const c_void;

/// A core option, declared through `SET_VARIABLES` or the core options interface (v1 or v2).
#[derive(Debug)]
pub(crate) struct Variable {
    /// Position in the core's declaration (options come back in the core's order).
    pub order: usize,
    pub description: String,
    /// What the option does, where the core says (core options v1 and v2).
    pub info: String,
    /// The key of its category (core options v2).
    pub category: Option<String>,
    pub options: Vec<String>,
    /// A display label per option (the value itself where the core gives none).
    pub labels: Vec<String>,
    pub default: String,
    /// The current value; the core reads it through a pointer, so it must stay put until the
    /// next `GET_VARIABLE`.
    pub value: CString,
}

#[derive(Debug, Default)]
pub(crate) struct HostState {
    pub pixel_format: PixelFormat,
    pub video: VideoFrame,
    pub video_updated: bool,
    /// Interleaved stereo samples produced so far this frame.
    pub audio: Vec<i16>,
    pub input: [JoypadButtons; MAX_PORTS],
    /// Analog state per port; kept between frames until the frontend changes it.
    pub analog: [AnalogSticks; MAX_PORTS],
    pub system_dir: CString,
    pub save_dir: CString,
    pub variables: BTreeMap<String, Variable>,
    /// Option categories the core declared (core options v2): key, description, info.
    pub categories: Vec<(String, String, String)>,
    /// Values chosen by the frontend, applied whenever the core (re)declares its variables.
    pub overrides: BTreeMap<String, String>,
    pub variables_dirty: bool,
    pub supports_no_game: bool,
    /// Set when the core changes its geometry or timing; taken by [`crate::Core::run_frame`].
    pub geometry_update: Option<ffi::retro_game_geometry>,
    pub av_update: Option<ffi::retro_system_av_info>,
    pub shutdown_requested: bool,
    pub unsupported_logged: HashSet<c_uint>,
    /// Whether this frontend offers GPU rendering at all (a GPU choice was configured).
    pub hw_allowed: bool,
    /// What the core asked for in `SET_HW_RENDER`.
    pub hw: Option<HwRequest>,
    /// The size of the frame the core rendered into the framebuffer this frame.
    pub hw_frame: Option<(u32, u32)>,
    /// The core's disk control (`SET_DISK_CONTROL_[EXT_]INTERFACE`): a multi-disc game's
    /// tray and disc index.
    pub disk: Option<DiskControl>,
}

/// A core's disk-control functions: the part of `retro_disk_control_[ext_]callback` SovEmu
/// uses. Copied out of [`HostState`] before any is called (never call into the core under the
/// callback lock).
#[derive(Debug, Clone, Copy)]
pub(crate) struct DiskControl {
    pub set_eject_state: ffi::retro_set_eject_state_t,
    pub get_image_index: ffi::retro_get_image_index_t,
    pub set_image_index: ffi::retro_set_image_index_t,
    pub get_num_images: ffi::retro_get_num_images_t,
}

/// A core's `SET_HW_RENDER` request, kept to set up and tear down its context.
#[derive(Debug, Clone, Copy)]
pub(crate) struct HwRequest {
    pub context: ContextRequest,
    pub bottom_left_origin: bool,
    pub context_reset: ffi::retro_hw_context_reset_t,
    pub context_destroy: ffi::retro_hw_context_reset_t,
}

/// The framebuffer a GPU core renders into; read by `get_current_framebuffer`, which a core
/// calls from inside `retro_run`, so it lives outside the state lock.
pub(crate) static HW_FRAMEBUFFER: AtomicUsize = AtomicUsize::new(0);

unsafe extern "C" fn get_current_framebuffer() -> usize {
    HW_FRAMEBUFFER.load(Ordering::Acquire)
}

unsafe extern "C" fn get_proc_address(sym: *const c_char) -> ffi::retro_proc_address_t {
    if sym.is_null() {
        return None;
    }
    // SAFETY: the core passes a NUL-terminated symbol name.
    let p = crate::gl::proc_address(unsafe { CStr::from_ptr(sym) });
    // SAFETY: a GL function pointer (or null, which becomes None); libretro's
    // `retro_proc_address_t` is the generic function pointer the core casts back.
    unsafe { std::mem::transmute::<*const c_void, ffi::retro_proc_address_t>(p) }
}

static STATE: Mutex<Option<HostState>> = Mutex::new(None);

/// Installs fresh state for a core about to be loaded.
pub(crate) fn install(state: HostState) {
    *STATE.lock().unwrap_or_else(PoisonError::into_inner) = Some(state);
}

/// Drops the state of an unloaded core.
pub(crate) fn clear() {
    *STATE.lock().unwrap_or_else(PoisonError::into_inner) = None;
}

/// Runs `f` on the state, if a core is loaded. Never call into the core from `f`.
pub(crate) fn with<R>(f: impl FnOnce(&mut HostState) -> R) -> Option<R> {
    STATE.lock().unwrap_or_else(PoisonError::into_inner).as_mut().map(f)
}

/// Routes the C log shim into `tracing`. Registered once, before the first core is loaded.
pub(crate) fn install_log_sink() {
    // SAFETY: `log_sink` matches the shim's sink signature and lives for the whole process.
    unsafe { ffi::log_shim::sovemu_set_log_sink(Some(log_sink)) };
}

unsafe extern "C" fn log_sink(level: ffi::retro_log_level::Type, message: *const c_char) {
    if message.is_null() {
        return;
    }
    // SAFETY: the shim passes a NUL-terminated buffer that outlives this call.
    let message = unsafe { CStr::from_ptr(message) }.to_string_lossy();
    let message = message.trim_end();
    match level {
        ffi::retro_log_level::RETRO_LOG_DEBUG => tracing::debug!(target: "core", "{message}"),
        ffi::retro_log_level::RETRO_LOG_INFO => tracing::info!(target: "core", "{message}"),
        ffi::retro_log_level::RETRO_LOG_WARN => tracing::warn!(target: "core", "{message}"),
        _ => tracing::error!(target: "core", "{message}"),
    }
}

/// Reads a C string the core passed us, if the pointer is non-null.
///
/// # Safety
///
/// `ptr` must be null or point to a NUL-terminated string.
unsafe fn read_str(ptr: *const c_char) -> Option<String> {
    // SAFETY: guaranteed by the caller.
    (!ptr.is_null()).then(|| unsafe { CStr::from_ptr(ptr) }.to_string_lossy().into_owned())
}

/// An option as a core declares it, before SovEmu gives it a value.
struct Declared {
    key: String,
    description: String,
    info: String,
    category: Option<String>,
    options: Vec<String>,
    labels: Vec<String>,
    default: Option<String>,
}

/// An option's values and their labels (the value where there is no label), up to the first
/// null value.
///
/// # Safety
///
/// Every non-null string in `values` must be NUL-terminated.
unsafe fn read_values(values: &[ffi::retro_core_option_value]) -> (Vec<String>, Vec<String>) {
    let mut options = Vec::new();
    let mut labels = Vec::new();
    for v in values {
        // SAFETY: guaranteed by the caller.
        let Some(value) = (unsafe { read_str(v.value) }) else { break };
        // SAFETY: as above.
        labels.push(
            unsafe { read_str(v.label) }.filter(|l| !l.is_empty()).unwrap_or_else(|| value.clone()),
        );
        options.push(value);
    }
    (options, labels)
}

/// Writes an environment call's out-parameter, refusing a null pointer.
///
/// # Safety
///
/// `data` must be null or valid for writing a `T`.
unsafe fn write<T>(data: *mut c_void, value: T) -> bool {
    if data.is_null() {
        return false;
    }
    // SAFETY: guaranteed by the caller.
    unsafe { data.cast::<T>().write(value) };
    true
}

/// Reads an environment call's in-parameter, refusing a null pointer.
///
/// # Safety
///
/// `data` must be null or valid for reading a `T`.
unsafe fn read<T: Copy>(data: *mut c_void) -> Option<T> {
    // SAFETY: guaranteed by the caller.
    (!data.is_null()).then(|| unsafe { data.cast::<T>().read() })
}

impl HostState {
    /// Handles one `retro_environment_t` call. Unknown or unsupported commands return false,
    /// which tells the core to fall back or go without.
    ///
    /// # Safety
    ///
    /// `data` must be what libretro.h documents for `cmd`.
    unsafe fn environment(&mut self, cmd: c_uint, data: *mut c_void) -> bool {
        // SAFETY: in every arm, `data` has the type libretro.h documents for `cmd`.
        unsafe {
            match cmd {
                ffi::RETRO_ENVIRONMENT_GET_CAN_DUPE => write(data, true),
                ffi::RETRO_ENVIRONMENT_GET_OVERSCAN => write(data, false),
                ffi::RETRO_ENVIRONMENT_SHUTDOWN => {
                    self.shutdown_requested = true;
                    true
                }
                ffi::RETRO_ENVIRONMENT_SET_MESSAGE => {
                    if let Some(msg) = read::<ffi::retro_message>(data)
                        && let Some(text) = read_str(msg.msg)
                    {
                        tracing::info!(target: "core", text = %text, "core message");
                    }
                    true
                }
                ffi::RETRO_ENVIRONMENT_SET_MESSAGE_EXT => {
                    if let Some(msg) = read::<ffi::retro_message_ext>(data)
                        && let Some(text) = read_str(msg.msg)
                    {
                        tracing::info!(target: "core", text = %text, "core message");
                    }
                    true
                }
                ffi::RETRO_ENVIRONMENT_GET_MESSAGE_INTERFACE_VERSION => write(data, 1 as c_uint),
                ffi::RETRO_ENVIRONMENT_SET_PIXEL_FORMAT => {
                    let format =
                        read::<ffi::retro_pixel_format::Type>(data).and_then(PixelFormat::from_raw);
                    match format {
                        Some(format) => {
                            self.pixel_format = format;
                            true
                        }
                        None => false,
                    }
                }
                ffi::RETRO_ENVIRONMENT_GET_SYSTEM_DIRECTORY
                | ffi::RETRO_ENVIRONMENT_GET_CORE_ASSETS_DIRECTORY => {
                    write(data, self.system_dir.as_ptr())
                }
                ffi::RETRO_ENVIRONMENT_GET_SAVE_DIRECTORY => write(data, self.save_dir.as_ptr()),
                ffi::RETRO_ENVIRONMENT_GET_LOG_INTERFACE => write(
                    data,
                    ffi::retro_log_callback { log: Some(ffi::log_shim::sovemu_log_printf) },
                ),
                ffi::RETRO_ENVIRONMENT_SET_VARIABLES => {
                    self.set_variables(data.cast::<ffi::retro_variable>());
                    true
                }
                // Core options v2: categories, an explanation per option, and value labels.
                // Cores ask for the version first and declare their options the richest way the
                // frontend supports.
                ffi::RETRO_ENVIRONMENT_GET_CORE_OPTIONS_VERSION => write(data, 2 as c_uint),
                ffi::RETRO_ENVIRONMENT_SET_CORE_OPTIONS_V2 => {
                    self.set_core_options_v2(data.cast::<ffi::retro_core_options_v2>());
                    // True: categories are supported.
                    true
                }
                ffi::RETRO_ENVIRONMENT_SET_CORE_OPTIONS_V2_INTL => {
                    // SovEmu asks for English (GET_LANGUAGE), so the `us` set is the one.
                    match read::<ffi::retro_core_options_v2_intl>(data) {
                        Some(intl) => self.set_core_options_v2(intl.us),
                        None => self.declare(Vec::new(), Vec::new()),
                    }
                    true
                }
                ffi::RETRO_ENVIRONMENT_SET_CORE_OPTIONS => {
                    self.set_core_options_v1(data.cast::<ffi::retro_core_option_definition>());
                    true
                }
                ffi::RETRO_ENVIRONMENT_SET_CORE_OPTIONS_INTL => {
                    match read::<ffi::retro_core_options_intl>(data) {
                        Some(intl) => self.set_core_options_v1(intl.us),
                        None => self.declare(Vec::new(), Vec::new()),
                    }
                    true
                }
                ffi::RETRO_ENVIRONMENT_GET_VARIABLE => {
                    self.get_variable(data.cast::<ffi::retro_variable>())
                }
                ffi::RETRO_ENVIRONMENT_GET_VARIABLE_UPDATE => {
                    write(data, std::mem::take(&mut self.variables_dirty))
                }
                ffi::RETRO_ENVIRONMENT_SET_SUPPORT_NO_GAME => {
                    self.supports_no_game = read::<bool>(data).unwrap_or(false);
                    true
                }
                ffi::RETRO_ENVIRONMENT_SET_GEOMETRY => {
                    self.geometry_update = read::<ffi::retro_game_geometry>(data);
                    true
                }
                ffi::RETRO_ENVIRONMENT_SET_SYSTEM_AV_INFO => {
                    self.av_update = read::<ffi::retro_system_av_info>(data);
                    true
                }
                ffi::RETRO_ENVIRONMENT_GET_LANGUAGE => {
                    write(data, ffi::retro_language::RETRO_LANGUAGE_ENGLISH as c_uint)
                }
                // Disk control, for multi-disc games: version 1 is the extended interface.
                ffi::RETRO_ENVIRONMENT_GET_DISK_CONTROL_INTERFACE_VERSION => {
                    write(data, 1 as c_uint)
                }
                ffi::RETRO_ENVIRONMENT_SET_DISK_CONTROL_INTERFACE => {
                    let Some(cb) = read::<ffi::retro_disk_control_callback>(data) else {
                        return false;
                    };
                    self.disk = Some(DiskControl {
                        set_eject_state: cb.set_eject_state,
                        get_image_index: cb.get_image_index,
                        set_image_index: cb.set_image_index,
                        get_num_images: cb.get_num_images,
                    });
                    true
                }
                ffi::RETRO_ENVIRONMENT_SET_DISK_CONTROL_EXT_INTERFACE => {
                    let Some(cb) = read::<ffi::retro_disk_control_ext_callback>(data) else {
                        return false;
                    };
                    self.disk = Some(DiskControl {
                        set_eject_state: cb.set_eject_state,
                        get_image_index: cb.get_image_index,
                        set_image_index: cb.set_image_index,
                        get_num_images: cb.get_num_images,
                    });
                    true
                }
                ffi::RETRO_ENVIRONMENT_SET_HW_RENDER => {
                    self.set_hw_render(data.cast::<ffi::retro_hw_render_callback>())
                }
                ffi::RETRO_ENVIRONMENT_GET_PREFERRED_HW_RENDER if self.hw_allowed => {
                    write(data, ffi::retro_hw_context_type::RETRO_HW_CONTEXT_OPENGL_CORE as c_uint)
                }
                // Cores that render from their own threads ask for this; the context is shared
                // by being current on the thread that runs the core, which is what they expect.
                ffi::RETRO_ENVIRONMENT_SET_HW_SHARED_CONTEXT => self.hw_allowed,
                // SovEmu answers `RETRO_DEVICE_ID_JOYPAD_MASK` in `input_state`.
                ffi::RETRO_ENVIRONMENT_GET_INPUT_BITMASKS => true,
                ffi::RETRO_ENVIRONMENT_GET_INPUT_DEVICE_CAPABILITIES => write(
                    data,
                    (1u64 << ffi::RETRO_DEVICE_JOYPAD) | (1u64 << ffi::RETRO_DEVICE_ANALOG),
                ),
                ffi::RETRO_ENVIRONMENT_GET_AUDIO_VIDEO_ENABLE => {
                    // Bit 0: video enabled, bit 1: audio enabled.
                    write(data, 0b11 as c_int)
                }
                ffi::RETRO_ENVIRONMENT_GET_FASTFORWARDING => write(data, false),
                // Information the core offers that SovEmu does not use yet; accepting it is
                // harmless. (Memory maps come back for RetroAchievements.)
                ffi::RETRO_ENVIRONMENT_SET_PERFORMANCE_LEVEL
                | ffi::RETRO_ENVIRONMENT_SET_INPUT_DESCRIPTORS
                | ffi::RETRO_ENVIRONMENT_SET_CONTROLLER_INFO
                | ffi::RETRO_ENVIRONMENT_SET_MEMORY_MAPS
                | ffi::RETRO_ENVIRONMENT_SET_SUBSYSTEM_INFO
                | ffi::RETRO_ENVIRONMENT_SET_CONTENT_INFO_OVERRIDE
                | ffi::RETRO_ENVIRONMENT_SET_SUPPORT_ACHIEVEMENTS
                | ffi::RETRO_ENVIRONMENT_SET_SERIALIZATION_QUIRKS
                | ffi::RETRO_ENVIRONMENT_SET_CORE_OPTIONS_DISPLAY => true,
                // Everything else, including Vulkan's GET_HW_RENDER_INTERFACE, the core options
                // update-display callback, and callbacks we would never call.
                _ => {
                    if self.unsupported_logged.insert(cmd) {
                        tracing::debug!(command = %format!("{cmd:#x}"), "unsupported environment command");
                    }
                    false
                }
            }
        }
    }

    /// Accepts an OpenGL or OpenGL ES request when GPU rendering is allowed, filling in the
    /// frontend's half of the struct. The context itself is made once the game has loaded.
    ///
    /// # Safety
    ///
    /// `hw` must be null or point to a writable `retro_hw_render_callback`.
    unsafe fn set_hw_render(&mut self, hw: *mut ffi::retro_hw_render_callback) -> bool {
        // SAFETY: guaranteed by the caller.
        let Some(hw) = (unsafe { hw.as_mut() }) else { return false };
        use ffi::retro_hw_context_type as t;
        let api = match hw.context_type {
            t::RETRO_HW_CONTEXT_OPENGL => GlApi::Gl,
            t::RETRO_HW_CONTEXT_OPENGL_CORE => GlApi::GlCore,
            t::RETRO_HW_CONTEXT_OPENGLES2
            | t::RETRO_HW_CONTEXT_OPENGLES3
            | t::RETRO_HW_CONTEXT_OPENGLES_VERSION => GlApi::Gles,
            other => {
                tracing::info!(api = other, "the core asked for a GPU API SovEmu does not offer");
                return false;
            }
        };
        if !self.hw_allowed {
            tracing::warn!("the core needs a GPU context, and GPU rendering is off");
            return false;
        }
        let (major, minor) = match hw.context_type {
            t::RETRO_HW_CONTEXT_OPENGLES2 => (2, 0),
            t::RETRO_HW_CONTEXT_OPENGLES3 => (3, 0),
            _ => (hw.version_major, hw.version_minor),
        };
        hw.get_current_framebuffer = Some(get_current_framebuffer);
        hw.get_proc_address = Some(get_proc_address);
        self.hw = Some(HwRequest {
            context: ContextRequest { api, major, minor, depth: hw.depth, stencil: hw.stencil },
            bottom_left_origin: hw.bottom_left_origin,
            context_reset: hw.context_reset,
            context_destroy: hw.context_destroy,
        });
        true
    }

    /// Replaces the declared options, giving each its frontend override or its default.
    fn declare(&mut self, options: Vec<Declared>, categories: Vec<(String, String, String)>) {
        self.variables.clear();
        self.categories = categories;
        for (order, d) in options.into_iter().enumerate() {
            let default = d
                .default
                .filter(|v| d.options.contains(v))
                .or_else(|| d.options.first().cloned())
                .unwrap_or_default();
            let value = match self.overrides.get(&d.key) {
                Some(chosen) => {
                    if !d.options.contains(chosen) {
                        tracing::warn!(
                            option = %d.key,
                            value = %chosen,
                            allowed = ?d.options,
                            "core option value is not one of its choices; using it anyway"
                        );
                    }
                    chosen.clone()
                }
                None => default.clone(),
            };
            self.variables.insert(
                d.key,
                Variable {
                    order,
                    description: d.description,
                    info: d.info,
                    category: d.category,
                    options: d.options,
                    labels: d.labels,
                    default,
                    value: CString::new(value).unwrap_or_default(),
                },
            );
        }
    }

    /// # Safety
    ///
    /// `vars` must be null or point to an array of `retro_variable` terminated by a null key.
    unsafe fn set_variables(&mut self, mut vars: *const ffi::retro_variable) {
        let mut declared = Vec::new();
        while !vars.is_null() {
            // SAFETY: the array is terminated by an entry with a null key, checked below.
            let var = unsafe { vars.read() };
            // SAFETY: libretro strings are NUL-terminated.
            let Some(key) = (unsafe { read_str(var.key) }) else { break };
            // SAFETY: as above.
            let spec = unsafe { read_str(var.value) }.unwrap_or_default();
            // "Description; option1|option2|option3", where the first option is the default.
            let (description, options) = spec.split_once("; ").unwrap_or(("", &spec));
            let options: Vec<String> = options.split('|').map(str::to_owned).collect();
            declared.push(Declared {
                key,
                description: description.to_owned(),
                info: String::new(),
                category: None,
                labels: options.clone(),
                default: None,
                options,
            });
            // SAFETY: not yet past the terminator.
            vars = unsafe { vars.add(1) };
        }
        self.declare(declared, Vec::new());
    }

    /// # Safety
    ///
    /// `defs` must be null or point to an array of `retro_core_option_definition` terminated by
    /// a null key.
    unsafe fn set_core_options_v1(&mut self, mut defs: *const ffi::retro_core_option_definition) {
        let mut declared = Vec::new();
        while !defs.is_null() {
            // SAFETY: the array is terminated by an entry with a null key, checked below.
            let def = unsafe { &*defs };
            // SAFETY: libretro strings are NUL-terminated.
            let Some(key) = (unsafe { read_str(def.key) }) else { break };
            // SAFETY: as above, for every string in the definition.
            let (options, labels) = unsafe { read_values(&def.values) };
            declared.push(Declared {
                key,
                // SAFETY: as above.
                description: unsafe { read_str(def.desc) }.unwrap_or_default(),
                // SAFETY: as above.
                info: unsafe { read_str(def.info) }.unwrap_or_default(),
                category: None,
                options,
                labels,
                // SAFETY: as above.
                default: unsafe { read_str(def.default_value) },
            });
            // SAFETY: not yet past the terminator.
            defs = unsafe { defs.add(1) };
        }
        self.declare(declared, Vec::new());
    }

    /// # Safety
    ///
    /// `options` must be null or point to a `retro_core_options_v2` whose arrays are terminated
    /// by null keys.
    unsafe fn set_core_options_v2(&mut self, options: *const ffi::retro_core_options_v2) {
        // SAFETY: guaranteed by the caller.
        let Some(options) = (unsafe { options.as_ref() }) else {
            return self.declare(Vec::new(), Vec::new());
        };
        let mut categories = Vec::new();
        let mut cat = options.categories.cast_const();
        while !cat.is_null() {
            // SAFETY: terminated by an entry with a null key, checked below.
            let c = unsafe { &*cat };
            // SAFETY: libretro strings are NUL-terminated.
            let Some(key) = (unsafe { read_str(c.key) }) else { break };
            categories.push((
                key,
                // SAFETY: as above.
                unsafe { read_str(c.desc) }.unwrap_or_default(),
                // SAFETY: as above.
                unsafe { read_str(c.info) }.unwrap_or_default(),
            ));
            // SAFETY: not yet past the terminator.
            cat = unsafe { cat.add(1) };
        }
        let mut declared = Vec::new();
        let mut def = options.definitions.cast_const();
        while !def.is_null() {
            // SAFETY: terminated by an entry with a null key, checked below.
            let d = unsafe { &*def };
            // SAFETY: libretro strings are NUL-terminated.
            let Some(key) = (unsafe { read_str(d.key) }) else { break };
            // SAFETY: as above.
            let category = unsafe { read_str(d.category_key) }
                .filter(|k| categories.iter().any(|(c, _, _)| c == k));
            // Within a category the core offers shorter wording, as RetroArch shows it there.
            let pick = |plain: *const c_char, categorized: *const c_char| {
                // SAFETY: as above.
                let categorized = unsafe { read_str(categorized) }.filter(|s| !s.is_empty());
                // SAFETY: as above.
                let plain = unsafe { read_str(plain) }.unwrap_or_default();
                if category.is_some() { categorized.unwrap_or(plain) } else { plain }
            };
            // SAFETY: as above, for every value.
            let (values, labels) = unsafe { read_values(&d.values) };
            declared.push(Declared {
                description: pick(d.desc, d.desc_categorized),
                info: pick(d.info, d.info_categorized),
                key,
                category: category.clone(),
                options: values,
                labels,
                // SAFETY: as above.
                default: unsafe { read_str(d.default_value) },
            });
            // SAFETY: not yet past the terminator.
            def = unsafe { def.add(1) };
        }
        self.declare(declared, categories);
    }

    /// # Safety
    ///
    /// `var` must be null or point to a writable `retro_variable` whose key is a C string.
    unsafe fn get_variable(&mut self, var: *mut ffi::retro_variable) -> bool {
        // SAFETY: guaranteed by the caller.
        let Some(var) = (unsafe { var.as_mut() }) else { return false };
        // SAFETY: as above.
        let found = unsafe { read_str(var.key) }.and_then(|key| self.variables.get(&key));
        match found {
            Some(v) => {
                var.value = v.value.as_ptr();
                true
            }
            None => {
                var.value = std::ptr::null();
                false
            }
        }
    }

    /// Sets a core option from the frontend. Returns false if the core has not declared it
    /// (the value is still remembered, in case the core declares it later).
    pub(crate) fn set_variable(&mut self, key: &str, value: &str) -> bool {
        self.overrides.insert(key.to_owned(), value.to_owned());
        match self.variables.get_mut(key) {
            Some(var) => {
                var.value = CString::new(value).unwrap_or_default();
                self.variables_dirty = true;
                true
            }
            None => false,
        }
    }
}

pub(crate) unsafe extern "C" fn environment(cmd: c_uint, data: *mut c_void) -> bool {
    // SAFETY: the core passes `data` as libretro.h documents for `cmd`.
    with(|s| unsafe { s.environment(cmd, data) }).unwrap_or(false)
}

pub(crate) unsafe extern "C" fn video_refresh(
    data: *const c_void,
    width: c_uint,
    height: c_uint,
    pitch: usize,
) {
    with(|s| {
        // A null frame is a dupe (we answered GET_CAN_DUPE): the previous frame stands.
        if data.is_null() {
            return;
        }
        // A GPU core rendered into the framebuffer: `Core::run_frame` reads it back.
        if data == HW_FRAME_BUFFER_VALID {
            s.hw_frame = Some((width, height));
            s.video_updated = true;
            return;
        }
        let format = s.pixel_format;
        // SAFETY: the core passes `height` rows of `pitch` bytes in the format it set.
        unsafe { s.video.copy_from_core(data.cast(), width, height, pitch, format) };
        s.video_updated = true;
    });
}

pub(crate) unsafe extern "C" fn audio_sample(left: i16, right: i16) {
    with(|s| s.audio.extend_from_slice(&[left, right]));
}

pub(crate) unsafe extern "C" fn audio_sample_batch(data: *const i16, frames: usize) -> usize {
    if data.is_null() {
        return 0;
    }
    // SAFETY: the core passes `frames` interleaved stereo frames.
    let samples = unsafe { std::slice::from_raw_parts(data, frames * 2) };
    with(|s| s.audio.extend_from_slice(samples));
    frames
}

/// Input is latched before `retro_run`, so there is nothing to poll.
pub(crate) unsafe extern "C" fn input_poll() {}

pub(crate) unsafe extern "C" fn input_state(
    port: c_uint,
    device: c_uint,
    index: c_uint,
    id: c_uint,
) -> i16 {
    with(|s| {
        let Some(&buttons) = s.input.get(port as usize) else { return 0 };
        match device & ffi::RETRO_DEVICE_MASK {
            ffi::RETRO_DEVICE_JOYPAD => {}
            ffi::RETRO_DEVICE_ANALOG => return s.analog[port as usize].read(buttons, index, id),
            _ => return 0,
        }
        if id == ffi::RETRO_DEVICE_ID_JOYPAD_MASK {
            buttons.0 as i16
        } else {
            i16::from(buttons.is_pressed(id))
        }
    })
    .unwrap_or(0)
}
