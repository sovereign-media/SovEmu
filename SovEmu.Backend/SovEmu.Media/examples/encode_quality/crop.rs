//! The visual check: crops of one colour-edge region at 4× native size, side by side: the
//! reference, the picture the viewer sees and a ΔE2000 heatmap (0 black, 10 or more white).

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use crate::Result;
use crate::colour::{YuvToRgb, ycbcr};
use crate::score::{Picture, Reference, delta_e_map, shown_rgb};

/// The crop's size in native pixels.
const CROP_W: usize = 48;
const CROP_H: usize = 36;
/// Grid pixels (2× native) are drawn this many times over: 4× native.
const ZOOM: usize = 2;
const GAP: usize = 4;
const HEAT_MAX: f64 = 10.0;

/// A region of the 2× grid: x, y, width, height.
pub(crate) type Region = (usize, usize, usize, usize);

/// The native window with the most chroma change between neighbouring pixels: where 4:2:0
/// fails. Returned in grid pixels.
pub(crate) fn pick_region(frame: &Reference) -> Region {
    let (w, h) = (frame.width, frame.height);
    let (cw, ch) = (CROP_W.min(w), CROP_H.min(h));
    let chroma: Vec<[f64; 2]> = (0..w * h)
        .map(|i| {
            let c = ycbcr(frame.pixel(i % w, i / w));
            [c[1], c[2]]
        })
        .collect();
    let edge = |x: usize, y: usize| {
        let c = chroma[y * w + x];
        let mut e = 0.0;
        if x + 1 < w {
            let r = chroma[y * w + x + 1];
            e += (c[0] - r[0]).abs() + (c[1] - r[1]).abs();
        }
        if y + 1 < h {
            let d = chroma[(y + 1) * w + x];
            e += (c[0] - d[0]).abs() + (c[1] - d[1]).abs();
        }
        e
    };
    let edges: Vec<f64> = (0..w * h).map(|i| edge(i % w, i / w)).collect();
    let mut best = (0.0, 0, 0);
    for y0 in (0..=h - ch).step_by(4) {
        for x0 in (0..=w - cw).step_by(4) {
            let sum: f64 = (y0..y0 + ch)
                .flat_map(|y| (x0..x0 + cw).map(move |x| (x, y)))
                .map(|(x, y)| edges[y * w + x])
                .sum();
            if sum > best.0 {
                best = (sum, x0, y0);
            }
        }
    }
    (best.1 * 2, best.2 * 2, cw * 2, ch * 2)
}

fn reference_rgb(frame: &Reference, region: Region) -> Vec<[u8; 3]> {
    let (x0, y0, rw, rh) = region;
    (y0..y0 + rh).flat_map(|y| (x0..x0 + rw).map(move |x| frame.pixel(x / 2, y / 2))).collect()
}

fn heat(de: f64) -> [u8; 3] {
    let v = ((de / HEAT_MAX).min(1.0) * 255.0).round() as u8;
    [v, v, v]
}

/// Writes panels of `region`-sized RGB side by side, each zoomed.
fn write_panels(path: &Path, region: Region, panels: &[Vec<[u8; 3]>]) -> Result<()> {
    let (_, _, rw, rh) = region;
    let (pw, ph) = (rw * ZOOM, rh * ZOOM);
    let width = pw * panels.len() + GAP * (panels.len() - 1);
    let mut rgb = vec![255u8; width * ph * 3];
    for (n, panel) in panels.iter().enumerate() {
        let left = n * (pw + GAP);
        for y in 0..ph {
            for x in 0..pw {
                let p = panel[(y / ZOOM) * rw + x / ZOOM];
                let i = (y * width + left + x) * 3;
                rgb[i..i + 3].copy_from_slice(&p);
            }
        }
    }
    let mut encoder =
        png::Encoder::new(BufWriter::new(File::create(path)?), width as u32, ph as u32);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(&rgb)?;
    Ok(())
}

/// The reference alone, for the report's header row.
pub(crate) fn write_reference(path: &Path, frame: &Reference, region: Region) -> Result<()> {
    write_panels(path, region, &[reference_rgb(frame, region)])
}

/// Reference, shown picture and heatmap.
pub(crate) fn write_crop(
    path: &Path,
    frame: &Reference,
    shown: &Picture,
    scale: usize,
    region: Region,
    to_rgb: &YuvToRgb,
) -> Result<()> {
    let heatmap = delta_e_map(frame, shown, scale, to_rgb, region).into_iter().map(heat).collect();
    let panels = [reference_rgb(frame, region), shown_rgb(shown, scale, to_rgb, region), heatmap];
    write_panels(path, region, &panels)
}
