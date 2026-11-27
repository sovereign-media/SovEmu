//! Colour: the inverse of `yuv.rs`, sRGB to CIELAB and CIEDE2000.

/// `yuv.rs`'s forward matrix, in 256ths: rows Y, U, V; columns R, G, B.
const FORWARD: [[f64; 3]; 3] = [[66.0, 129.0, 25.0], [-38.0, -74.0, 112.0], [112.0, -94.0, -18.0]];

/// The exact inverse of `yuv.rs`'s BT.601 limited-range matrix, so a picture that went through
/// I420 and back with no codec loses only chroma subsampling and rounding.
#[derive(Debug, Clone, Copy)]
pub(crate) struct YuvToRgb {
    m: [[f64; 3]; 3],
}

impl YuvToRgb {
    pub(crate) fn new() -> Self {
        let f = FORWARD.map(|row| row.map(|c| c / 256.0));
        let det = f[0][0] * (f[1][1] * f[2][2] - f[1][2] * f[2][1])
            - f[0][1] * (f[1][0] * f[2][2] - f[1][2] * f[2][0])
            + f[0][2] * (f[1][0] * f[2][1] - f[1][1] * f[2][0]);
        let mut m = [[0.0; 3]; 3];
        for (r, row) in m.iter_mut().enumerate() {
            for (c, cell) in row.iter_mut().enumerate() {
                // Inverse = adjugate / determinant: cofactor of the transposed position.
                let (r1, r2) = ((c + 1) % 3, (c + 2) % 3);
                let (c1, c2) = ((r + 1) % 3, (r + 2) % 3);
                *cell = (f[r1][c1] * f[r2][c2] - f[r1][c2] * f[r2][c1]) / det;
            }
        }
        Self { m }
    }

    /// One pixel, rounded and clamped to 8 bits as a display would show it.
    pub(crate) fn rgb(&self, y: u8, u: u8, v: u8) -> [u8; 3] {
        let (y, u, v) = (f64::from(y) - 16.0, f64::from(u) - 128.0, f64::from(v) - 128.0);
        self.m.map(|row| (row[0] * y + row[1] * u + row[2] * v).round().clamp(0.0, 255.0) as u8)
    }
}

/// BT.601 limited-range luma and chroma of an RGB pixel, unrounded (`yuv.rs`'s matrix).
pub(crate) fn ycbcr(rgb: [u8; 3]) -> [f64; 3] {
    let [r, g, b] = rgb.map(f64::from);
    let offset = [16.0, 128.0, 128.0];
    let mut out = [0.0; 3];
    for (i, row) in FORWARD.iter().enumerate() {
        out[i] = (row[0] * r + row[1] * g + row[2] * b) / 256.0 + offset[i];
    }
    out
}

fn linear(c: u8) -> f64 {
    let c = f64::from(c) / 255.0;
    if c <= 0.040_45 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

/// sRGB (D65) → CIELAB.
pub(crate) fn lab(rgb: [u8; 3]) -> [f64; 3] {
    let [r, g, b] = rgb.map(linear);
    let x = (0.412_456_4 * r + 0.357_576_1 * g + 0.180_437_5 * b) / 0.950_47;
    let y = 0.212_672_9 * r + 0.715_152_2 * g + 0.072_175_0 * b;
    let z = (0.019_333_9 * r + 0.119_192_0 * g + 0.950_304_1 * b) / 1.088_83;
    let f = |t: f64| {
        const D: f64 = 6.0 / 29.0;
        if t > D * D * D { t.cbrt() } else { t / (3.0 * D * D) + 4.0 / 29.0 }
    };
    let (fx, fy, fz) = (f(x), f(y), f(z));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

/// CIEDE2000 colour difference (Sharma, Wu and Dalal's formulation), kL = kC = kH = 1.
pub(crate) fn ciede2000(lab1: [f64; 3], lab2: [f64; 3]) -> f64 {
    let [l1, a1, b1] = lab1;
    let [l2, a2, b2] = lab2;
    let pow25_7 = 25f64.powi(7);
    let c_bar = (a1.hypot(b1) + a2.hypot(b2)) / 2.0;
    let g = 0.5 * (1.0 - (c_bar.powi(7) / (c_bar.powi(7) + pow25_7)).sqrt());
    let (a1p, a2p) = ((1.0 + g) * a1, (1.0 + g) * a2);
    let (c1p, c2p) = (a1p.hypot(b1), a2p.hypot(b2));
    let hue = |b: f64, a: f64| {
        if b == 0.0 && a == 0.0 { 0.0 } else { b.atan2(a).to_degrees().rem_euclid(360.0) }
    };
    let (h1p, h2p) = (hue(b1, a1p), hue(b2, a2p));

    let dl = l2 - l1;
    let dc = c2p - c1p;
    let dh = if c1p * c2p == 0.0 {
        0.0
    } else {
        let d = h2p - h1p;
        if d.abs() <= 180.0 {
            d
        } else if d > 180.0 {
            d - 360.0
        } else {
            d + 360.0
        }
    };
    let d_h = 2.0 * (c1p * c2p).sqrt() * (dh / 2.0).to_radians().sin();

    let l_bar = (l1 + l2) / 2.0;
    let c_bar_p = (c1p + c2p) / 2.0;
    let h_bar = if c1p * c2p == 0.0 {
        h1p + h2p
    } else if (h1p - h2p).abs() <= 180.0 {
        (h1p + h2p) / 2.0
    } else if h1p + h2p < 360.0 {
        (h1p + h2p + 360.0) / 2.0
    } else {
        (h1p + h2p - 360.0) / 2.0
    };
    let cos = |deg: f64| deg.to_radians().cos();
    let t =
        1.0 - 0.17 * cos(h_bar - 30.0) + 0.24 * cos(2.0 * h_bar) + 0.32 * cos(3.0 * h_bar + 6.0)
            - 0.20 * cos(4.0 * h_bar - 63.0);
    let d_theta = 30.0 * (-((h_bar - 275.0) / 25.0).powi(2)).exp();
    let r_c = 2.0 * (c_bar_p.powi(7) / (c_bar_p.powi(7) + pow25_7)).sqrt();
    let l50 = (l_bar - 50.0).powi(2);
    let s_l = 1.0 + 0.015 * l50 / (20.0 + l50).sqrt();
    let s_c = 1.0 + 0.045 * c_bar_p;
    let s_h = 1.0 + 0.015 * c_bar_p * t;
    let r_t = -(2.0 * d_theta).to_radians().sin() * r_c;
    let (tl, tc, th) = (dl / s_l, dc / s_c, d_h / s_h);
    (tl * tl + tc * tc + th * th + r_t * tc * th).max(0.0).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ciede2000_matches_sharmas_test_data() {
        // Pairs 1, 7, 17, 25 and 34 of Sharma, Wu and Dalal (2005), table 1.
        let pairs = [
            ([50.0, 2.6772, -79.7751], [50.0, 0.0, -82.7485], 2.0425),
            ([50.0, 0.0, 0.0], [50.0, -1.0, 2.0], 2.3669),
            ([50.0, 2.5, 0.0], [73.0, 25.0, -18.0], 27.1492),
            ([60.2574, -34.0099, 36.2677], [60.4626, -34.1751, 39.4387], 1.2644),
            ([90.8027, -2.0831, 1.4410], [91.1528, -1.6435, 0.0447], 1.4441),
        ];
        for (a, b, expected) in pairs {
            let got = ciede2000(a, b);
            assert!((got - expected).abs() < 1e-4, "{a:?} {b:?}: {got} != {expected}");
            assert!((ciede2000(b, a) - expected).abs() < 1e-4, "symmetric");
        }
    }

    #[test]
    fn lab_of_white_and_black() {
        let w = lab([255, 255, 255]);
        assert!((w[0] - 100.0).abs() < 1e-3 && w[1].abs() < 1e-2 && w[2].abs() < 1e-2, "{w:?}");
        assert_eq!(lab([0, 0, 0]), [0.0, 0.0, 0.0]);
    }

    #[test]
    fn the_inverse_undoes_yuv_rs() {
        // Through the forward matrix (rounded as yuv.rs rounds) and back: within one step.
        let inv = YuvToRgb::new();
        for rgb in
            [[0, 0, 0], [255, 255, 255], [255, 0, 0], [0, 255, 0], [0, 0, 255], [12, 200, 99]]
        {
            let [y, u, v] = ycbcr(rgb).map(|c| c.round() as u8);
            let back = inv.rgb(y, u, v);
            for c in 0..3 {
                assert!(back[c].abs_diff(rgb[c]) <= 2, "{rgb:?} -> {back:?}");
            }
        }
    }
}
