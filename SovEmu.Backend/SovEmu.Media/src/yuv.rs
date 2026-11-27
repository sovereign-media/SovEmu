//! RGBA → I420 (planar 4:2:0) or NV12 (the same, chroma interleaved) with BT.601 limited-range
//! coefficients, which is what VP8 and H.264 decoders assume when the stream carries no colour
//! metadata.
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
    let I420Planes { y, y_stride, u, u_stride, v, v_stride } = out;
    convert(rgba, width, height, y, y_stride, |cx, cy, (cu, cv)| {
        u[cy * u_stride + cx] = cu;
        v[cy * v_stride + cx] = cv;
    });
}

/// Converts a tightly packed RGBA frame into NV12: the I420 luma plane, then one plane of
/// interleaved U and V (what hardware encoders take).
///
/// # Panics
///
/// If `rgba` or either plane is too small for the given dimensions and strides.
pub fn rgba_to_nv12(
    rgba: &[u8],
    width: usize,
    height: usize,
    y: &mut [u8],
    y_stride: usize,
    uv: &mut [u8],
    uv_stride: usize,
) {
    convert(rgba, width, height, y, y_stride, |cx, cy, (cu, cv)| {
        let i = cy * uv_stride + cx * 2;
        uv[i] = cu;
        uv[i + 1] = cv;
    });
}

/// The packed I420 size of a `width` × `height` frame: Y, then U, then V, no row padding.
pub fn i420_len(width: usize, height: usize) -> usize {
    width * height + 2 * chroma_len(width) * chroma_len(height)
}

/// A frame as an encoder is handed it: tightly packed RGBA, or packed I420 (already converted,
/// by the GPU that drew it).
#[derive(Debug, Clone, Copy)]
pub(crate) enum Source<'a> {
    Rgba(&'a [u8]),
    I420(&'a [u8]),
}

impl Source<'_> {
    /// The bytes a `width` × `height` frame of this kind takes, and the bytes there are.
    pub(crate) fn sizes(&self, width: usize, height: usize) -> (usize, usize) {
        match self {
            Self::Rgba(d) => (width * height * 4, d.len()),
            Self::I420(d) => (i420_len(width, height), d.len()),
        }
    }

    /// Writes the frame into I420 planes.
    pub(crate) fn write_i420(&self, width: usize, height: usize, out: I420Planes<'_>) {
        match self {
            Self::Rgba(rgba) => rgba_to_i420(rgba, width, height, out),
            Self::I420(src) => {
                let (y, u, v) = split_i420(src, width, height);
                copy_plane(y, width, out.y, out.y_stride);
                copy_plane(u, chroma_len(width), out.u, out.u_stride);
                copy_plane(v, chroma_len(width), out.v, out.v_stride);
            }
        }
    }

    /// Writes the frame into NV12 planes.
    pub(crate) fn write_nv12(
        &self,
        width: usize,
        height: usize,
        y: &mut [u8],
        y_stride: usize,
        uv: &mut [u8],
        uv_stride: usize,
    ) {
        match self {
            Self::Rgba(rgba) => rgba_to_nv12(rgba, width, height, y, y_stride, uv, uv_stride),
            Self::I420(src) => {
                let (sy, su, sv) = split_i420(src, width, height);
                copy_plane(sy, width, y, y_stride);
                let cw = chroma_len(width);
                for (row, (u, v)) in
                    uv.chunks_mut(uv_stride).zip(su.chunks_exact(cw).zip(sv.chunks_exact(cw)))
                {
                    for (pair, (&u, &v)) in
                        row.as_chunks_mut::<2>().0.iter_mut().zip(u.iter().zip(v))
                    {
                        *pair = [u, v];
                    }
                }
            }
        }
    }
}

/// Packed I420's three planes.
fn split_i420(src: &[u8], width: usize, height: usize) -> (&[u8], &[u8], &[u8]) {
    let chroma = chroma_len(width) * chroma_len(height);
    let (y, rest) = src.split_at(width * height);
    let (u, v) = rest.split_at(chroma);
    (y, u, &v[..chroma])
}

/// Copies rows of `width` bytes from a packed plane into one with `stride`-byte rows.
fn copy_plane(src: &[u8], width: usize, dst: &mut [u8], stride: usize) {
    for (s, d) in src.chunks_exact(width).zip(dst.chunks_mut(stride)) {
        d[..width].copy_from_slice(s);
    }
}

/// `rgba` as packed I420, for tests that compare the two kinds of source.
#[cfg(test)]
pub(crate) fn packed_i420(rgba: &[u8], width: usize, height: usize) -> Vec<u8> {
    let (cw, ch) = (chroma_len(width), chroma_len(height));
    let (mut y, mut u, mut v) = (vec![0; width * height], vec![0; cw * ch], vec![0; cw * ch]);
    let planes =
        I420Planes { y: &mut y, y_stride: width, u: &mut u, u_stride: cw, v: &mut v, v_stride: cw };
    rgba_to_i420(rgba, width, height, planes);
    [y, u, v].concat()
}

/// Writes luma into `y` and hands each chroma sample (column, row, (U, V)) to `chroma_out`.
fn convert(
    rgba: &[u8],
    width: usize,
    height: usize,
    y_plane: &mut [u8],
    y_stride: usize,
    mut chroma_out: impl FnMut(usize, usize, (u8, u8)),
) {
    assert!(rgba.len() >= width * height * 4, "rgba buffer too small");
    let px = |x: usize, y: usize| {
        let i = (y * width + x) * 4;
        (rgba[i] as i32, rgba[i + 1] as i32, rgba[i + 2] as i32)
    };

    for y in 0..height {
        let row = &mut y_plane[y * y_stride..y * y_stride + width];
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
            chroma_out(cx, cy, chroma((r + 2) / 4, (g + 2) / 4, (b + 2) / 4));
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

    #[test]
    fn an_i420_source_is_copied_into_either_layout() {
        let (w, h) = (5, 3);
        let rgba: Vec<u8> =
            (0..w * h).flat_map(|i| [(i * 17) as u8, (i * 5) as u8, 200, 255]).collect();
        let (iy, iu, iv) = convert(&rgba, w, h);
        let packed = [iy.clone(), iu.clone(), iv.clone()].concat();
        assert_eq!(packed.len(), i420_len(w, h));
        let source = Source::I420(&packed);
        assert_eq!(source.sizes(w, h), (i420_len(w, h), packed.len()));

        // Into planes with padded rows, as an encoder's image has them.
        let (cw, ch) = (chroma_len(w), chroma_len(h));
        let (mut y, mut u, mut v) = (vec![0; 8 * h], vec![0; 4 * ch], vec![0; 4 * ch]);
        let planes =
            I420Planes { y: &mut y, y_stride: 8, u: &mut u, u_stride: 4, v: &mut v, v_stride: 4 };
        source.write_i420(w, h, planes);
        let unpad = |p: &[u8], width, stride| -> Vec<u8> {
            p.chunks(stride).flat_map(|r| r[..width].to_vec()).collect()
        };
        assert_eq!(unpad(&y, w, 8), iy);
        assert_eq!(unpad(&u, cw, 4), iu);
        assert_eq!(unpad(&v, cw, 4), iv);

        let (mut ny, mut nuv) = (vec![0; w * h], vec![0; cw * 2 * ch]);
        source.write_nv12(w, h, &mut ny, w, &mut nuv, cw * 2);
        let (mut ry, mut ruv) = (vec![0; w * h], vec![0; cw * 2 * ch]);
        Source::Rgba(&rgba).write_nv12(w, h, &mut ry, w, &mut ruv, cw * 2);
        assert_eq!((ny, nuv), (ry, ruv), "the same NV12 from RGBA and from its I420");
    }

    #[test]
    fn nv12_is_i420_with_chroma_interleaved() {
        let (w, h) = (5, 3);
        let rgba: Vec<u8> =
            (0..w * h).flat_map(|i| [(i * 17) as u8, (i * 5) as u8, 200, 255]).collect();
        let (iy, iu, iv) = convert(&rgba, w, h);
        let cw = chroma_len(w);
        let (mut y, mut uv) = (vec![0; w * h], vec![0; cw * 2 * chroma_len(h)]);
        rgba_to_nv12(&rgba, w, h, &mut y, w, &mut uv, cw * 2);
        assert_eq!(y, iy);
        let interleaved: Vec<u8> = iu.iter().zip(&iv).flat_map(|(&u, &v)| [u, v]).collect();
        assert_eq!(uv, interleaved);
    }
}
