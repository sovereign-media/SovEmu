//! A deterministic libretro core for testing the SovEmu frontend.
//!
//! It loads like any real core (built as a `cdylib`, opened with `dlopen`) and exercises the
//! parts of the API the frontend must get right, with output a test can predict exactly:
//!
//! - **Video:** 256×224 XRGB8888, with each row padded to [`PITCH`] bytes so the frontend must
//!   honour the pitch. Pixel `(x, y)` on frame `n` is `R = x + n`, `G = y + n` (both mod 256) and
//!   `B` = the low byte of the held buttons. Holding SELECT dupes the frame (passes null).
//!   With the core option `sovemu_test_pattern=solid` every pixel is [`SOLID_COLOUR`].
//! - **Audio:** 48 kHz at 60 fps, so exactly 800 stereo frames per video frame, of a 440 Hz
//!   sine. All but the last arrive through the batch callback; the last through the single-
//!   sample one.
//! - **Input:** read once per frame through `RETRO_DEVICE_ID_JOYPAD_MASK`.
//! - **Save states:** 16 bytes, the frame counter and the sine phase.
//! - **Content:** optional (`SET_SUPPORT_NO_GAME`). Loaded content's size is logged.

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
const TONE_HZ: f64 = 440.0;

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
    solid: bool,
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
    solid: false,
    log: None,
    pixels: Vec::new(),
    audio: Vec::new(),
});

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
fn read_pattern() -> bool {
    let mut var = ffi::retro_variable { key: PATTERN_KEY.as_ptr(), value: std::ptr::null() };
    if env(ffi::RETRO_ENVIRONMENT_GET_VARIABLE, (&raw mut var).cast()) && !var.value.is_null() {
        // SAFETY: the frontend returns a NUL-terminated string.
        unsafe { CStr::from_ptr(var.value) }.to_bytes() == b"solid"
    } else {
        false
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
    let mut vars = [
        ffi::retro_variable {
            key: PATTERN_KEY.as_ptr(),
            value: c"Test pattern; gradient|solid".as_ptr(),
        },
        ffi::retro_variable { key: std::ptr::null(), value: std::ptr::null() },
    ];
    env(ffi::RETRO_ENVIRONMENT_SET_VARIABLES, vars.as_mut_ptr().cast());
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
        s.solid = false;
        s.log = log_fn;
        s.pixels = vec![0; PITCH / 4 * HEIGHT as usize];
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
    let value = ffi::retro_system_av_info {
        geometry: ffi::retro_game_geometry {
            base_width: WIDTH,
            base_height: HEIGHT,
            max_width: WIDTH,
            max_height: HEIGHT,
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
    let refresh_pattern = updated || STATE.lock().unwrap().frame == 0;
    let solid = if refresh_pattern { Some(read_pattern()) } else { None };

    let mut s = STATE.lock().unwrap();
    if let Some(solid) = solid {
        s.solid = solid;
    }
    let frame = s.frame;

    // Video.
    let dupe = buttons & (1 << ffi::RETRO_DEVICE_ID_JOYPAD_SELECT) != 0;
    if !dupe {
        let stride = PITCH / 4;
        let solid = s.solid;
        for y in 0..HEIGHT as usize {
            for x in 0..WIDTH as usize {
                s.pixels[y * stride + x] = if solid {
                    SOLID_COLOUR
                } else {
                    let r = (x as u64 + frame) as u8 as u32;
                    let g = (y as u64 + frame) as u8 as u32;
                    (r << 16) | (g << 8) | (buttons as u8 as u32)
                };
            }
        }
    }

    // Audio.
    s.audio.clear();
    let step = TONE_HZ / SAMPLE_RATE;
    for _ in 0..SAMPLES_PER_FRAME {
        let v = ((s.phase * std::f64::consts::TAU).sin() * 8000.0) as i16;
        s.audio.extend_from_slice(&[v, v]);
        s.phase = (s.phase + step).fract();
    }
    s.frame += 1;

    // Copy out what the callbacks need, then release the lock before calling the frontend.
    let pixels = if dupe { None } else { Some(s.pixels.clone()) };
    let audio = s.audio.clone();
    drop(s);

    if let Some(video) = cb.video {
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
    // SAFETY: guaranteed by the caller.
    if let Some(game) = unsafe { game.as_ref() } {
        if game.data.is_null() {
            return false;
        }
        let msg = format!("loaded {} bytes of content\0", game.size);
        log(CStr::from_bytes_with_nul(msg.as_bytes()).unwrap());
    }
    true
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
pub extern "C" fn retro_get_memory_data(_id: c_uint) -> *mut c_void {
    std::ptr::null_mut()
}

#[unsafe(no_mangle)]
pub extern "C" fn retro_get_memory_size(_id: c_uint) -> usize {
    0
}
