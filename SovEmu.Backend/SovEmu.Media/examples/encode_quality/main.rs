//! Encoder quality bench (sovereign-media/SovEmu#263): what a session's encoders would send,
//! scored against what the core drew.
//!
//! ```text
//! cargo run --release -p sovemu-media --example encode_quality -- bench \
//!   --captures ../test-results/encode-quality/captures --out ../test-results/encode-quality \
//!   [--clips nes-nova,snes-castle] [--codecs vp8,x264] [--prescale 1,2] [--screen-content 0] \
//!   [--kbps 800,1500,2500,5000] [--timing-runs 2] [--crop-frame 900] [--csv results.csv]
//! ```
//!
//! A clip is a folder of `frame_NNNNNN.png` (the core's own frames, from
//! `scripts/encode-quality-captures.sh`). For every clip it scores the floors (no codec: RGBA →
//! I420 → RGB at 1× and 2×), then every variant: the frames pre-scaled, encoded with the
//! session's settings (only the variable under test changed), decoded, and each frame the viewer
//! would see scored on the 2× grid (`score.rs`). Rows are appended to the CSV; crops of one
//! colour-edge region (the reference, the shown picture and a ΔE2000 heatmap) go to `crops/`.
//!
//! `score-dump` scores frames a client decoded (`device.rs`) the same way, and `score-screen` a
//! screenshot of a client showing a still clip (`screen.rs`).

mod colour;
mod crop;
mod device;
mod score;
mod screen;

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, Write as _};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use sovemu_media::yuv::{I420Planes, chroma_len, rgba_to_i420};
use sovemu_media::{
    DecodedFrame, EncodedFrame, H264Backend, H264Config, H264Decoder, H264Encoder, H264Preference,
    Vp8Config, Vp8Decoder, Vp8Encoder,
};

use colour::YuvToRgb;
use score::{FrameScore, Picture, Reference};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Frames per second the encoders are told (a session rounds the core's rate the same way).
const FPS: u32 = 60;
/// The keyframe the cost table measures: forced here, as a client's request would.
const FORCED_KEYFRAME_AT: usize = 600;

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let command = args.next().unwrap_or_default();
    let options = Options::parse(args)?;
    match command.as_str() {
        "bench" => bench(&options),
        "score-dump" => device::score_dump(&options),
        "score-screen" => screen::score_screen(&options),
        _ => {
            Err("usage: encode_quality bench|score-dump|score-screen ... (see the module docs)"
                .into())
        }
    }
}

/// Command-line options, `--name value` pairs; lists are comma-separated.
#[derive(Debug, Default)]
pub(crate) struct Options {
    values: BTreeMap<String, String>,
}

impl Options {
    fn parse(mut args: impl Iterator<Item = String>) -> Result<Self> {
        let mut values = BTreeMap::new();
        while let Some(name) = args.next() {
            let name = name.strip_prefix("--").ok_or(format!("expected --option, got {name}"))?;
            let value = args.next().ok_or(format!("--{name} needs a value"))?;
            values.insert(name.to_owned(), value);
        }
        Ok(Self { values })
    }

    pub(crate) fn get(&self, name: &str) -> Option<&str> {
        self.values.get(name).map(String::as_str)
    }

    pub(crate) fn path(&self, name: &str) -> Result<PathBuf> {
        Ok(PathBuf::from(self.get(name).ok_or(format!("--{name} is required"))?))
    }

    pub(crate) fn list<T: std::str::FromStr>(&self, name: &str, default: &str) -> Result<Vec<T>> {
        self.get(name)
            .unwrap_or(default)
            .split(',')
            .filter(|s| !s.is_empty())
            .map(|s| s.trim().parse().map_err(|_| format!("--{name}: bad value {s}").into()))
            .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Codec {
    Vp8,
    X264,
}

impl std::str::FromStr for Codec {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, String> {
        match s {
            "vp8" => Ok(Self::Vp8),
            "x264" | "h264" => Ok(Self::X264),
            _ => Err(format!("unknown codec {s}")),
        }
    }
}

impl Codec {
    fn name(self) -> &'static str {
        match self {
            Self::Vp8 => "vp8",
            Self::X264 => "x264",
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Variant {
    codec: Codec,
    prescale: usize,
    screen_content: u8,
    kbps: u32,
}

impl Variant {
    fn name(&self) -> String {
        format!("{}-{}x-sc{}-{}k", self.codec.name(), self.prescale, self.screen_content, self.kbps)
    }
}

/// One clip's frames, as the core drew them.
#[derive(Debug)]
pub(crate) struct Clip {
    pub(crate) name: String,
    pub(crate) frames: Vec<Reference>,
}

impl Clip {
    pub(crate) fn system(&self) -> &str {
        self.name.split('-').next().unwrap_or(&self.name)
    }
}

pub(crate) fn load_clip(dir: &Path) -> Result<Clip> {
    let mut paths: Vec<PathBuf> = fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("frame_") && n.ends_with(".png"))
        })
        .collect();
    paths.sort();
    if paths.is_empty() {
        return Err(format!("no frame_*.png in {}", dir.display()).into());
    }
    let frames = parallel_map(&paths, |p| read_png(p).expect("a readable capture"));
    let name = dir.file_name().and_then(|n| n.to_str()).unwrap_or("clip").to_owned();
    Ok(Clip { name, frames })
}

pub(crate) fn read_png(path: &Path) -> Result<Reference> {
    let decoder = png::Decoder::new(BufReader::new(File::open(path)?));
    let mut reader = decoder.read_info()?;
    let mut buf = vec![0; reader.output_buffer_size().ok_or("png too large")?];
    let info = reader.next_frame(&mut buf)?;
    let (w, h) = (info.width as usize, info.height as usize);
    let channels = match info.color_type {
        png::ColorType::Rgba => 4,
        png::ColorType::Rgb => 3,
        other => return Err(format!("{}: unsupported {other:?}", path.display()).into()),
    };
    let rgb = buf[..w * h * channels].chunks(channels).flat_map(|p| [p[0], p[1], p[2]]).collect();
    Ok(Reference { width: w, height: h, rgb })
}

/// Maps `f` over `items` on every core, keeping the order.
pub(crate) fn parallel_map<T: Sync, U: Send>(items: &[T], f: impl Fn(&T) -> U + Sync) -> Vec<U> {
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let chunk = items.len().div_ceil(threads).max(1);
    std::thread::scope(|s| {
        let handles: Vec<_> =
            items.chunks(chunk).map(|c| s.spawn(|| c.iter().map(&f).collect::<Vec<_>>())).collect();
        handles.into_iter().flat_map(|h| h.join().expect("a worker thread")).collect()
    })
}

/// The frame the encoder is given: RGBA, pixel-duplicated `scale` times.
fn encoder_input(frame: &Reference, scale: usize) -> Vec<u8> {
    let (w, h) = (frame.width * scale, frame.height * scale);
    let mut out = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        for x in 0..w {
            let [r, g, b] = frame.pixel(x / scale, y / scale);
            out.extend_from_slice(&[r, g, b, 255]);
        }
    }
    out
}

/// RGBA → I420 with `yuv.rs` and nothing else: the best a pre-scale could ever do.
fn floor_picture(frame: &Reference, scale: usize) -> Picture {
    let (w, h) = (frame.width * scale, frame.height * scale);
    let (cw, ch) = (chroma_len(w), chroma_len(h));
    let (mut y, mut u, mut v) = (vec![0; w * h], vec![0; cw * ch], vec![0; cw * ch]);
    let planes =
        I420Planes { y: &mut y, y_stride: w, u: &mut u, u_stride: cw, v: &mut v, v_stride: cw };
    rgba_to_i420(&encoder_input(frame, scale), w, h, planes);
    Picture { width: w, height: h, y, u, v }
}

fn picture(d: DecodedFrame) -> Picture {
    Picture { width: d.width as usize, height: d.height as usize, y: d.luma, u: d.u, v: d.v }
}

enum Encoder {
    Vp8(Vp8Encoder, Vp8Decoder),
    X264(H264Encoder, H264Decoder),
}

impl Encoder {
    fn new(v: &Variant) -> Result<Self> {
        Ok(match v.codec {
            Codec::Vp8 => Self::Vp8(
                Vp8Encoder::new(Vp8Config {
                    bitrate_kbps: v.kbps,
                    fps: FPS,
                    screen_content: v.screen_content,
                    ..Vp8Config::default()
                }),
                Vp8Decoder::new()?,
            ),
            Codec::X264 => Self::X264(
                H264Encoder::new(H264Config {
                    bitrate_kbps: v.kbps,
                    fps: FPS,
                    keyframe_interval: Vp8Config::default().keyframe_interval,
                    preference: H264Preference::Only(H264Backend::Software),
                    device: None,
                }),
                H264Decoder::new()?,
            ),
        })
    }

    fn request_keyframe(&mut self) {
        match self {
            Self::Vp8(e, _) => e.request_keyframe(),
            Self::X264(e, _) => e.request_keyframe(),
        }
    }

    fn encode(&mut self, rgba: &[u8], w: u32, h: u32) -> Result<Option<EncodedFrame>> {
        Ok(match self {
            Self::Vp8(e, _) => e.encode(rgba, w, h)?,
            Self::X264(e, _) => e.encode(rgba, w, h)?,
        })
    }

    fn decode(&mut self, data: &[u8]) -> Result<Option<DecodedFrame>> {
        Ok(match self {
            Self::Vp8(_, d) => d.decode(data)?,
            Self::X264(_, d) => d.decode(data)?,
        })
    }
}

/// One encode-and-decode pass over a clip.
struct Run {
    shown: Vec<Arc<Picture>>,
    bytes: usize,
    first_keyframe: usize,
    forced_keyframe: usize,
    dropped: usize,
    enc_ms: Vec<f64>,
    dec_ms: Vec<f64>,
}

fn run(clip: &Clip, v: &Variant) -> Result<Run> {
    let mut encoder = Encoder::new(v)?;
    let first = &clip.frames[0];
    let (w, h) = ((first.width * v.prescale) as u32, (first.height * v.prescale) as u32);
    // Before anything is decoded the viewer sees black.
    let black = Arc::new(Picture {
        width: w as usize,
        height: h as usize,
        y: vec![16; (w * h) as usize],
        u: vec![128; chroma_len(w as usize) * chroma_len(h as usize)],
        v: vec![128; chroma_len(w as usize) * chroma_len(h as usize)],
    });
    let mut out = Run {
        shown: Vec::with_capacity(clip.frames.len()),
        bytes: 0,
        first_keyframe: 0,
        forced_keyframe: 0,
        dropped: 0,
        enc_ms: Vec::with_capacity(clip.frames.len()),
        dec_ms: Vec::new(),
    };
    let mut last = black;
    for (i, frame) in clip.frames.iter().enumerate() {
        let input = encoder_input(frame, v.prescale);
        if i == FORCED_KEYFRAME_AT {
            encoder.request_keyframe();
        }
        let t = Instant::now();
        let encoded = encoder.encode(&input, w, h)?;
        out.enc_ms.push(t.elapsed().as_secs_f64() * 1e3);
        match encoded {
            Some(e) => {
                out.bytes += e.data.len();
                if i == 0 {
                    out.first_keyframe = e.data.len();
                }
                if i == FORCED_KEYFRAME_AT {
                    out.forced_keyframe = e.data.len();
                }
                let t = Instant::now();
                let decoded = encoder.decode(&e.data)?;
                out.dec_ms.push(t.elapsed().as_secs_f64() * 1e3);
                match decoded {
                    Some(d) => last = Arc::new(picture(d)),
                    None => out.dropped += 1,
                }
            }
            // Rate control dropped it: the previous picture stays on screen.
            None => out.dropped += 1,
        }
        out.shown.push(Arc::clone(&last));
    }
    Ok(out)
}

fn mean(xs: &[f64]) -> f64 {
    if xs.is_empty() { 0.0 } else { xs.iter().sum::<f64>() / xs.len() as f64 }
}

fn p95(xs: &[f64]) -> f64 {
    let mut v = xs.to_vec();
    v.sort_by(f64::total_cmp);
    v.get(((v.len() as f64 * 0.95).ceil() as usize).saturating_sub(1)).copied().unwrap_or(0.0)
}

/// Mean of the worst 1% of frames by `key` (`worst_high`: higher is worse).
fn worst_1pct(scores: &[FrameScore], key: impl Fn(&FrameScore) -> f64, worst_high: bool) -> f64 {
    let mut v: Vec<f64> = scores.iter().map(key).collect();
    v.sort_by(f64::total_cmp);
    if worst_high {
        v.reverse();
    }
    let n = (v.len() as f64 * 0.01).ceil().max(1.0) as usize;
    mean(&v[..n.min(v.len())])
}

const CSV_HEADER: &str = "clip,system,codec,prescale,screen_content,target_kbps,actual_kbps,\
de_mean,de_over_2_3_pct,de_p99,psnr_y,psnr_cb,psnr_cr,ssim_y,worst1pct_de,enc_ms_mean,enc_ms_p95,\
dec_ms_mean,dec_ms_p95,keyframe_bytes,dropped,first_keyframe_bytes,worst1pct_psnr_y,\
worst1pct_ssim_y,frames";

/// A CSV row's measured part, after `clip,system,codec,prescale,screen_content,target_kbps`.
#[derive(Debug, Default)]
pub(crate) struct Row {
    pub(crate) actual_kbps: f64,
    pub(crate) enc_ms: Vec<f64>,
    pub(crate) dec_ms: Vec<f64>,
    pub(crate) keyframe_bytes: usize,
    pub(crate) first_keyframe_bytes: usize,
    pub(crate) dropped: usize,
}

pub(crate) fn csv_row(prefix: &str, scores: &[FrameScore], row: &Row) -> String {
    let m = |f: fn(&FrameScore) -> f64| mean(&scores.iter().map(f).collect::<Vec<_>>());
    format!(
        "{prefix},{:.1},{:.4},{:.3},{:.3},{:.3},{:.3},{:.3},{:.5},{:.4},{:.3},{:.3},{:.3},{:.3},{},{},{},{:.3},{:.5},{}",
        row.actual_kbps,
        m(|s| s.de_mean),
        m(|s| s.de_over_pct),
        m(|s| s.de_p99),
        m(|s| s.psnr_y),
        m(|s| s.psnr_cb),
        m(|s| s.psnr_cr),
        m(|s| s.ssim_y),
        worst_1pct(scores, |s| s.de_mean, true),
        mean(&row.enc_ms),
        p95(&row.enc_ms),
        mean(&row.dec_ms),
        p95(&row.dec_ms),
        row.keyframe_bytes,
        row.dropped,
        row.first_keyframe_bytes,
        worst_1pct(scores, |s| s.psnr_y, false),
        worst_1pct(scores, |s| s.ssim_y, false),
        scores.len(),
    )
}

pub(crate) fn open_csv(path: &Path) -> Result<File> {
    let new = !path.exists();
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    if new {
        writeln!(file, "{CSV_HEADER}")?;
    }
    Ok(file)
}

fn bench(o: &Options) -> Result<()> {
    let captures = o.path("captures")?;
    let out = o.path("out")?;
    fs::create_dir_all(&out)?;
    let clips: Vec<String> = match o.get("clips") {
        Some(_) => o.list("clips", "")?,
        None => {
            let mut names: Vec<String> = fs::read_dir(&captures)?
                .filter_map(|e| e.ok())
                .filter(|e| e.path().is_dir())
                .filter_map(|e| e.file_name().into_string().ok())
                .collect();
            names.sort();
            names
        }
    };
    let codecs: Vec<Codec> = o.list("codecs", "vp8,x264")?;
    let prescales: Vec<usize> = o.list("prescale", "1,2")?;
    let modes: Vec<u8> = o.list("screen-content", "0")?;
    let rates: Vec<u32> = o.list("kbps", "800,1500,2500,5000")?;
    let timing_runs: usize = o.get("timing-runs").unwrap_or("2").parse()?;
    let crop_frame: usize = o.get("crop-frame").unwrap_or("900").parse()?;
    let skip_floors = o.get("floors") == Some("no");
    let mut csv = open_csv(&out.join(o.get("csv").unwrap_or("results.csv")))?;
    let to_rgb = YuvToRgb::new();

    for name in &clips {
        let t = Instant::now();
        let clip = load_clip(&captures.join(name))?;
        let crop_at = crop_frame.min(clip.frames.len() - 1);
        let region = crop::pick_region(&clip.frames[crop_at]);
        let crops = out.join("crops").join(&clip.name);
        fs::create_dir_all(&crops)?;
        crop::write_reference(&crops.join("reference.png"), &clip.frames[crop_at], region)?;
        eprintln!(
            "{}: {} frames loaded in {:.1}s",
            clip.name,
            clip.frames.len(),
            t.elapsed().as_secs_f64()
        );

        if !skip_floors {
            for scale in [1, 2] {
                let scores = parallel_map(&clip.frames, |f| {
                    score::score(f, &floor_picture(f, scale), scale, &to_rgb)
                });
                let prefix = format!("{},{},none,{scale},0,0", clip.name, clip.system());
                writeln!(csv, "{}", csv_row(&prefix, &scores, &Row::default()))?;
                let shown = floor_picture(&clip.frames[crop_at], scale);
                let path = crops.join(format!("floor-{scale}x.png"));
                crop::write_crop(&path, &clip.frames[crop_at], &shown, scale, region, &to_rgb)?;
            }
        }

        for &codec in &codecs {
            for &prescale in &prescales {
                for &screen_content in &modes {
                    if codec == Codec::X264 && screen_content != 0 {
                        continue; // x264 has no such mode
                    }
                    for &kbps in &rates {
                        let v = Variant { codec, prescale, screen_content, kbps };
                        let t = Instant::now();
                        let mut best = run(&clip, &v)?;
                        for _ in 1..timing_runs {
                            let again = run(&clip, &v)?;
                            if again.bytes != best.bytes {
                                eprintln!(
                                    "  {}: runs differ ({} vs {} bytes)",
                                    v.name(),
                                    again.bytes,
                                    best.bytes
                                );
                            }
                            if mean(&again.enc_ms) < mean(&best.enc_ms) {
                                best.enc_ms = again.enc_ms;
                            }
                            if mean(&again.dec_ms) < mean(&best.dec_ms) {
                                best.dec_ms = again.dec_ms;
                            }
                        }
                        let encoded = t.elapsed().as_secs_f64();
                        let pairs: Vec<(usize, Arc<Picture>)> =
                            best.shown.iter().cloned().enumerate().collect();
                        let scores = parallel_map(&pairs, |(i, p)| {
                            score::score(&clip.frames[*i], p, prescale, &to_rgb)
                        });
                        let seconds = clip.frames.len() as f64 / f64::from(FPS);
                        let row = Row {
                            actual_kbps: best.bytes as f64 * 8.0 / seconds / 1000.0,
                            enc_ms: best.enc_ms,
                            dec_ms: best.dec_ms,
                            keyframe_bytes: best.forced_keyframe,
                            first_keyframe_bytes: best.first_keyframe,
                            dropped: best.dropped,
                        };
                        let prefix = format!(
                            "{},{},{},{prescale},{screen_content},{kbps}",
                            clip.name,
                            clip.system(),
                            codec.name()
                        );
                        writeln!(csv, "{}", csv_row(&prefix, &scores, &row))?;
                        csv.flush()?;
                        let path = crops.join(format!("{}.png", v.name()));
                        crop::write_crop(
                            &path,
                            &clip.frames[crop_at],
                            &best.shown[crop_at],
                            prescale,
                            region,
                            &to_rgb,
                        )?;
                        eprintln!(
                            "  {:<22} {:>7.0} kbit/s  dE {:.3}  encode {encoded:.1}s  score {:.1}s",
                            v.name(),
                            row.actual_kbps,
                            mean(&scores.iter().map(|s| s.de_mean).collect::<Vec<_>>()),
                            t.elapsed().as_secs_f64() - encoded,
                        );
                    }
                }
            }
        }
    }
    Ok(())
}
