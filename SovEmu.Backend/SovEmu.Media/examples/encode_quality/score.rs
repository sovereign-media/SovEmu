//! Scoring one shown picture against the core's own frame, on a grid of twice the native size.
//!
//! The reference is the core's RGB frame, pixel-duplicated to the grid. A picture at native
//! size (`scale` 1) is pixel-duplicated too, as the web client's `pixelated` display does; one
//! at 2× is already on the grid. Decoded chroma is upsampled by nearest neighbour for every
//! variant alike (a real client's display upsamples it bilinearly).
//!
//! At `scale` 1 every 2×2 block of the grid holds one reference pixel and one decoded pixel,
//! so the per-pixel metrics are computed once per block: means, shares and percentiles over the
//! grid are the same. SSIM's windows span blocks, so it always runs on the full grid.

use crate::colour::{YuvToRgb, ciede2000, lab, ycbcr};

/// A shown picture: I420 planes, packed, `width` × `height` (at least the clip's size × scale).
#[derive(Debug, Clone)]
pub(crate) struct Picture {
    pub(crate) width: usize,
    pub(crate) height: usize,
    pub(crate) y: Vec<u8>,
    pub(crate) u: Vec<u8>,
    pub(crate) v: Vec<u8>,
}

impl Picture {
    fn chroma_width(&self) -> usize {
        self.width.div_ceil(2)
    }

    /// The pixel at (x, y) as Y, U, V (chroma by nearest neighbour).
    fn yuv(&self, x: usize, y: usize) -> (u8, u8, u8) {
        debug_assert!(x < self.width && y < self.height, "({x}, {y}) outside the picture");
        let c = (y / 2) * self.chroma_width() + x / 2;
        (self.y[y * self.width + x], self.u[c], self.v[c])
    }
}

/// The core's frame: packed RGB at native size.
#[derive(Debug, Clone)]
pub(crate) struct Reference {
    pub(crate) width: usize,
    pub(crate) height: usize,
    pub(crate) rgb: Vec<u8>,
}

impl Reference {
    pub(crate) fn pixel(&self, x: usize, y: usize) -> [u8; 3] {
        let i = (y * self.width + x) * 3;
        [self.rgb[i], self.rgb[i + 1], self.rgb[i + 2]]
    }
}

/// ΔE2000 above this is visible: about one just-noticeable difference.
pub(crate) const VISIBLE_DE: f64 = 2.3;
/// Per-frame PSNR is capped here, so an exact frame doesn't make a clip's mean infinite.
const PSNR_CAP: f64 = 100.0;
/// Histogram resolution for the ΔE percentile.
const DE_BIN: f64 = 0.01;
const DE_BINS: usize = 10_000;

/// One frame's scores.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct FrameScore {
    pub(crate) de_mean: f64,
    /// Share of grid pixels with ΔE2000 > 2.3, in percent.
    pub(crate) de_over_pct: f64,
    pub(crate) de_p99: f64,
    pub(crate) psnr_y: f64,
    pub(crate) psnr_cb: f64,
    pub(crate) psnr_cr: f64,
    pub(crate) ssim_y: f64,
}

fn psnr(sum_sq: f64, n: usize) -> f64 {
    let mse = sum_sq / n as f64;
    if mse <= 0.0 { PSNR_CAP } else { (10.0 * (255.0 * 255.0 / mse).log10()).min(PSNR_CAP) }
}

/// The colour difference of one pixel pair: zero without any work when they are equal, which
/// most pixel-art pixels are once the codec has settled.
pub(crate) fn delta_e(a: [u8; 3], b: [u8; 3]) -> f64 {
    if a == b { 0.0 } else { ciede2000(lab(a), lab(b)) }
}

/// Scores `shown` (at `scale` times the reference's size) against `reference`.
pub(crate) fn score(
    reference: &Reference,
    shown: &Picture,
    scale: usize,
    to_rgb: &YuvToRgb,
) -> FrameScore {
    let (w, h) = (reference.width, reference.height);
    // At scale 1 a native pixel stands for its whole 2×2 block; at 2 each grid pixel is its own.
    let (sw, sh) = (w * scale, h * scale);
    let mut histogram = vec![0u32; DE_BINS + 1];
    let (mut de_sum, mut over, mut sq_y, mut sq_cb, mut sq_cr) = (0.0, 0usize, 0.0, 0.0, 0.0);
    for y in 0..sh {
        for x in 0..sw {
            let r = reference.pixel(x / scale, y / scale);
            let (yy, u, v) = shown.yuv(x, y);
            let s = to_rgb.rgb(yy, u, v);
            let de = delta_e(r, s);
            de_sum += de;
            over += usize::from(de > VISIBLE_DE);
            histogram[((de / DE_BIN) as usize).min(DE_BINS)] += 1;
            let ry = ycbcr(r);
            let sy = ycbcr(s);
            sq_y += (ry[0] - sy[0]).powi(2);
            sq_cb += (ry[1] - f64::from(u)).powi(2);
            sq_cr += (ry[2] - f64::from(v)).powi(2);
        }
    }
    let n = sw * sh;
    let target = (n as f64 * 0.99).ceil() as u32;
    let mut seen = 0;
    let mut p99 = DE_BINS as f64 * DE_BIN;
    for (bin, &count) in histogram.iter().enumerate() {
        seen += count;
        if seen >= target {
            p99 = bin as f64 * DE_BIN;
            break;
        }
    }
    FrameScore {
        de_mean: de_sum / n as f64,
        de_over_pct: 100.0 * over as f64 / n as f64,
        de_p99: p99,
        psnr_y: psnr(sq_y, n),
        psnr_cb: psnr(sq_cb, n),
        psnr_cr: psnr(sq_cr, n),
        ssim_y: ssim_on_grid(reference, shown, scale, to_rgb),
    }
}

/// Luma SSIM on the 2× grid: 8×8 windows every 4 pixels, the standard constants.
fn ssim_on_grid(reference: &Reference, shown: &Picture, scale: usize, to_rgb: &YuvToRgb) -> f64 {
    let (gw, gh) = (reference.width * 2, reference.height * 2);
    let mut a = vec![0f64; gw * gh];
    let mut b = vec![0f64; gw * gh];
    // Luma of each native or shown pixel once, then spread over the grid.
    for y in 0..gh {
        for x in 0..gw {
            a[y * gw + x] = ycbcr(reference.pixel(x / 2, y / 2))[0];
            let (sx, sy) = if scale == 2 { (x, y) } else { (x / 2, y / 2) };
            let (yy, u, v) = shown.yuv(sx, sy);
            b[y * gw + x] = ycbcr(to_rgb.rgb(yy, u, v))[0];
        }
    }
    const C1: f64 = (0.01 * 255.0) * (0.01 * 255.0);
    const C2: f64 = (0.03 * 255.0) * (0.03 * 255.0);
    let (mut total, mut windows) = (0.0, 0usize);
    for wy in (0..=gh.saturating_sub(8)).step_by(4) {
        for wx in (0..=gw.saturating_sub(8)).step_by(4) {
            let (mut sa, mut sb, mut saa, mut sbb, mut sab) = (0.0, 0.0, 0.0, 0.0, 0.0);
            for y in wy..wy + 8 {
                for x in wx..wx + 8 {
                    let (p, q) = (a[y * gw + x], b[y * gw + x]);
                    sa += p;
                    sb += q;
                    saa += p * p;
                    sbb += q * q;
                    sab += p * q;
                }
            }
            let n = 64.0;
            let (ma, mb) = (sa / n, sb / n);
            let (va, vb) = (saa / n - ma * ma, sbb / n - mb * mb);
            let cov = sab / n - ma * mb;
            total += ((2.0 * ma * mb + C1) * (2.0 * cov + C2))
                / ((ma * ma + mb * mb + C1) * (va + vb + C2));
            windows += 1;
        }
    }
    if windows == 0 { 1.0 } else { total / windows as f64 }
}

/// Per-pixel ΔE2000 on the grid, for a region given in grid pixels (the heatmap).
pub(crate) fn delta_e_map(
    reference: &Reference,
    shown: &Picture,
    scale: usize,
    to_rgb: &YuvToRgb,
    region: (usize, usize, usize, usize),
) -> Vec<f64> {
    let (x0, y0, rw, rh) = region;
    let mut out = Vec::with_capacity(rw * rh);
    for y in y0..y0 + rh {
        for x in x0..x0 + rw {
            let (sx, sy) = if scale == 2 { (x, y) } else { (x / 2, y / 2) };
            let (yy, u, v) = shown.yuv(sx, sy);
            out.push(delta_e(reference.pixel(x / 2, y / 2), to_rgb.rgb(yy, u, v)));
        }
    }
    out
}

/// The shown picture's RGB for a region of the grid.
pub(crate) fn shown_rgb(
    shown: &Picture,
    scale: usize,
    to_rgb: &YuvToRgb,
    region: (usize, usize, usize, usize),
) -> Vec<[u8; 3]> {
    let (x0, y0, rw, rh) = region;
    let mut out = Vec::with_capacity(rw * rh);
    for y in y0..y0 + rh {
        for x in x0..x0 + rw {
            let (sx, sy) = if scale == 2 { (x, y) } else { (x / 2, y / 2) };
            let (yy, u, v) = shown.yuv(sx, sy);
            out.push(to_rgb.rgb(yy, u, v));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use sovemu_media::yuv::{I420Planes, chroma_len, rgba_to_i420};

    fn i420(rgb: &[u8], w: usize, h: usize) -> Picture {
        let rgba: Vec<u8> = rgb.chunks(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect();
        let (cw, ch) = (chroma_len(w), chroma_len(h));
        let (mut y, mut u, mut v) = (vec![0; w * h], vec![0; cw * ch], vec![0; cw * ch]);
        let planes =
            I420Planes { y: &mut y, y_stride: w, u: &mut u, u_stride: cw, v: &mut v, v_stride: cw };
        rgba_to_i420(&rgba, w, h, planes);
        Picture { width: w, height: h, y, u, v }
    }

    /// Red and blue columns alternating every pixel: the worst case for 4:2:0.
    fn stripes(w: usize, h: usize) -> Reference {
        let rgb = (0..w * h).flat_map(|i| if i % 2 == 0 { [255, 0, 0] } else { [0, 0, 255] });
        Reference { width: w, height: h, rgb: rgb.collect() }
    }

    fn doubled(r: &Reference) -> Vec<u8> {
        let mut out = Vec::new();
        for y in 0..r.height * 2 {
            for x in 0..r.width * 2 {
                out.extend_from_slice(&r.pixel(x / 2, y / 2));
            }
        }
        out
    }

    #[test]
    fn the_2x_floor_is_rounding_only_and_the_1x_floor_loses_colour() {
        let to_rgb = YuvToRgb::new();
        let r = stripes(32, 16);
        let one = score(&r, &i420(&r.rgb, 32, 16), 1, &to_rgb);
        let two = score(&r, &i420(&doubled(&r), 64, 32), 2, &to_rgb);
        assert!(two.de_mean < 1.0, "2x floor: {two:?}");
        assert!(two.de_over_pct == 0.0 && two.psnr_cb > 40.0, "2x floor: {two:?}");
        assert!(one.de_mean > 10.0 && one.de_over_pct > 90.0, "1x floor: {one:?}");
        assert!(one.psnr_cb < 20.0, "1x floor: {one:?}");
    }

    #[test]
    fn an_identical_picture_scores_perfectly() {
        let to_rgb = YuvToRgb::new();
        // Grey: no chroma, and every luma level survives the round trip exactly.
        let r = Reference { width: 8, height: 8, rgb: vec![128; 8 * 8 * 3] };
        let s = score(&r, &i420(&r.rgb, 8, 8), 1, &to_rgb);
        assert!(s.de_mean < 0.5 && s.ssim_y > 0.999, "{s:?}");
    }
}
