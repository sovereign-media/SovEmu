//! Drives the real frontend against `SovEmu.TestCore`, loaded with dlopen like any core.
//!
//! Only one core may be loaded per process, so every test takes `CORE_LOCK` first.

use std::sync::{Mutex, MutexGuard};

#[path = "../../SovEmu.TestCore/locate.rs"]
mod locate;
use locate::test_core;

use sovemu_retro_host::{
    Core, CoreOptions, Discs, Error, GpuChoice, JoypadButtons, Keep, PixelFormat,
};

const WIDTH: u32 = 256;
const HEIGHT: u32 = 224;
const SAMPLES_PER_FRAME: usize = 800;
const SOLID_COLOUR: [u8; 4] = [0x20, 0x40, 0x80, 255];

static CORE_LOCK: Mutex<()> = Mutex::new(());

fn lock() -> MutexGuard<'static, ()> {
    CORE_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn options() -> CoreOptions {
    let dir = std::env::temp_dir();
    CoreOptions { system_dir: dir.clone(), save_dir: dir, variables: Vec::new(), gpu: None }
}

fn load_running(options: CoreOptions) -> Core {
    let mut core = Core::load(test_core(), options).expect("load test core");
    core.load_game(None).expect("start test core with no content");
    core
}

/// The RGBA value the test core draws at (x, y) on frame `n` with `buttons` held.
fn expected_pixel(x: u32, y: u32, n: u64, buttons: u16) -> [u8; 4] {
    [(x as u64 + n) as u8, (y as u64 + n) as u8, buttons as u8, 255]
}

fn pixel(rgba: &[u8], x: u32, y: u32) -> [u8; 4] {
    let i = ((y * WIDTH + x) * 4) as usize;
    rgba[i..i + 4].try_into().unwrap()
}

#[test]
fn reports_system_and_av_info() {
    let _guard = lock();
    let mut core = Core::load(test_core(), options()).unwrap();
    let info = core.system_info();
    assert_eq!(info.library_name, "SovEmu Test Core");
    assert_eq!(info.library_version, "1.0.0");
    assert_eq!(info.valid_extensions, ["sovtest"]);
    assert!(!info.need_fullpath);
    assert!(core.supports_no_game());

    let av = *core.load_game(None).unwrap();
    assert_eq!((av.base_width, av.base_height), (WIDTH, HEIGHT));
    assert_eq!(av.fps, 60.0);
    assert_eq!(av.sample_rate, 48_000.0);
    assert!((av.aspect_ratio - 4.0 / 3.0).abs() < 1e-6);
}

#[test]
fn runs_frames_with_video_and_audio() {
    let _guard = lock();
    let mut core = load_running(options());
    for n in 0..3u64 {
        let out = core.run_frame(&[]).unwrap();
        assert!(out.video_updated);
        assert_eq!((out.video.width, out.video.height), (WIDTH, HEIGHT));
        assert_eq!(out.video.format, PixelFormat::Xrgb8888);
        // The core pads rows to a wider pitch; the frontend must have dropped the padding.
        assert_eq!(out.video.data.len(), (WIDTH * HEIGHT * 4) as usize);
        let rgba = out.video.to_rgba8();
        for (x, y) in [(0, 0), (10, 20), (WIDTH - 1, HEIGHT - 1)] {
            assert_eq!(pixel(&rgba, x, y), expected_pixel(x, y, n, 0), "frame {n} ({x},{y})");
        }
        // Batch and single-sample callbacks together deliver exactly one frame of stereo audio.
        assert_eq!(out.audio.len(), SAMPLES_PER_FRAME * 2);
        assert!(out.audio.iter().any(|&s| s != 0));
    }
    // Every frame's time inside the core is counted; this core draws on the CPU, so no readback.
    let timing = core.take_timing();
    assert_eq!(timing.frames, 3);
    assert!(timing.run >= timing.worst_run && !timing.worst_run.is_zero());
    assert!(timing.readback.is_zero());
    assert_eq!(core.take_timing().frames, 0, "taking it starts the count again");
}

#[test]
fn input_reaches_the_core() {
    let _guard = lock();
    let mut core = load_running(options());
    let held = JoypadButtons::A | JoypadButtons::B;
    let out = core.run_frame(&[held]).unwrap();
    let rgba = out.video.to_rgba8();
    assert_eq!(pixel(&rgba, 5, 5), expected_pixel(5, 5, 0, held.0));
}

#[test]
fn a_duped_frame_keeps_the_previous_one() {
    let _guard = lock();
    let mut core = load_running(options());
    let first = core.run_frame(&[]).unwrap().video.clone();
    let out = core.run_frame(&[JoypadButtons::SELECT]).unwrap();
    assert!(!out.video_updated);
    assert_eq!(*out.video, first);
    assert_eq!(out.audio.len(), SAMPLES_PER_FRAME * 2, "audio continues through a dupe");
}

/// The first left sample of frame `n`: the 440 Hz sine, 800 samples a frame from phase 0.
fn first_sample(n: u64) -> i16 {
    let phase = (n as f64 * SAMPLES_PER_FRAME as f64 * 440.0 / 48_000.0).fract();
    ((phase * std::f64::consts::TAU).sin() * 8000.0) as i16
}

#[test]
fn a_frame_run_without_its_picture_or_sound_leaves_the_game_where_it_would_be() {
    let _guard = lock();
    let mut core = load_running(options());
    let first = core.run_frame(&[]).unwrap().video.clone();

    // Nothing kept: no picture (the last one stands) and no sound.
    let out = core.run_frame_keeping(&[], Keep::NOTHING).unwrap();
    assert!(!out.video_updated, "the core presented a picture that was not wanted");
    assert_eq!(*out.video, first);
    assert!(out.audio.is_empty());

    // Sound alone, then the picture alone.
    let out = core.run_frame_keeping(&[], Keep::AUDIO).unwrap();
    assert!(!out.video_updated);
    assert_eq!(out.audio.len(), SAMPLES_PER_FRAME * 2);
    assert_eq!(out.audio[0], first_sample(2));
    let out = core.run_frame_keeping(&[], Keep::VIDEO).unwrap();
    assert!(out.video_updated);
    assert_eq!(pixel(&out.video.to_rgba8(), 7, 9), expected_pixel(7, 9, 3, 0));
    assert!(out.audio.is_empty());

    // And a whole frame again: frame 4, its picture and sound as if all were kept throughout.
    let out = core.run_frame(&[]).unwrap();
    assert_eq!(pixel(&out.video.to_rgba8(), 7, 9), expected_pixel(7, 9, 4, 0));
    assert_eq!(out.audio[0], first_sample(4));
}

#[test]
fn a_state_written_into_a_kept_buffer_reuses_it() {
    let _guard = lock();
    let mut core = load_running(options());
    let mut state = vec![0xAA; 4096];
    let capacity = state.capacity();
    core.run_frame(&[]).unwrap();
    core.serialize_into(&mut state).unwrap();
    assert_eq!(state, core.serialize().unwrap(), "the same state, at the state's size");
    assert_eq!(state.capacity(), capacity, "no new allocation");
    let shown = core.run_frame(&[]).unwrap().video.clone();
    core.run_frame(&[]).unwrap();
    core.unserialize(&state).unwrap();
    assert_eq!(*core.run_frame(&[]).unwrap().video, shown);
}

#[test]
fn save_states_round_trip() {
    let _guard = lock();
    let mut core = load_running(options());
    for _ in 0..5 {
        core.run_frame(&[]).unwrap();
    }
    let state = core.serialize().unwrap();
    let after_save = core.run_frame(&[]).unwrap().video.clone();
    for _ in 0..10 {
        core.run_frame(&[]).unwrap();
    }
    core.unserialize(&state).unwrap();
    assert_eq!(*core.run_frame(&[]).unwrap().video, after_save);
    assert!(matches!(core.unserialize(&[1, 2, 3]), Err(Error::UnserializeFailed)));
}

#[test]
fn reset_restarts_the_game() {
    let _guard = lock();
    let mut core = load_running(options());
    let first = core.run_frame(&[]).unwrap().video.clone();
    core.run_frame(&[]).unwrap();
    core.reset();
    assert_eq!(*core.run_frame(&[]).unwrap().video, first);
}

#[test]
fn core_options_default_override_and_change() {
    let _guard = lock();
    {
        let mut core = load_running(options());
        let mut vars = core.variables();
        let keys: Vec<&str> = vars.iter().map(|v| v.key.as_str()).collect();
        assert_eq!(keys, ["sovemu_test_pattern", "sovemu_test_render"]);
        vars.truncate(1);
        assert_eq!(vars[0].description, "Test pattern");
        assert_eq!(vars[0].options, ["gradient", "solid", "noise"]);
        assert_eq!(vars[0].value, "gradient");
        // Declared through core options v2: category, explanation, labels and default.
        assert_eq!(vars[0].category.as_deref(), Some("video"));
        assert!(vars[0].info.starts_with("What every frame shows"));
        assert_eq!(vars[0].labels, ["Gradient", "Solid colour", "Noise"]);
        assert_eq!(vars[0].default, "gradient");
        let categories = core.option_categories();
        assert_eq!(categories.len(), 1);
        assert_eq!(
            (categories[0].key.as_str(), categories[0].description.as_str()),
            ("video", "Video")
        );

        // Changed at runtime: the core sees GET_VARIABLE_UPDATE on the next frame.
        assert_ne!(pixel(&core.run_frame(&[]).unwrap().video.to_rgba8(), 1, 1), SOLID_COLOUR);
        assert!(core.set_variable("sovemu_test_pattern", "solid"));
        assert_eq!(pixel(&core.run_frame(&[]).unwrap().video.to_rgba8(), 1, 1), SOLID_COLOUR);
        assert!(!core.set_variable("no_such_option", "x"));
    }
    // Overridden at load.
    let mut opts = options();
    opts.variables.push(("sovemu_test_pattern".into(), "solid".into()));
    let mut core = load_running(opts);
    assert_eq!(core.variables()[0].value, "solid");
    assert_eq!(pixel(&core.run_frame(&[]).unwrap().video.to_rgba8(), 1, 1), SOLID_COLOUR);
}

#[test]
fn loads_content_from_a_file() {
    let _guard = lock();
    let rom = std::env::temp_dir().join(format!("sovemu-test-{}.sovtest", std::process::id()));
    std::fs::write(&rom, [0u8; 1024]).unwrap();
    let mut core = Core::load(test_core(), options()).unwrap();
    let result = core.load_game(Some(&rom)).map(|av| av.fps);
    std::fs::remove_file(&rom).ok();
    assert_eq!(result.unwrap(), 60.0);
    assert!(matches!(core.load_game(None), Err(Error::GameAlreadyLoaded)));
    assert!(core.run_frame(&[]).is_ok());
}

#[test]
fn changes_discs_of_a_multi_disc_game() {
    let _guard = lock();
    let playlist = std::env::temp_dir().join(format!("sovemu-test-{}.m3u", std::process::id()));
    std::fs::write(&playlist, "Game (Disc 1).chd\nGame (Disc 2).chd\nGame (Disc 3).chd\n").unwrap();
    let mut core = Core::load(test_core(), options()).unwrap();
    let loaded = core.load_game(Some(&playlist)).map(|_| ());
    std::fs::remove_file(&playlist).ok();
    loaded.unwrap();
    assert_eq!(core.discs(), Some(Discs { count: 3, current: 0 }));
    core.set_disc(2).unwrap();
    assert_eq!(core.discs(), Some(Discs { count: 3, current: 2 }));
    assert!(matches!(core.set_disc(3), Err(Error::Disc(_))));
    assert!(core.run_frame(&[]).is_ok());
}

#[test]
fn a_single_disc_game_has_no_discs_to_change() {
    let _guard = lock();
    let mut core = load_running(options());
    assert_eq!(core.discs(), None);
    assert!(matches!(core.set_disc(1), Err(Error::Disc(_))));
}

#[test]
fn only_one_core_per_process() {
    let _guard = lock();
    let core = Core::load(test_core(), options()).unwrap();
    assert!(matches!(Core::load(test_core(), options()), Err(Error::AlreadyLoaded)));
    drop(core);
    // Unloading frees the slot.
    Core::load(test_core(), options()).unwrap();
}

#[test]
fn running_before_loading_a_game_is_an_error() {
    let _guard = lock();
    let mut core = Core::load(test_core(), options()).unwrap();
    assert!(matches!(core.run_frame(&[]), Err(Error::NoGameLoaded)));
    assert!(matches!(core.serialize(), Err(Error::NoGameLoaded)));
}

#[test]
fn a_missing_library_is_reported() {
    let _guard = lock();
    let err = Core::load("/nonexistent/core.so", options()).unwrap_err();
    assert!(matches!(err, Error::OpenLibrary { .. }));
    // The failed load must not leave the per-process slot taken.
    Core::load(test_core(), options()).unwrap();
}

#[test]
fn exposes_battery_ram() {
    let _guard = lock();
    let mut core = Core::load(test_core(), options()).unwrap();
    assert!(core.save_ram().is_none(), "no game loaded yet");
    core.load_game(None).unwrap();
    assert!(core.system_ram().is_none(), "the test core has no system RAM region");
    let sram = core.save_ram().expect("the test core has battery RAM");
    assert_eq!(sram.len(), 8 * 1024);
    assert!(sram.iter().all(|&b| b == 0), "fresh after init");
    // A frontend restores a .srm by writing into the region before running.
    sram[100] = 0xAB;
    for _ in 0..3 {
        core.run_frame(&[]).unwrap();
    }
    let sram = core.save_ram().unwrap();
    assert_eq!(
        u64::from_le_bytes(sram[..8].try_into().unwrap()),
        3,
        "the core writes its frame count"
    );
    assert_eq!(sram[100], 0xAB, "the restored byte survives");
}

fn gl_options(gpu: Option<GpuChoice>) -> CoreOptions {
    CoreOptions { variables: vec![("sovemu_test_render".into(), "gl".into())], gpu, ..options() }
}

#[test]
fn a_gpu_core_is_refused_when_gpu_rendering_is_off() {
    let _guard = lock();
    let mut core = Core::load(test_core(), gl_options(None)).unwrap();
    assert!(matches!(core.load_game(None), Err(Error::LoadGameFailed)));
    assert!(core.renderer().is_none());
}

/// Needs libEGL with a working device (Mesa's llvmpipe will do): Linux CI and the image's tests
/// have it; elsewhere the test says so and passes.
#[test]
fn renders_a_gpu_core_and_reads_frames_back_upright() {
    if !sovemu_retro_host::gl::available() {
        eprintln!("skipped: no libEGL here");
        return;
    }
    let _guard = lock();
    let mut core = Core::load(test_core(), gl_options(Some(GpuChoice::Software))).unwrap();
    core.load_game(None).expect("load with a GL context");
    let renderer = core.renderer().expect("a GL context").clone();
    assert!(renderer.software, "asked for the software renderer, got {renderer:?}");
    for _ in 0..2 {
        let frame = core.run_frame(&[]).unwrap();
        assert!(frame.video_updated);
        assert_eq!((frame.video.width, frame.video.height), (WIDTH, HEIGHT));
        assert_eq!(frame.video.format, PixelFormat::Rgba8888);
        let rgba = frame.video.to_rgba8();
        assert_eq!(pixel(&rgba, 0, 0), [0xFF, 0, 0, 255], "top half: drawn at the top");
        assert_eq!(pixel(&rgba, WIDTH - 1, HEIGHT - 1), SOLID_COLOUR, "bottom half");
    }
    // Save states still work with a context current.
    let state = core.serialize().unwrap();
    core.unserialize(&state).unwrap();
}

/// Needs libEGL, like the test above.
#[test]
fn a_gpu_cores_frames_can_come_back_as_i420() {
    if !sovemu_retro_host::gl::available() {
        eprintln!("skipped: no libEGL here");
        return;
    }
    let _guard = lock();
    let mut core = Core::load(test_core(), gl_options(Some(GpuChoice::Software))).unwrap();
    core.load_game(None).expect("load with a GL context");
    let rgba = core.run_frame(&[]).unwrap().video.to_rgba8();
    core.set_gpu_i420(true);
    for _ in 0..2 {
        let frame = core.run_frame(&[]).unwrap();
        assert_eq!(frame.video.format, PixelFormat::I420);
        assert_eq!((frame.video.width, frame.video.height), (WIDTH, HEIGHT));
        let (w, h) = (WIDTH as usize, HEIGHT as usize);
        assert_eq!(frame.video.data.len(), w * h + 2 * (w / 2) * (h / 2));
        // Two flat halves, so back to RGBA it is the RGBA frame give or take rounding.
        let back = frame.video.to_rgba8();
        for (x, y) in [(0, 0), (WIDTH - 1, HEIGHT - 1)] {
            let (a, b) = (pixel(&rgba, x, y), pixel(&back, x, y));
            assert!(a.iter().zip(b).all(|(a, b)| a.abs_diff(b) <= 2), "({x}, {y}): {a:?} {b:?}");
        }
    }
    core.set_gpu_i420(false);
    assert_eq!(core.run_frame(&[]).unwrap().video.format, PixelFormat::Rgba8888);
}
