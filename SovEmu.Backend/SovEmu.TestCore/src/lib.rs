//! A deterministic libretro core for testing the SovEmu frontend.
//!
//! It loads like any real core (built as a `cdylib`, opened with `dlopen`) and exercises the
//! parts of the API the frontend must get right, with output a test can predict exactly:
//!
//! - **Video:** 256×224 XRGB8888, with each row padded to [`PITCH`] bytes so the frontend must
//!   honour the pitch. Pixel `(x, y)` on frame `n` is `R = x + n`, `G = y + n` (both mod 256) and
//!   `B` = the low byte of the held buttons. Holding SELECT dupes the frame (passes null).
//!   With the core option `sovemu_test_pattern=solid` every pixel is [`SOLID_COLOUR`]; with
//!   `noise` every pixel is a different colour on every frame, the busiest picture there is:
//!   an encoder then sends all of whatever bitrate it is given, as a demanding game does.
//! - **Stdout:** prints a line to stdout in `retro_init`, as some real cores do.
//! - **Core options:** declared through core options v2 (a "Video" category, explanations and
//!   value labels) when the frontend reports version 2, else through `SET_VARIABLES`.
//! - **Audio:** 48 kHz at 60 fps, so exactly 800 stereo frames per video frame, of a 440 Hz
//!   sine. All but the last arrive through the batch callback; the last through the single-
//!   sample one.
//! - **Skipped output:** each frame asks `GET_AUDIO_VIDEO_ENABLE`. A frame whose video is off
//!   is not drawn (in GL mode, the framebuffer is not cleared) and one whose audio is off is not
//!   synthesised, the sine's phase moving on all the same, so the state comes out as it would
//!   have. Both are still presented, the stale picture and silence, as a core that ignores the
//!   flag would: the frontend must drop them.
//! - **Input:** read once per frame through `RETRO_DEVICE_ID_JOYPAD_MASK`.
//! - **Save states:** 16 bytes, the frame counter and the sine phase.
//! - **Content:** optional (`SET_SUPPORT_NO_GAME`). Loaded content's size is logged.
//! - **Discs:** content that is an `.m3u` playlist is a multi-disc game, one disc per line: the
//!   core declares disk control (`SET_DISK_CONTROL_EXT_INTERFACE`), and like a real core only
//!   changes disc while the tray is open.
//! - **GPU rendering:** with the core option `sovemu_test_render=gl` the core asks for an OpenGL
//!   context (`SET_HW_RENDER`) when the game loads, refusing to load without one, and each frame
//!   clears the frontend's framebuffer to [`SOLID_COLOUR`] with its top half [`GL_TOP_COLOUR`]
//!   (bottom-left origin, so a frontend that forgets to flip gets them the wrong way round).
//!   `gl-<w>x<h>` ([`GL_SIZES`]) does the same at a 3D core's frame size, for benches.
//! - **Battery save:** [`SAVE_RAM_SIZE`] bytes of `RETRO_MEMORY_SAVE_RAM`. Each frame writes the
//!   frame counter into its first 8 bytes (little-endian), so a frontend's `.srm` persistence
//!   can be checked; the rest is left alone.
//! - **A leak:** with [`LEAK_VAR`] set to a number of MiB, each frame allocates that much,
//!   touches every page and never frees it, as a runaway core does, for the frontend's memory
//!   limit. An environment variable rather than a core option, so the options the core declares
//!   stay the same.

use std::ffi::{CStr, c_char, c_uint, c_void};
use std::sync::Mutex;

use sovemu_libretro_ffi as ffi;

pub const WIDTH: u32 = 256;
pub const HEIGHT: u32 = 224;
/// Bytes per row, deliberately wider than `WIDTH * 4`.
pub const PITCH: usize = WIDTH as usize * 4 + 64;
pub const FPS: f64 = 60.0;
pub const SAMPLE_RATE: f64 = 48_000.0;
pub const SAMPLES_PER_FRAME: usize = 800;
pub const SOLID_COLOUR: u32 = 0x00_20_40_80;
pub const SAVE_RAM_SIZE: usize = 8 * 1024;
/// MiB to leak each frame (see the module's docs).
pub const LEAK_VAR: &str = "SOVEMU_TEST_LEAK_MB";

/// What the leak has taken, kept so it is never freed.
static LEAKED: Mutex<Vec<Vec<u8>>> = Mutex::new(Vec::new());

/// [`LEAK_VAR`]'s MiB per frame, read once.
fn leak_mb() -> usize {
    static MB: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *MB.get_or_init(|| std::env::var(LEAK_VAR).ok().and_then(|v| v.parse().ok()).unwrap_or(0))
}

/// Battery RAM. A raw static so its address stays fixed for the frontend, as in a real core.
static SAVE_RAM: Mutex<[u8; SAVE_RAM_SIZE]> = Mutex::new([0; SAVE_RAM_SIZE]);
const TONE_HZ: f64 = 440.0;
/// What the GL mode clears the top half of the frame to (`0x00RRGGBB`).
pub const GL_TOP_COLOUR: u32 = 0x00_FF_00_00;

const RENDER_KEY: &CStr = c"sovemu_test_render";

/// The GL mode's other frame sizes (`sovemu_test_render=gl-640x480`): an N64 and a GameCube at
/// 2× and 4× their native resolution, as Mupen64Plus-Next and Dolphin render them.
pub const GL_SIZES: [(u32, u32); 4] = [(640, 480), (1280, 960), (1280, 1056), (2560, 2112)];

/// The GL mode's frame size, read when the game loads.
static GL_SIZE: Mutex<(u32, u32)> = Mutex::new((WIDTH, HEIGHT));

/// The GL functions the GL mode uses, loaded in `context_reset`.
#[derive(Clone, Copy)]
struct GlFns {
    bind_framebuffer: unsafe extern "C" fn(u32, u32),
    viewport: unsafe extern "C" fn(i32, i32, i32, i32),
    clear_color: unsafe extern "C" fn(f32, f32, f32, f32),
    clear: unsafe extern "C" fn(u32),
    enable: unsafe extern "C" fn(u32),
    disable: unsafe extern "C" fn(u32),
    scissor: unsafe extern "C" fn(i32, i32, i32, i32),
}

struct Hw {
    callback: Option<ffi::retro_hw_render_callback>,
    gl: Option<GlFns>,
}

static HW: Mutex<Hw> = Mutex::new(Hw { callback: None, gl: None });

unsafe extern "C" fn context_reset() {
    let mut hw = HW.lock().unwrap();
    let Some(get) = hw.callback.and_then(|c| c.get_proc_address) else { return };
    let load = |name: &CStr| {
        // SAFETY: the frontend's get_proc_address takes a C string.
        let f = unsafe { get(name.as_ptr()) };
        f.expect("the frontend has this GL function")
    };
    // SAFETY: each pointer is the named GL function, with the signature GL defines.
    hw.gl = Some(unsafe {
        GlFns {
            bind_framebuffer: std::mem::transmute::<
                unsafe extern "C" fn(),
                unsafe extern "C" fn(u32, u32),
            >(load(c"glBindFramebuffer")),
            viewport: std::mem::transmute::<
                unsafe extern "C" fn(),
                unsafe extern "C" fn(i32, i32, i32, i32),
            >(load(c"glViewport")),
            clear_color: std::mem::transmute::<
                unsafe extern "C" fn(),
                unsafe extern "C" fn(f32, f32, f32, f32),
            >(load(c"glClearColor")),
            clear: std::mem::transmute::<unsafe extern "C" fn(), unsafe extern "C" fn(u32)>(load(
                c"glClear",
            )),
            enable: std::mem::transmute::<unsafe extern "C" fn(), unsafe extern "C" fn(u32)>(load(
                c"glEnable",
            )),
            disable: std::mem::transmute::<unsafe extern "C" fn(), unsafe extern "C" fn(u32)>(
                load(c"glDisable"),
            ),
            scissor: std::mem::transmute::<
                unsafe extern "C" fn(),
                unsafe extern "C" fn(i32, i32, i32, i32),
            >(load(c"glScissor")),
        }
    });
}

unsafe extern "C" fn context_destroy() {
    HW.lock().unwrap().gl = None;
}

fn rgb(c: u32) -> (f32, f32, f32) {
    let ch = |shift: u32| ((c >> shift) & 0xff) as f32 / 255.0;
    (ch(16), ch(8), ch(0))
}

/// Draws a GL-mode frame into the frontend's framebuffer. False if there is no context yet.
fn draw_gl() -> bool {
    let hw = HW.lock().unwrap();
    let (Some(gl), Some(cb)) = (hw.gl, hw.callback) else { return false };
    let Some(fbo) = cb.get_current_framebuffer else { return false };
    const COLOR_BUFFER_BIT: u32 = 0x4000;
    const SCISSOR_TEST: u32 = 0x0C11;
    let (w, h) = *GL_SIZE.lock().unwrap();
    let (w, h) = (w as i32, h as i32);
    // SAFETY: the frontend made its context current before retro_run; these are its GL
    // functions, called with valid arguments.
    unsafe {
        (gl.bind_framebuffer)(0x8D40, fbo() as u32);
        (gl.viewport)(0, 0, w, h);
        let (r, g, b) = rgb(SOLID_COLOUR);
        (gl.clear_color)(r, g, b, 1.0);
        (gl.clear)(COLOR_BUFFER_BIT);
        (gl.enable)(SCISSOR_TEST);
        (gl.scissor)(0, h / 2, w, h - h / 2);
        let (r, g, b) = rgb(GL_TOP_COLOUR);
        (gl.clear_color)(r, g, b, 1.0);
        (gl.clear)(COLOR_BUFFER_BIT);
        (gl.disable)(SCISSOR_TEST);
    }
    true
}

/// Reads `sovemu_test_render` from the frontend: the GL mode's frame size, if it is on.
fn wants_gl() -> Option<(u32, u32)> {
    let mut var = ffi::retro_variable { key: RENDER_KEY.as_ptr(), value: std::ptr::null() };
    if !env(ffi::RETRO_ENVIRONMENT_GET_VARIABLE, (&raw mut var).cast()) || var.value.is_null() {
        return None;
    }
    // SAFETY: the frontend returns a NUL-terminated string.
    let value = unsafe { CStr::from_ptr(var.value) }.to_str().ok()?;
    if value == "gl" {
        return Some((WIDTH, HEIGHT));
    }
    let (w, h) = value.strip_prefix("gl-")?.split_once('x')?;
    Some((w.parse().ok()?, h.parse().ok()?)).filter(|size| GL_SIZES.contains(size))
}

#[derive(Clone, Copy)]
struct Callbacks {
    environment: ffi::retro_environment_t,
    video: ffi::retro_video_refresh_t,
    audio_sample: ffi::retro_audio_sample_t,
    audio_batch: ffi::retro_audio_sample_batch_t,
    input_poll: ffi::retro_input_poll_t,
    input_state: ffi::retro_input_state_t,
}

struct State {
    frame: u64,
    phase: f64,
    pattern: Pattern,
    log: ffi::retro_log_printf_t,
    pixels: Vec<u32>,
    audio: Vec<i16>,
}

static CALLBACKS: Mutex<Callbacks> = Mutex::new(Callbacks {
    environment: None,
    video: None,
    audio_sample: None,
    audio_batch: None,
    input_poll: None,
    input_state: None,
});

static STATE: Mutex<State> = Mutex::new(State {
    frame: 0,
    phase: 0.0,
    pattern: Pattern::Gradient,
    log: None,
    pixels: Vec::new(),
    audio: Vec::new(),
});

/// What every frame shows (`sovemu_test_pattern`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Pattern {
    Gradient,
    Solid,
    Noise,
}

const PATTERN_KEY: &CStr = c"sovemu_test_pattern";

fn callbacks() -> Callbacks {
    *CALLBACKS.lock().unwrap()
}

/// Calls the frontend's environment callback.
fn env(cmd: u32, data: *mut c_void) -> bool {
    match callbacks().environment {
        // SAFETY: the frontend's callback; `data` matches `cmd` at every call site.
        Some(f) => unsafe { f(cmd, data) },
        None => false,
    }
}

fn log(message: &CStr) {
    let log = STATE.lock().unwrap().log;
    if let Some(log) = log {
        // SAFETY: "%s" with one C-string argument.
        unsafe { log(ffi::retro_log_level::RETRO_LOG_INFO, c"%s\n".as_ptr(), message.as_ptr()) };
    }
}

/// Reads `sovemu_test_pattern` from the frontend.
fn read_pattern() -> Pattern {
    let mut var = ffi::retro_variable { key: PATTERN_KEY.as_ptr(), value: std::ptr::null() };
    if env(ffi::RETRO_ENVIRONMENT_GET_VARIABLE, (&raw mut var).cast()) && !var.value.is_null() {
        // SAFETY: the frontend returns a NUL-terminated string.
        match unsafe { CStr::from_ptr(var.value) }.to_bytes() {
            b"solid" => Pattern::Solid,
            b"noise" => Pattern::Noise,
            _ => Pattern::Gradient,
        }
    } else {
        Pattern::Gradient
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn retro_api_version() -> c_uint {
    ffi::RETRO_API_VERSION
}

#[unsafe(no_mangle)]
pub extern "C" fn retro_set_environment(cb: ffi::retro_environment_t) {
    CALLBACKS.lock().unwrap().environment = cb;
    let mut no_game = true;
    env(ffi::RETRO_ENVIRONMENT_SET_SUPPORT_NO_GAME, (&raw mut no_game).cast());
    // Like a real core: the richest options interface the frontend offers.
    let mut version: c_uint = 0;
    if env(ffi::RETRO_ENVIRONMENT_GET_CORE_OPTIONS_VERSION, (&raw mut version).cast())
        && version >= 2
    {
        declare_options_v2();
        return;
    }
    let mut vars = [
        ffi::retro_variable {
            key: PATTERN_KEY.as_ptr(),
            value: c"Test pattern; gradient|solid|noise".as_ptr(),
        },
        ffi::retro_variable {
            key: RENDER_KEY.as_ptr(),
            value: c"Rendering; software|gl|gl-640x480|gl-1280x960|gl-1280x1056|gl-2560x2112"
                .as_ptr(),
        },
        ffi::retro_variable { key: std::ptr::null(), value: std::ptr::null() },
    ];
    env(ffi::RETRO_ENVIRONMENT_SET_VARIABLES, vars.as_mut_ptr().cast());
}

/// The same two options through core options v2: a "Video" category, an explanation for each,
/// and labels for their values. The frontend copies everything during the call.
fn declare_options_v2() {
    const NONE: ffi::retro_core_option_value =
        ffi::retro_core_option_value { value: std::ptr::null(), label: std::ptr::null() };
    let values = |list: &[(&'static CStr, &'static CStr)]| {
        let mut out = [NONE; ffi::RETRO_NUM_CORE_OPTION_VALUES_MAX as usize];
        for (slot, (value, label)) in out.iter_mut().zip(list) {
            *slot = ffi::retro_core_option_value { value: value.as_ptr(), label: label.as_ptr() };
        }
        out
    };
    let definition = |key: &'static CStr,
                      desc: &'static CStr,
                      info: &'static CStr,
                      category: *const c_char,
                      list: &[(&'static CStr, &'static CStr)],
                      default: &'static CStr| {
        ffi::retro_core_option_v2_definition {
            key: key.as_ptr(),
            desc: desc.as_ptr(),
            desc_categorized: std::ptr::null(),
            info: info.as_ptr(),
            info_categorized: std::ptr::null(),
            category_key: category,
            values: values(list),
            default_value: default.as_ptr(),
        }
    };
    let mut categories = [
        ffi::retro_core_option_v2_category {
            key: c"video".as_ptr(),
            desc: c"Video".as_ptr(),
            info: c"How frames are drawn.".as_ptr(),
        },
        ffi::retro_core_option_v2_category {
            key: std::ptr::null(),
            desc: std::ptr::null(),
            info: std::ptr::null(),
        },
    ];
    let mut definitions = [
        definition(
            PATTERN_KEY,
            c"Test pattern",
            c"What every frame shows: a moving gradient, one solid colour, or noise.",
            c"video".as_ptr(),
            &[(c"gradient", c"Gradient"), (c"solid", c"Solid colour"), (c"noise", c"Noise")],
            c"gradient",
        ),
        definition(
            RENDER_KEY,
            c"Rendering",
            c"Draw in software, or through an OpenGL context from the frontend.",
            c"video".as_ptr(),
            &[
                (c"software", c"Software"),
                (c"gl", c"OpenGL"),
                (c"gl-640x480", c"OpenGL, N64 at 2×"),
                (c"gl-1280x960", c"OpenGL, N64 at 4×"),
                (c"gl-1280x1056", c"OpenGL, GameCube at 2×"),
                (c"gl-2560x2112", c"OpenGL, GameCube at 4×"),
            ],
            c"software",
        ),
        definition(c"", c"", c"", std::ptr::null(), &[], c""),
    ];
    // The terminator has a null key.
    definitions[2].key = std::ptr::null();
    let mut options = ffi::retro_core_options_v2 {
        categories: categories.as_mut_ptr(),
        definitions: definitions.as_mut_ptr(),
    };
    env(ffi::RETRO_ENVIRONMENT_SET_CORE_OPTIONS_V2, (&raw mut options).cast());
}

#[unsafe(no_mangle)]
pub extern "C" fn retro_set_video_refresh(cb: ffi::retro_video_refresh_t) {
    CALLBACKS.lock().unwrap().video = cb;
}

#[unsafe(no_mangle)]
pub extern "C" fn retro_set_audio_sample(cb: ffi::retro_audio_sample_t) {
    CALLBACKS.lock().unwrap().audio_sample = cb;
}

#[unsafe(no_mangle)]
pub extern "C" fn retro_set_audio_sample_batch(cb: ffi::retro_audio_sample_batch_t) {
    CALLBACKS.lock().unwrap().audio_batch = cb;
}

#[unsafe(no_mangle)]
pub extern "C" fn retro_set_input_poll(cb: ffi::retro_input_poll_t) {
    CALLBACKS.lock().unwrap().input_poll = cb;
}

#[unsafe(no_mangle)]
pub extern "C" fn retro_set_input_state(cb: ffi::retro_input_state_t) {
    CALLBACKS.lock().unwrap().input_state = cb;
}

#[unsafe(no_mangle)]
pub extern "C" fn retro_init() {
    // Like PCSX-ReARMed's Lightrec, chatter on stdout: a frontend whose stdout is a protocol
    // channel must not let it through.
    println!("sovemu test core: initialising (printed to stdout)");
    let mut log_cb = ffi::retro_log_callback::default();
    let log_fn = if env(ffi::RETRO_ENVIRONMENT_GET_LOG_INTERFACE, (&raw mut log_cb).cast()) {
        log_cb.log
    } else {
        None
    };
    {
        let mut s = STATE.lock().unwrap();
        s.frame = 0;
        s.phase = 0.0;
        s.pattern = Pattern::Gradient;
        s.log = log_fn;
        s.pixels = vec![0; PITCH / 4 * HEIGHT as usize];
        *SAVE_RAM.lock().unwrap() = [0; SAVE_RAM_SIZE];
        s.audio = Vec::with_capacity(SAMPLES_PER_FRAME * 2);
    }
    log(c"test core initialised");
}

#[unsafe(no_mangle)]
pub extern "C" fn retro_deinit() {
    log(c"test core deinitialised");
}

/// # Safety
///
/// `info` must be a valid out-parameter.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn retro_get_system_info(info: *mut ffi::retro_system_info) {
    let value = ffi::retro_system_info {
        library_name: c"SovEmu Test Core".as_ptr(),
        library_version: c"1.0.0".as_ptr(),
        valid_extensions: c"sovtest".as_ptr(),
        need_fullpath: false,
        block_extract: false,
    };
    // SAFETY: guaranteed by the caller.
    unsafe { info.write(value) };
}

/// # Safety
///
/// `info` must be a valid out-parameter.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn retro_get_system_av_info(info: *mut ffi::retro_system_av_info) {
    let (width, height) = *GL_SIZE.lock().unwrap();
    let value = ffi::retro_system_av_info {
        geometry: ffi::retro_game_geometry {
            base_width: width,
            base_height: height,
            max_width: width,
            max_height: height,
            aspect_ratio: 4.0 / 3.0,
        },
        timing: ffi::retro_system_timing { fps: FPS, sample_rate: SAMPLE_RATE },
    };
    // SAFETY: guaranteed by the caller.
    unsafe { info.write(value) };
}

#[unsafe(no_mangle)]
pub extern "C" fn retro_set_controller_port_device(_port: c_uint, _device: c_uint) {}

#[unsafe(no_mangle)]
pub extern "C" fn retro_reset() {
    let mut s = STATE.lock().unwrap();
    s.frame = 0;
    s.phase = 0.0;
}

#[unsafe(no_mangle)]
pub extern "C" fn retro_run() {
    let cb = callbacks();
    if let Some(poll) = cb.input_poll {
        // SAFETY: the frontend's callback.
        unsafe { poll() };
    }
    let buttons = match cb.input_state {
        Some(state) => {
            // SAFETY: the frontend's callback.
            let mask =
                unsafe { state(0, ffi::RETRO_DEVICE_JOYPAD, 0, ffi::RETRO_DEVICE_ID_JOYPAD_MASK) };
            mask as u16
        }
        None => 0,
    };

    let mut updated = false;
    env(ffi::RETRO_ENVIRONMENT_GET_VARIABLE_UPDATE, (&raw mut updated).cast());
    let mut av: std::ffi::c_int = 0b11;
    env(ffi::RETRO_ENVIRONMENT_GET_AUDIO_VIDEO_ENABLE, (&raw mut av).cast());
    let video_on = av & ffi::retro_av_enable_flags::RETRO_AV_ENABLE_VIDEO as std::ffi::c_int != 0;
    let audio_on = av & ffi::retro_av_enable_flags::RETRO_AV_ENABLE_AUDIO as std::ffi::c_int != 0;
    let refresh_pattern = updated || STATE.lock().unwrap().frame == 0;
    let pattern = if refresh_pattern { Some(read_pattern()) } else { None };

    let mut s = STATE.lock().unwrap();
    if let Some(pattern) = pattern {
        s.pattern = pattern;
    }
    let frame = s.frame;

    // Video.
    let dupe = buttons & (1 << ffi::RETRO_DEVICE_ID_JOYPAD_SELECT) != 0;
    if !dupe && video_on {
        let stride = PITCH / 4;
        let pattern = s.pattern;
        for y in 0..HEIGHT as usize {
            for x in 0..WIDTH as usize {
                s.pixels[y * stride + x] = match pattern {
                    Pattern::Solid => SOLID_COLOUR,
                    Pattern::Gradient => {
                        let r = (x as u64 + frame) as u8 as u32;
                        let g = (y as u64 + frame) as u8 as u32;
                        (r << 16) | (g << 8) | (buttons as u8 as u32)
                    }
                    // A hash of where and when: no two pixels or frames alike.
                    Pattern::Noise => {
                        let n = (x as u64) << 40 | (y as u64) << 24 | (frame & 0xFF_FFFF);
                        (n.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 40) as u32
                    }
                };
            }
        }
    }

    // Audio.
    s.audio.clear();
    let step = TONE_HZ / SAMPLE_RATE;
    for _ in 0..SAMPLES_PER_FRAME {
        let v =
            if audio_on { ((s.phase * std::f64::consts::TAU).sin() * 8000.0) as i16 } else { 0 };
        s.audio.extend_from_slice(&[v, v]);
        s.phase = (s.phase + step).fract();
    }
    s.frame += 1;
    SAVE_RAM.lock().unwrap()[..8].copy_from_slice(&s.frame.to_le_bytes());
    if leak_mb() > 0 {
        let mut leak = vec![0u8; leak_mb() << 20];
        // Every page written, so it is resident.
        leak.iter_mut().step_by(4096).for_each(|b| *b = 1);
        LEAKED.lock().unwrap().push(leak);
    }

    // Copy out what the callbacks need, then release the lock before calling the frontend.
    let pixels = if dupe { None } else { Some(s.pixels.clone()) };
    let audio = s.audio.clone();
    drop(s);

    let gl_mode = HW.lock().unwrap().callback.is_some();
    if gl_mode {
        if let Some(video) = cb.video {
            let data = if !dupe && (!video_on || draw_gl()) {
                usize::MAX as *const c_void
            } else {
                std::ptr::null()
            };
            let (w, h) = *GL_SIZE.lock().unwrap();
            // SAFETY: RETRO_HW_FRAME_BUFFER_VALID (all bits set) or a dupe.
            unsafe { video(data, w, h, 0) };
        }
    } else if let Some(video) = cb.video {
        let data = pixels.as_ref().map_or(std::ptr::null(), |p| p.as_ptr().cast::<c_void>());
        // SAFETY: `data` is null (a dupe) or HEIGHT rows of PITCH bytes.
        unsafe { video(data, WIDTH, HEIGHT, PITCH) };
    }
    let batch_frames = SAMPLES_PER_FRAME - 1;
    if let Some(batch) = cb.audio_batch {
        // SAFETY: `audio` holds at least `batch_frames` stereo frames.
        unsafe { batch(audio.as_ptr(), batch_frames) };
    }
    if let Some(sample) = cb.audio_sample {
        let last = &audio[batch_frames * 2..];
        // SAFETY: the frontend's callback.
        unsafe { sample(last[0], last[1]) };
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn retro_serialize_size() -> usize {
    16
}

/// # Safety
///
/// `data` must be writable for `size` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn retro_serialize(data: *mut c_void, size: usize) -> bool {
    if size < 16 {
        return false;
    }
    let s = STATE.lock().unwrap();
    let mut bytes = [0u8; 16];
    bytes[..8].copy_from_slice(&s.frame.to_le_bytes());
    bytes[8..].copy_from_slice(&s.phase.to_le_bytes());
    // SAFETY: guaranteed by the caller.
    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), data.cast(), 16) };
    true
}

/// # Safety
///
/// `data` must be readable for `size` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn retro_unserialize(data: *const c_void, size: usize) -> bool {
    if size < 16 {
        return false;
    }
    // SAFETY: guaranteed by the caller.
    let bytes = unsafe { std::slice::from_raw_parts(data.cast::<u8>(), 16) };
    let mut s = STATE.lock().unwrap();
    s.frame = u64::from_le_bytes(bytes[..8].try_into().unwrap());
    s.phase = f64::from_le_bytes(bytes[8..].try_into().unwrap());
    true
}

#[unsafe(no_mangle)]
pub extern "C" fn retro_cheat_reset() {}

#[unsafe(no_mangle)]
pub extern "C" fn retro_cheat_set(_index: c_uint, _enabled: bool, _code: *const c_char) {}

/// # Safety
///
/// `game` must be null or a valid `retro_game_info`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn retro_load_game(game: *const ffi::retro_game_info) -> bool {
    let mut format = ffi::retro_pixel_format::RETRO_PIXEL_FORMAT_XRGB8888;
    if !env(ffi::RETRO_ENVIRONMENT_SET_PIXEL_FORMAT, (&raw mut format).cast()) {
        return false;
    }
    let mut dir: *const c_char = std::ptr::null();
    if !env(ffi::RETRO_ENVIRONMENT_GET_SYSTEM_DIRECTORY, (&raw mut dir).cast()) || dir.is_null() {
        return false;
    }
    HW.lock().unwrap().callback = None;
    let gl = wants_gl();
    *GL_SIZE.lock().unwrap() = gl.unwrap_or((WIDTH, HEIGHT));
    if gl.is_some() {
        let mut cb = ffi::retro_hw_render_callback {
            context_type: ffi::retro_hw_context_type::RETRO_HW_CONTEXT_OPENGL,
            context_reset: Some(context_reset),
            context_destroy: Some(context_destroy),
            depth: true,
            stencil: false,
            bottom_left_origin: true,
            version_major: 2,
            version_minor: 1,
            ..Default::default()
        };
        if !env(ffi::RETRO_ENVIRONMENT_SET_HW_RENDER, (&raw mut cb).cast()) {
            log(c"no GL context: refusing to load");
            return false;
        }
        HW.lock().unwrap().callback = Some(cb);
    }
    // SAFETY: guaranteed by the caller.
    if let Some(game) = unsafe { game.as_ref() } {
        if game.data.is_null() {
            return false;
        }
        let msg = format!("loaded {} bytes of content\0", game.size);
        log(CStr::from_bytes_with_nul(msg.as_bytes()).unwrap());
        // SAFETY: a non-null path is a C string, per the libretro API.
        let path = (!game.path.is_null()).then(|| unsafe { CStr::from_ptr(game.path) });
        if path.is_some_and(|p| p.to_bytes().ends_with(b".m3u")) {
            // SAFETY: `data` holds `size` bytes, per the libretro API.
            let text = unsafe { std::slice::from_raw_parts(game.data.cast::<u8>(), game.size) };
            let discs = text.split(|&b| b == b'\n').filter(|l| !l.trim_ascii().is_empty()).count();
            declare_discs(discs as c_uint);
        }
    }
    true
}

/// A multi-disc game's tray: how many discs, which is in, and whether the tray is open.
struct Tray {
    count: c_uint,
    index: c_uint,
    open: bool,
}

static TRAY: Mutex<Tray> = Mutex::new(Tray { count: 0, index: 0, open: false });

extern "C" fn set_eject_state(open: bool) -> bool {
    TRAY.lock().unwrap().open = open;
    true
}

extern "C" fn get_eject_state() -> bool {
    TRAY.lock().unwrap().open
}

extern "C" fn get_image_index() -> c_uint {
    TRAY.lock().unwrap().index
}

/// Only with the tray open, and only to a disc that exists, as real cores insist.
extern "C" fn set_image_index(index: c_uint) -> bool {
    let mut tray = TRAY.lock().unwrap();
    if !tray.open || index >= tray.count {
        return false;
    }
    tray.index = index;
    true
}

extern "C" fn get_num_images() -> c_uint {
    TRAY.lock().unwrap().count
}

/// Declares disk control for an `.m3u` of `discs` lines.
fn declare_discs(discs: c_uint) {
    *TRAY.lock().unwrap() = Tray { count: discs, index: 0, open: false };
    let mut cb = ffi::retro_disk_control_ext_callback {
        set_eject_state: Some(set_eject_state),
        get_eject_state: Some(get_eject_state),
        get_image_index: Some(get_image_index),
        set_image_index: Some(set_image_index),
        get_num_images: Some(get_num_images),
        ..Default::default()
    };
    env(ffi::RETRO_ENVIRONMENT_SET_DISK_CONTROL_EXT_INTERFACE, (&raw mut cb).cast());
}

#[unsafe(no_mangle)]
pub extern "C" fn retro_load_game_special(
    _type: c_uint,
    _info: *const ffi::retro_game_info,
    _num: usize,
) -> bool {
    false
}

#[unsafe(no_mangle)]
pub extern "C" fn retro_unload_game() {}

#[unsafe(no_mangle)]
pub extern "C" fn retro_get_region() -> c_uint {
    ffi::RETRO_REGION_NTSC
}

#[unsafe(no_mangle)]
pub extern "C" fn retro_get_memory_data(id: c_uint) -> *mut c_void {
    if id == ffi::RETRO_MEMORY_SAVE_RAM {
        // The array lives in a static, so the pointer stays valid after the guard drops.
        SAVE_RAM.lock().unwrap().as_mut_ptr().cast()
    } else {
        std::ptr::null_mut()
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn retro_get_memory_size(id: c_uint) -> usize {
    if id == ffi::RETRO_MEMORY_SAVE_RAM { SAVE_RAM_SIZE } else { 0 }
}
