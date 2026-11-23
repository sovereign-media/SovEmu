//! `sovemu-worker encoders`: which video encoders work on this host, and what a frame costs
//! with each.
//!
//! The server runs this once at startup, before any game can start (codecs only ever load in
//! workers), and keeps the answer: it orders the codecs it offers a client by it. As Sovereign's
//! startup probe, an encoder's presence is not proof that it works, so each one is run for
//! real; unlike it, each is also timed.
//!
//! Every encoder runs exactly as a session would run it (the same settings, the same threads),
//! on a moving picture at console sizes, and the time measured is the whole step a session
//! pays: colour conversion, the upload to a GPU where there is one, and the encode. They run
//! one after another, never side by side, so one encoder's threads do not slow another's
//! timing. The whole test fits a budget (`--budget-ms`, 10 seconds): each encoder has an equal
//! share, and one still running when its share is up is reported as not working.

use std::io::Write;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::Result;
use serde::Serialize;
use sovemu_media::{
    H264Backend, H264Config, H264Encoder, H264Preference, Vp8Config, Vp8Encoder, X264_THREADS,
};

/// The picture sizes timed: a 2D console's, a PS2's, and 720p for a 3D system scaled up.
const SIZES: [(u32, u32); 3] = [(256, 224), (640, 448), (1280, 720)];
/// Frames encoded at each size before timing starts: the keyframe, and an encoder settling.
const WARM_UP: usize = 6;
/// Frames timed at each size.
const TIMED: usize = 60;
/// Different pictures the frames cycle through.
const PICTURES: usize = 12;
/// The bitrate tested at, kbit/s: a stream's default start.
const KBPS: u32 = 5000;

/// One size's result for an encoder.
#[derive(Debug, Serialize)]
struct Timing {
    width: u32,
    height: u32,
    /// Frames timed.
    frames: usize,
    /// Milliseconds a frame, on average and at the 95th percentile.
    mean_ms: f64,
    p95_ms: f64,
}

/// One encoder's result.
#[derive(Debug, Serialize)]
struct Encoder {
    /// `vp8` or `h264`.
    codec: &'static str,
    /// `libvpx`, `vaapi`, `nvenc` or `x264`.
    encoder: &'static str,
    /// It encodes on a GPU.
    hardware: bool,
    /// CPU threads a frame is encoded on; none for a GPU's encoder.
    threads: Option<u32>,
    works: bool,
    /// Why it does not work: it did not open, failed, held frames back, or ran out of time.
    error: Option<String>,
    /// A row for each size it finished.
    timings: Vec<Timing>,
}

#[derive(Debug, Serialize)]
struct Report {
    encoders: Vec<Encoder>,
    /// How long the whole test took, and how long it was given.
    took_ms: u64,
    budget_ms: u64,
}

/// An encoder to test.
#[derive(Debug, Clone, Copy)]
enum Candidate {
    Vp8,
    H264(H264Backend),
}

impl Candidate {
    /// Every encoder a session could use on this platform, in the order they are tested.
    fn all() -> Vec<Self> {
        let mut all = vec![Self::Vp8];
        if cfg!(target_os = "linux") {
            all.extend([Self::H264(H264Backend::Vaapi), Self::H264(H264Backend::Nvenc)]);
        }
        all.push(Self::H264(H264Backend::Software));
        all
    }

    fn describe(self) -> Encoder {
        let (codec, encoder, hardware, threads) = match self {
            Self::Vp8 => ("vp8", "libvpx", false, Some(Vp8Config::default().threads)),
            Self::H264(backend) => {
                let threads = (!backend.is_hardware()).then_some(X264_THREADS);
                ("h264", backend.name(), backend.is_hardware(), threads)
            }
        };
        Encoder {
            codec,
            encoder,
            hardware,
            threads,
            works: false,
            error: None,
            timings: Vec::new(),
        }
    }
}

/// A moving picture: textured blocks that scroll, with a little noise, as a 3D game's scene.
fn picture(width: u32, height: u32, n: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let (sx, sy) = (x + n * 3, y + n);
            let block = ((sx / 16) * 73 + (sy / 16) * 151) % 251;
            let texture = ((sx * 7) ^ (sy * 13)) % 61;
            let noise =
                (x.wrapping_mul(2_654_435_761).wrapping_add(y * 40_503 + n * 9_973) >> 13) % 9;
            let v = (block + texture + noise) as u8;
            out.extend_from_slice(&[v, v.wrapping_mul(3), v.wrapping_add(90), 255]);
        }
    }
    out
}

/// What a test thread tells the main one.
enum Progress {
    Timing(Timing),
    Done(Result<(), String>),
}

/// Runs one encoder over every size until `deadline`, reporting each size as it finishes.
fn test(
    candidate: Candidate,
    device: Option<String>,
    pictures: &[Vec<Vec<u8>>],
    deadline: Instant,
    progress: &mpsc::Sender<Progress>,
) -> Result<(), String> {
    let mut vp8 = None;
    let mut h264 = None;
    match candidate {
        Candidate::Vp8 => {
            vp8 = Some(Vp8Encoder::new(Vp8Config { bitrate_kbps: KBPS, ..Vp8Config::default() }));
        }
        Candidate::H264(backend) => {
            h264 = Some(H264Encoder::new(H264Config {
                bitrate_kbps: KBPS,
                preference: H264Preference::Only(backend),
                device,
                ..H264Config::default()
            }));
        }
    }
    for (&(width, height), frames) in SIZES.iter().zip(pictures) {
        let mut times = Vec::with_capacity(TIMED);
        for n in 0..WARM_UP + TIMED {
            if Instant::now() >= deadline {
                return Err("ran out of time: too slow to be of use".into());
            }
            let rgba = &frames[n % frames.len()];
            let from = Instant::now();
            let encoded = match (&mut vp8, &mut h264) {
                (Some(encoder), _) => encoder.encode(rgba, width, height),
                (_, Some(encoder)) => encoder.encode(rgba, width, height),
                _ => unreachable!("one encoder is made above"),
            };
            let took = from.elapsed();
            encoded.map_err(|e| e.to_string())?;
            // Asked for one encoder only: a fall back to x264 is this one not working.
            if let (Candidate::H264(backend), Some(encoder)) = (candidate, &h264)
                && encoder.backend() != Some(backend)
            {
                let why = encoder.fallbacks().map(|(_, why)| why.to_owned()).next();
                return Err(why.unwrap_or_else(|| "it did not open".into()));
            }
            if n >= WARM_UP {
                times.push(took.as_secs_f64() * 1000.0);
            }
        }
        times.sort_by(f64::total_cmp);
        let timing = Timing {
            width,
            height,
            frames: times.len(),
            mean_ms: round(times.iter().sum::<f64>() / times.len() as f64),
            p95_ms: round(times[(times.len() * 95 / 100).min(times.len() - 1)]),
        };
        let _ = progress.send(Progress::Timing(timing));
    }
    Ok(())
}

/// To a hundredth of a millisecond.
fn round(ms: f64) -> f64 {
    (ms * 100.0).round() / 100.0
}

pub(crate) fn run(gpu: &crate::GpuArg, budget_ms: u64) -> Result<()> {
    // Drivers may print to stdout; the JSON report must be the only thing on it.
    let mut out = sovemu_retro_host::take_stdout()?;
    let started = Instant::now();
    let budget = Duration::from_millis(budget_ms);
    let device = match &gpu.0 {
        Some(sovemu_retro_host::GpuChoice::Device(node)) => Some(node.clone()),
        _ => None,
    };
    let pictures: std::sync::Arc<Vec<Vec<Vec<u8>>>> = std::sync::Arc::new(
        SIZES
            .iter()
            .map(|&(w, h)| (0..PICTURES as u32).map(|n| picture(w, h, n)).collect())
            .collect(),
    );

    let candidates = Candidate::all();
    let mut encoders = Vec::with_capacity(candidates.len());
    for (i, candidate) in candidates.iter().copied().enumerate() {
        let mut result = candidate.describe();
        // An equal share of what is left: an encoder that fails at once leaves its share to
        // the rest.
        let left = budget.saturating_sub(started.elapsed());
        let share = left / (candidates.len() - i) as u32;
        let deadline = Instant::now() + share;
        let (tx, rx) = mpsc::channel();
        let (device, pictures) = (device.clone(), pictures.clone());
        // On a thread of its own, so an encoder stuck inside its library cannot hold the
        // report up: it is left behind, and ends with the process.
        std::thread::spawn(move || {
            let done = test(candidate, device, &pictures, deadline, &tx);
            let _ = tx.send(Progress::Done(done));
        });
        // A little past the deadline, for the frame being encoded as it passes.
        let patience = deadline + Duration::from_millis(100);
        loop {
            match rx.recv_timeout(patience.saturating_duration_since(Instant::now())) {
                Ok(Progress::Timing(timing)) => result.timings.push(timing),
                Ok(Progress::Done(Ok(()))) => {
                    result.works = true;
                    break;
                }
                Ok(Progress::Done(Err(why))) => {
                    result.error = Some(why);
                    break;
                }
                Err(_) => {
                    result.error = Some("ran out of time: it stopped answering".into());
                    break;
                }
            }
        }
        encoders.push(result);
    }

    let report = Report { encoders, took_ms: started.elapsed().as_millis() as u64, budget_ms };
    writeln!(out, "{}", serde_json::to_string_pretty(&report)?)?;
    out.flush()?;
    // A stuck encoder's thread may still be running: leave without waiting for it.
    std::process::exit(0);
}
