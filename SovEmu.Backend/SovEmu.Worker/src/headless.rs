//! `sovemu-worker headless`: run a core with no client and dump its output.
//!
//! Writes into `--out`:
//! - `frame_NNNNNN.png` every `--png-every` frames, plus `last.png`;
//! - `audio.wav`, the whole run's audio at the core's sample rate (rounded to whole Hz);
//! - `summary.json`, the core's info and how long each `retro_run` took.

use std::fs::{self, File};
use std::io::BufWriter;
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde::Serialize;
use sovemu_retro_host::{Core, CoreOptions};

use crate::HeadlessArgs;
use crate::pacing::Pacer;
use crate::runahead::RunAhead;
use crate::schedule::buttons_at;

#[derive(Debug, Serialize)]
struct Summary {
    core: CoreSummary,
    av: AvSummary,
    frames_run: u64,
    frames_duped: u64,
    shutdown_requested: bool,
    realtime: bool,
    wall_seconds: f64,
    effective_fps: f64,
    pacer_resyncs: u64,
    run_frame_ms: TimingSummary,
    audio: AudioSummary,
}

#[derive(Debug, Serialize)]
struct CoreSummary {
    name: String,
    version: String,
    valid_extensions: Vec<String>,
    need_fullpath: bool,
    variables: Vec<(String, String)>,
}

#[derive(Debug, Serialize)]
struct AvSummary {
    width: u32,
    height: u32,
    max_width: u32,
    max_height: u32,
    aspect_ratio: f32,
    fps: f64,
    sample_rate: f64,
}

#[derive(Debug, Serialize)]
struct TimingSummary {
    mean: f64,
    p50: f64,
    p99: f64,
    max: f64,
}

#[derive(Debug, Serialize)]
struct AudioSummary {
    stereo_frames: u64,
    expected_stereo_frames: f64,
    wav_sample_rate: u32,
}

impl TimingSummary {
    fn from_durations(mut samples: Vec<Duration>) -> Self {
        if samples.is_empty() {
            return Self { mean: 0.0, p50: 0.0, p99: 0.0, max: 0.0 };
        }
        samples.sort_unstable();
        let ms = |d: Duration| d.as_secs_f64() * 1000.0;
        let pct = |p: f64| ms(samples[((samples.len() - 1) as f64 * p).round() as usize]);
        let total: Duration = samples.iter().sum();
        Self {
            mean: ms(total) / samples.len() as f64,
            p50: pct(0.50),
            p99: pct(0.99),
            max: ms(*samples.last().unwrap()),
        }
    }
}

pub(crate) fn run(args: HeadlessArgs) -> Result<()> {
    fs::create_dir_all(&args.out)
        .with_context(|| format!("cannot create output directory {}", args.out.display()))?;
    let save_dir = args.save_dir.clone().unwrap_or_else(|| args.out.join("saves"));
    fs::create_dir_all(&save_dir)?;
    fs::create_dir_all(&args.system_dir)?;

    let system_dir = crate::sysdir::SystemDir::for_core(
        &args.core,
        &args.system_dir.canonicalize()?,
        &args.bios_path,
    )?;
    let options = CoreOptions {
        system_dir: system_dir.path().to_owned(),
        save_dir: save_dir.canonicalize()?,
        variables: args.variables.clone(),
        gpu: args.gpu.0.clone(),
    };
    let mut core = Core::load(&args.core, options)
        .with_context(|| format!("loading core {}", args.core.display()))?;
    let info = core.system_info().clone();
    tracing::info!("loaded {} {}", info.library_name, info.library_version);

    let av = *core.load_game(args.rom.as_deref()).context("loading content")?;
    tracing::info!(
        "{}x{} @ {:.4} fps, audio {} Hz",
        av.base_width,
        av.base_height,
        av.fps,
        av.sample_rate
    );

    let wav_rate = av.sample_rate.round() as u32;
    let mut wav = hound::WavWriter::create(
        args.out.join("audio.wav"),
        hound::WavSpec {
            channels: 2,
            sample_rate: wav_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )?;

    let mut timings = Vec::with_capacity(args.frames as usize);
    let mut stereo_frames = 0u64;
    let mut duped = 0u64;
    let mut frames_run = 0u64;
    let mut rgba = Vec::new();
    // The picture in `rgba` that the last frame showed, by its size.
    let mut last_picture = None;
    let started = Instant::now();
    let mut pacer = args.realtime.then(|| Pacer::new(av.fps, started));
    let mut run_ahead = RunAhead::new(args.run_ahead, av.fps);

    for n in 0..args.frames {
        let buttons = buttons_at(&args.holds, n);
        let t = Instant::now();
        let out = core.run_frame(&[buttons])?;
        frames_run += 1;

        if out.av_changed.is_some() {
            tracing::warn!("core changed geometry/timing at frame {n}; summary shows the initial");
        }
        if !out.video_updated {
            duped += 1;
        }
        for &sample in out.audio {
            wav.write_sample(sample)?;
        }
        stereo_frames += out.audio.len() as u64 / 2;

        let png = args.png_every > 0 && n % args.png_every == 0;
        let last = n + 1 == args.frames;
        let mut picture = None;
        if (png || last) && out.video.width > 0 {
            out.video.write_rgba8(&mut rgba);
            picture = Some((out.video.width, out.video.height));
        }
        if let Some(ahead) = run_ahead.picture(&mut core, &[buttons], &mut rgba)? {
            picture = Some(ahead);
        }
        timings.push(t.elapsed());
        if let (true, Some((width, height))) = (png, picture) {
            write_png(&args.out.join(format!("frame_{n:06}.png")), width, height, &rgba)?;
        }
        if last {
            last_picture = picture;
        }

        if core.shutdown_requested() {
            tracing::info!("core requested shutdown at frame {n}");
            break;
        }
        if let Some(pacer) = &mut pacer {
            pacer.wait();
        }
    }
    let wall = started.elapsed().as_secs_f64();
    wav.finalize()?;

    if let Some((width, height)) = last_picture {
        write_png(&args.out.join("last.png"), width, height, &rgba)?;
    }

    let summary = Summary {
        core: CoreSummary {
            name: info.library_name,
            version: info.library_version,
            valid_extensions: info.valid_extensions,
            need_fullpath: info.need_fullpath,
            variables: core.variables().into_iter().map(|v| (v.key, v.value)).collect(),
        },
        av: AvSummary {
            width: av.base_width,
            height: av.base_height,
            max_width: av.max_width,
            max_height: av.max_height,
            aspect_ratio: av.aspect_ratio,
            fps: av.fps,
            sample_rate: av.sample_rate,
        },
        frames_run,
        frames_duped: duped,
        shutdown_requested: core.shutdown_requested(),
        realtime: args.realtime,
        wall_seconds: wall,
        effective_fps: if wall > 0.0 { frames_run as f64 / wall } else { 0.0 },
        pacer_resyncs: pacer.map_or(0, |p| p.resyncs),
        run_frame_ms: TimingSummary::from_durations(timings),
        audio: AudioSummary {
            stereo_frames,
            expected_stereo_frames: frames_run as f64 * av.sample_rate / av.fps,
            wav_sample_rate: wav_rate,
        },
    };
    let json = serde_json::to_string_pretty(&summary)?;
    fs::write(args.out.join("summary.json"), &json)?;
    println!("{json}");
    Ok(())
}

fn write_png(path: &Path, width: u32, height: u32, rgba: &[u8]) -> Result<()> {
    let file = BufWriter::new(File::create(path)?);
    let mut encoder = png::Encoder::new(file, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(rgba)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timing_percentiles() {
        let samples = (1..=100).map(Duration::from_millis).collect();
        let t = TimingSummary::from_durations(samples);
        assert!((t.mean - 50.5).abs() < 1e-9);
        assert!((t.p50 - 51.0).abs() < 1e-9 || (t.p50 - 50.0).abs() < 1e-9);
        assert!((t.p99 - 99.0).abs() < 1e-9);
        assert!((t.max - 100.0).abs() < 1e-9);
        assert_eq!(TimingSummary::from_durations(Vec::new()).max, 0.0);
    }
}
