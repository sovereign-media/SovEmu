//! What a GPU core's frame costs from its framebuffer to the encoder, read back as RGBA and
//! converted on the CPU, or converted to I420 on the GPU (`Core::set_gpu_i420`). A bench, not
//! a test: it is ignored unless asked for, and prints a Markdown table.
//!
//! ```text
//! cargo test --release -p sovemu-worker --test gpu_frames -- --ignored --nocapture
//! ```
//!
//! `SOVEMU_BENCH_GPU` picks the GPU (`auto`, the default; `software`; or a render node). The
//! test core draws the frames, at an N64's and a GameCube's 2× and 4× sizes; its drawing (two
//! clears) is not counted. Numbers for the record are in `docs/research/`.

use std::time::{Duration, Instant};

#[path = "../../SovEmu.TestCore/locate.rs"]
mod locate;

use sovemu_media::yuv::{I420Planes, chroma_len, i420_len, rgba_to_i420};
use sovemu_media::{Vp8Config, Vp8Encoder};
use sovemu_retro_host::{Core, CoreOptions, GpuChoice, PixelFormat, VideoFrame};

/// The test core's sized GL modes (`SovEmu.TestCore`'s `GL_SIZES`).
const SIZES: [(&str, u32, u32); 4] = [
    ("N64 2×", 640, 480),
    ("N64 4×", 1280, 960),
    ("GameCube 2×", 1280, 1056),
    ("GameCube 4×", 2560, 2112),
];

const FRAMES: u32 = 120;
/// Frames run first and not counted: the pass is made and checked on the first.
const WARM_UP: u32 = 10;

#[derive(Default)]
struct Times {
    readback: Duration,
    keep: Duration,
    convert: Duration,
    encode: Duration,
}

impl Times {
    fn ms(d: Duration) -> f64 {
        d.as_secs_f64() * 1000.0 / f64::from(FRAMES)
    }
}

/// Main's RGBA copy before this bench's change, kept to show what it cost: a push a pixel.
fn old_write_rgba8(frame: &VideoFrame, out: &mut Vec<u8>) {
    out.clear();
    out.reserve(frame.width as usize * frame.height as usize * 4);
    for px in frame.data.as_chunks::<4>().0 {
        out.extend_from_slice(&[px[0], px[1], px[2], 255]);
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Path {
    /// Main as it was: RGBA, its old copy, converted on the CPU.
    MainRgba,
    /// RGBA, the copy as it is now, converted on the CPU.
    Rgba,
    /// I420 converted on the GPU.
    GpuI420,
}

fn run(gpu: &GpuChoice, path: Path, w: u32, h: u32) -> (Times, String, bool) {
    let dir = std::env::temp_dir();
    let options = CoreOptions {
        system_dir: dir.clone(),
        save_dir: dir,
        variables: vec![("sovemu_test_render".into(), format!("gl-{w}x{h}"))],
        gpu: Some(gpu.clone()),
    };
    let mut core = Core::load(locate::test_core(), options).expect("the test core loads");
    core.load_game(None).expect("a GL context");
    core.set_gpu_i420(path == Path::GpuI420);
    let renderer = core.renderer().map(|r| r.renderer.clone()).unwrap_or_default();
    let mut encoder = Vp8Encoder::new(Vp8Config::default());
    let (mut picture, mut times, mut gpu_converted) =
        (VideoFrame::default(), Times::default(), false);
    let (cw, ch) = (chroma_len(w as usize), chroma_len(h as usize));
    let mut planes = (vec![0; (w * h) as usize], vec![0; cw * ch], vec![0; cw * ch]);
    for n in 0..WARM_UP + FRAMES {
        let video = core.run_frame(&[]).expect("a frame").video;
        let started = Instant::now();
        assert_eq!((video.width, video.height), (w, h));
        gpu_converted = video.format == PixelFormat::I420;
        match path {
            Path::MainRgba => old_write_rgba8(video, &mut picture.data),
            Path::Rgba => video.write_rgba8(&mut picture.data),
            Path::GpuI420 => picture.data.clone_from(&video.data),
        }
        let keep = started.elapsed();
        let readback = core.take_timing().readback;
        // The conversion an encoder does first, timed alone (the encode below does it again).
        let started = Instant::now();
        if gpu_converted {
            let (y, u, v) = &mut planes;
            let (sy, rest) = picture.data.split_at(y.len());
            let (su, sv) = rest.split_at(u.len());
            y.copy_from_slice(sy);
            u.copy_from_slice(su);
            let len = v.len();
            v.copy_from_slice(&sv[..len]);
        } else {
            let (y, u, v) = &mut planes;
            let out = I420Planes { y, y_stride: w as usize, u, u_stride: cw, v, v_stride: cw };
            rgba_to_i420(&picture.data, w as usize, h as usize, out);
        }
        let convert = started.elapsed();
        let started = Instant::now();
        if gpu_converted {
            assert_eq!(picture.data.len(), i420_len(w as usize, h as usize));
            encoder.encode_i420(&picture.data, w, h).expect("encodes");
        } else {
            encoder.encode(&picture.data, w, h).expect("encodes");
        }
        let encode = started.elapsed();
        if n >= WARM_UP {
            times.readback += readback;
            times.keep += keep;
            times.convert += convert;
            times.encode += encode;
        }
    }
    (times, renderer, gpu_converted)
}

#[test]
#[ignore = "a bench: run it with --ignored --nocapture"]
fn gpu_frames_bench() {
    if !sovemu_retro_host::gl::available() {
        eprintln!("skipped: no libEGL here");
        return;
    }
    let gpu: GpuChoice = std::env::var("SOVEMU_BENCH_GPU").unwrap_or_default().parse().unwrap();
    println!(
        "| Frame | Path | Read back | Keep | To I420 | VP8 encode (with its conversion) | \
         Frame to encoder input |"
    );
    println!("|---|---|---:|---:|---:|---:|---:|");
    let mut renderer = String::new();
    for (name, w, h) in SIZES {
        for (path, label) in [
            (Path::MainRgba, "RGBA, main's copy"),
            (Path::Rgba, "RGBA"),
            (Path::GpuI420, "I420 on the GPU"),
        ] {
            let (t, r, converted) = run(&gpu, path, w, h);
            renderer = r;
            assert_eq!(converted, path == Path::GpuI420, "{label}: the GPU did not convert");
            let ms = Times::ms;
            println!(
                "| {name} ({w}×{h}) | {label} | {:.2} | {:.2} | {:.2} | {:.2} | {:.2} |",
                ms(t.readback),
                ms(t.keep),
                ms(t.convert),
                ms(t.encode),
                ms(t.readback + t.keep + t.convert),
            );
        }
    }
    println!(
        "\nRenderer: {renderer}; {FRAMES} frames a row, after {WARM_UP}; milliseconds a frame."
    );
}
