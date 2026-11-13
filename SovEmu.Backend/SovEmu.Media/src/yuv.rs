//! RGBA → I420 (planar 4:2:0) with BT.601 limited-range coefficients, which is what VP8 and
//! H.264 decoders assume when the stream carries no colour metadata.
//!
//! Chroma is the average of each 2×2 block; with an odd width or height the last column or row
//! is averaged with itself. At console resolutions (256×224) this costs well under a
//! millisecond, so it stays plain scalar code.

/// The three destination planes and their row strides, in bytes.
#[derive(Debug)]
pub struct I420Planes<'a> {
    pub y: &'a mut [u8],
    pub y_stride: usize,
    pub u: &'a mut [u8],
    pub u_stride: usize,
    pub v: &'a mut [u8],
    pub v_stride: usize,
}

/// Size of a chroma plane dimension for a luma dimension.
pub fn chroma_len(luma: usize) -> usize {
    luma.div_ceil(2)
}

fn luma(r: i32, g: i32, b: i32) -> u8 {
    (((66 * r + 129 * g + 25 * b + 128) >> 8) + 16) as u8
}

fn chroma(r: i32, g: i32, b: i32) -> (u8, u8) {
    let u = ((-38 * r - 74 * g + 112 * b + 128) >> 8) + 128;
    let v = ((112 * r - 94 * g - 18 * b + 128) >> 8) + 128;
    (u.clamp(0, 255) as u8, v.clamp(0, 255) as u8)
}

/// Converts a tightly packed RGBA frame (`width * height * 4` bytes) into I420 planes.
///
/// # Panics
///
/// If `rgba` or any plane is too small for the given dimensions and strides.
pub fn rgba_to_i420(rgba: &[u8], width: usize, height: usize, out: I420Planes<'_>) {
    assert!(rgba.len() >= width * height * 4, "rgba buffer too small");
    let px = |x: usize, y: usize| {
        let i = (y * width + x) * 4;
        (rgba[i] as i32, rgba[i + 1] as i32, rgba[i + 2] as i32)
    };

    for y in 0..height {
        let row = &mut out.y[y * out.y_stride..y * out.y_stride + width];
        for (x, dst) in row.iter_mut().enumerate() {
            let (r, g, b) = px(x, y);
            *dst = luma(r, g, b);
        }
    }

    for cy in 0..chroma_len(height) {
        for cx in 0..chroma_len(width) {
            let (x0, y0) = (cx * 2, cy * 2);
            let (x1, y1) = ((x0 + 1).min(width - 1), (y0 + 1).min(height - 1));
            let (mut r, mut g, mut b) = (0, 0, 0);
            for (x, y) in [(x0, y0), (x1, y0), (x0, y1), (x1, y1)] {
                let (pr, pg, pb) = px(x, y);
                r += pr;
                g += pg;
                b += pb;
            }
            let (u, v) = chroma((r + 2) / 4, (g + 2) / 4, (b + 2) / 4);
            out.u[cy * out.u_stride + cx] = u;
            out.v[cy * out.v_stride + cx] = v;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn convert(rgba: &[u8], w: usize, h: usize) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
        let (cw, ch) = (chroma_len(w), chroma_len(h));
        let (mut y, mut u, mut v) = (vec![0; w * h], vec![0; cw * ch], vec![0; cw * ch]);
        rgba_to_i420(
            rgba,
            w,
            h,
            I420Planes { y: &mut y, y_stride: w, u: &mut u, u_stride: cw, v: &mut v, v_stride: cw },
        );
        (y, u, v)
    }

    #[test]
    fn primaries_match_bt601_limited_range() {
        let cases = [
            ([0, 0, 0], (16, 128, 128)),
            ([255, 255, 255], (235, 128, 128)),
            ([255, 0, 0], (82, 90, 240)),
            ([0, 255, 0], (144, 54, 34)),
            ([0, 0, 255], (41, 240, 110)),
        ];
        for (rgb, (ey, eu, ev)) in cases {
            let rgba: Vec<u8> = [rgb[0], rgb[1], rgb[2], 255].repeat(4);
            let (y, u, v) = convert(&rgba, 2, 2);
            assert_eq!((y[0], u[0], v[0]), (ey, eu, ev), "rgb {rgb:?}");
        }
    }

    #[test]
    fn odd_dimensions_cover_every_pixel() {
        let rgba = [255u8, 255, 255, 255].repeat(3 * 3);
        let (y, u, v) = convert(&rgba, 3, 3);
        assert_eq!(y.len(), 9);
        assert_eq!(u.len(), 4);
        assert!(y.iter().all(|&l| l == 235));
        assert!(u.iter().chain(&v).all(|&c| c == 128));
    }

    #[test]
    fn chroma_averages_each_block() {
        // Left column black, right column white: every 2x2 block is half and half.
        let mut rgba = Vec::new();
        for _ in 0..2 {
            rgba.extend_from_slice(&[0, 0, 0, 255, 255, 255, 255, 255]);
        }
        let (y, u, _) = convert(&rgba, 2, 2);
        assert_eq!(y, [16, 235, 16, 235]);
        assert_eq!(u, [128]);
    }
}
