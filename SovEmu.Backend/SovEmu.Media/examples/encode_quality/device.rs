//! `score-dump`: frames a client decoded, scored exactly as the bench scores its own decodes.
//!
//! ```text
//! encode_quality score-dump --captures CLIP_DIR --dump DUMP_DIR --out DIR \
//!   --label firetv,vp8,1,0,5000 [--fps 60] [--csv device.csv]
//! ```
//!
//! A dump (the Android app's debug dump, pulled with `adb pull`) is a folder of
//! `frame_NNNNNN.yuv`, each one decoded picture as packed I420, and `frames.csv`
//! (`index,timestamp_ns,received_ns,width,height`). The replay drew clip frame *i* at tick *i*:
//! the first dumped frame is matched against the whole clip, and each one after it is placed as
//! many frames on from the one before as its time says, then moved to the closest reference
//! within ±2. A reference frame no dumped frame landed on was never shown: the previous picture
//! stayed on screen, and it is scored that way (and counted in `dropped`).
//!
//! `--label` is the row's `codec,prescale,screen_content,target_kbps` with a device name first
//! (it fills the CSV's `system` column as `<system>@<device>`).

use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write as _};
use std::path::Path;
use std::sync::Arc;

use sovemu_media::yuv::chroma_len;

use crate::colour::{YuvToRgb, ycbcr};
use crate::score::{Picture, Reference};
use crate::{Options, Result, Row, crop, csv_row, load_clip, open_csv, parallel_map, score};

/// Reference frames either side of a timestamp's placement that are checked.
const SEARCH: i64 = 2;

struct Dumped {
    timestamp_ns: i64,
    picture: Arc<Picture>,
}

fn load_dump(dir: &Path) -> Result<Vec<Dumped>> {
    let mut out = Vec::new();
    for line in BufReader::new(File::open(dir.join("frames.csv"))?).lines().skip(1) {
        let line = line?;
        let f: Vec<&str> = line.split(',').map(str::trim).collect();
        // `timestamp_ns` is the frame's own time; a client that has none (Android's WebRTC
        // leaves received frames at 0) gives the time it arrived as `received_ns`.
        let (index, ts, received, w, h) = match f[..] {
            [i, t, w, h] => (i, t, "0", w, h),
            [i, t, r, w, h] => (i, t, r, w, h),
            _ => return Err(format!("bad line: {line}").into()),
        };
        // Arrival times come in bursts (a frame waits for its packets), too uneven to place
        // frames by: they are kept for reading, and a frame with no time of its own is placed
        // as the one after the last.
        let _ = received;
        let (w, h): (usize, usize) = (w.parse()?, h.parse()?);
        let bytes = fs::read(dir.join(format!("frame_{:06}.yuv", index.parse::<usize>()?)))?;
        let c = chroma_len(w) * chroma_len(h);
        if bytes.len() < w * h + 2 * c {
            return Err(format!("frame {index}: {} bytes for {w}x{h}", bytes.len()).into());
        }
        out.push(Dumped {
            timestamp_ns: ts.parse()?,
            picture: Arc::new(Picture {
                width: w,
                height: h,
                y: bytes[..w * h].to_vec(),
                u: bytes[w * h..w * h + c].to_vec(),
                v: bytes[w * h + c..w * h + 2 * c].to_vec(),
            }),
        });
    }
    out.sort_by_key(|d| d.timestamp_ns);
    Ok(out)
}

/// Luma difference between a reference and a picture at `scale`, sampled every native pixel.
fn distance(reference: &Reference, picture: &Picture, scale: usize) -> f64 {
    let mut sum = 0.0;
    for y in 0..reference.height {
        for x in 0..reference.width {
            let r = ycbcr(reference.pixel(x, y))[0];
            let p = f64::from(picture.y[(y * scale) * picture.width + x * scale]);
            sum += (r - p).powi(2);
        }
    }
    sum / (reference.width * reference.height) as f64
}

/// The reference in `range` closest to `picture`; of equally close ones (a run of identical
/// frames, such as a black screen), the one nearest `prefer`.
fn closest(
    frames: &[Reference],
    picture: &Picture,
    scale: usize,
    range: std::ops::Range<i64>,
    prefer: i64,
) -> usize {
    let n = frames.len() as i64;
    range
        .filter(|i| (0..n).contains(i))
        .map(|i| (i, distance(&frames[i as usize], picture, scale)))
        .min_by(|a, b| a.1.total_cmp(&b.1).then((a.0 - prefer).abs().cmp(&(b.0 - prefer).abs())))
        .map_or(0, |(i, _)| i as usize)
}

pub(crate) fn score_dump(o: &Options) -> Result<()> {
    let clip = load_clip(&o.path("captures")?)?;
    let dump = load_dump(&o.path("dump")?)?;
    let out = o.path("out")?;
    fs::create_dir_all(&out)?;
    let label = o.get("label").ok_or("--label device,codec,prescale,screen_content,kbps")?;
    let (device, rest) = label.split_once(',').ok_or("--label needs a device first")?;
    let fps: f64 = o.get("fps").unwrap_or("60").parse()?;
    let first = dump.first().ok_or("the dump is empty")?;
    let scale = (first.picture.width as f64 / clip.frames[0].width as f64).round() as usize;
    if !(1..=2).contains(&scale) {
        return Err(format!(
            "dump is {}px wide for a {}px clip",
            first.picture.width, clip.frames[0].width
        )
        .into());
    }

    // Place every dumped frame after the one before it: as many frames on as its timestamp says
    // (one, when the dump has no times), then the closest reference within ±2 of that, never
    // before the previous one. Placing from the previous frame, not the first, keeps a core
    // that runs at 60.1 fps from drifting against `--fps`.
    let start = closest(&clip.frames, &first.picture, scale, 0..clip.frames.len() as i64, 0);
    let mut placed: Vec<(usize, i64)> = vec![(start, 0)];
    for pair in dump.windows(2) {
        let (prev, d) = (placed.last().map_or(start, |p| p.0) as i64, &pair[1]);
        let dt = pair[1].timestamp_ns - pair[0].timestamp_ns;
        let step = if dt > 0 { ((dt as f64 * fps / 1e9).round() as i64).max(1) } else { 1 };
        let predicted = prev + step;
        if predicted - SEARCH >= clip.frames.len() as i64 {
            break; // the clip looped: score one pass
        }
        let lo = (predicted - SEARCH).max(prev);
        let at = closest(&clip.frames, &d.picture, scale, lo..predicted + SEARCH + 1, predicted);
        placed.push((at, at as i64 - predicted));
    }
    let dump = &dump[..placed.len()];
    let moved = placed.iter().filter(|(_, d)| *d != 0).count();
    eprintln!(
        "{} dumped frames from reference {start}; {moved} moved from their predicted place",
        dump.len()
    );

    // What was on screen at each reference frame from the first placed to the last.
    let (lo, hi) = (placed[0].0, placed.iter().map(|p| p.0).max().unwrap_or(start));
    let mut shown: Vec<(usize, Arc<Picture>)> = Vec::with_capacity(hi - lo + 1);
    let mut missing = 0;
    let mut next = 0;
    let mut current = Arc::clone(&dump[0].picture);
    for k in lo..=hi {
        let mut landed = false;
        while next < dump.len() && placed[next].0 <= k {
            current = Arc::clone(&dump[next].picture);
            landed |= placed[next].0 == k;
            next += 1;
        }
        missing += usize::from(!landed);
        shown.push((k, Arc::clone(&current)));
    }

    let to_rgb = YuvToRgb::new();
    let scores = parallel_map(&shown, |(k, p)| score::score(&clip.frames[*k], p, scale, &to_rgb));
    if let Some(path) = o.get("per-frame") {
        // Each reference frame's score and the dumped frame shown for it, to find bad stretches.
        let mut f = File::create(path)?;
        writeln!(f, "reference,de_mean,de_over_2_3_pct,psnr_y")?;
        for ((k, _), s) in shown.iter().zip(&scores) {
            writeln!(f, "{k},{:.3},{:.2},{:.2}", s.de_mean, s.de_over_pct, s.psnr_y)?;
        }
    }
    let mut csv = open_csv(&out.join(o.get("csv").unwrap_or("device.csv")))?;
    let prefix = format!("{},{}@{device},{rest}", clip.name, clip.system());
    let row = Row { dropped: missing, ..Row::default() };
    writeln!(csv, "{}", csv_row(&prefix, &scores, &row))?;

    let crop_at: usize = o.get("crop-frame").unwrap_or("900").parse()?;
    if let Some((k, p)) = shown.iter().find(|(k, _)| *k >= crop_at) {
        let region = crop::pick_region(&clip.frames[*k]);
        let dir = out.join("crops").join(&clip.name);
        fs::create_dir_all(&dir)?;
        let name = format!("{device}-{}.png", rest.replace(',', "-"));
        crop::write_crop(&dir.join(name), &clip.frames[*k], p, scale, region, &to_rgb)?;
    }
    eprintln!("scored {} reference frames ({lo}-{hi}); {missing} never shown", shown.len());
    Ok(())
}
