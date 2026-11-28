//! A frame as a core draws it (RGBA, XRGB8888, RGB565 or 0RGB1555) → I420 (planar 4:2:0) or
//! NV12 (the same, chroma interleaved), with BT.601 limited-range coefficients, which is what
//! VP8 and H.264 decoders assume when the stream carries no colour metadata.
//!
//! Chroma is the average of each 2×2 block; with an odd width or height the last column or row
//! is averaged with itself. 5- and 6-bit channels are widened as RetroHost widens them
//! (`expand5`, `expand6`), so a frame converts to the same bytes from its own format as from
//! its RGBA. The GPU's conversion (`RetroHost/src/gl/yuv.rs`) matches this one byte for byte.
//!
//! It runs on every frame a CPU core draws, straight from the core's format: each pair of rows
//! is read once, in 16-bit arithmetic (luma peaks at 56,228; chroma stays within ±28,700), which
//! the compiler vectorises. It is built twice, for the SSE2 every x86-64 has and for AVX2, which
//! is chosen at run time where the CPU has it: about 2× and 4× the scalar conversion it replaced.

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

/// How a packed frame's pixels are laid out: one of libretro's formats, or RGBA.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelLayout {
    /// Bytes R, G, B, A.
    Rgba8888,
    /// A native-endian `u32`, `0x00RRGGBB`.
    Xrgb8888,
    /// A native-endian `u16`, `RRRRRGGGGGGBBBBB`.
    Rgb565,
    /// A native-endian `u16`, `0RRRRRGGGGGBBBBB`.
    Rgb1555,
}

impl PixelLayout {
    pub fn bytes_per_pixel(self) -> usize {
        match self {
            Self::Rgba8888 | Self::Xrgb8888 => 4,
            Self::Rgb565 | Self::Rgb1555 => 2,
        }
    }
}

/// Converts a tightly packed RGBA frame (`width * height * 4` bytes) into I420 planes.
///
/// # Panics
///
/// If `rgba` or any plane is too small for the given dimensions and strides.
pub fn rgba_to_i420(rgba: &[u8], width: usize, height: usize, out: I420Planes<'_>) {
    to_i420(rgba, PixelLayout::Rgba8888, width, height, out);
}

/// Converts a tightly packed frame in `layout` into I420 planes.
///
/// # Panics
///
/// If `pixels` or any plane is too small for the given dimensions and strides.
pub fn to_i420(
    pixels: &[u8],
    layout: PixelLayout,
    width: usize,
    height: usize,
    out: I420Planes<'_>,
) {
    let I420Planes { y, y_stride, u, u_stride, v, v_stride } = out;
    let chroma = Planar { u, u_stride, v, v_stride };
    convert(pixels, layout, width, height, y, y_stride, chroma);
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
    convert(rgba, PixelLayout::Rgba8888, width, height, y, y_stride, Interleaved { uv, uv_stride });
}

/// The packed I420 size of a `width` × `height` frame: Y, then U, then V, no row padding.
pub fn i420_len(width: usize, height: usize) -> usize {
    width * height + 2 * chroma_len(width) * chroma_len(height)
}

/// A frame as an encoder is handed it: packed pixels in a layout, or packed I420 (already
/// converted, by the GPU that drew it).
#[derive(Debug, Clone, Copy)]
pub(crate) enum Source<'a> {
    Packed(&'a [u8], PixelLayout),
    I420(&'a [u8]),
}

impl Source<'_> {
    /// The bytes a `width` × `height` frame of this kind takes, and the bytes there are.
    pub(crate) fn sizes(&self, width: usize, height: usize) -> (usize, usize) {
        match self {
            Self::Packed(d, layout) => (width * height * layout.bytes_per_pixel(), d.len()),
            Self::I420(d) => (i420_len(width, height), d.len()),
        }
    }

    /// Writes the frame into I420 planes.
    pub(crate) fn write_i420(&self, width: usize, height: usize, out: I420Planes<'_>) {
        match self {
            Self::Packed(pixels, layout) => to_i420(pixels, *layout, width, height, out),
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
            Self::Packed(pixels, layout) => {
                convert(pixels, *layout, width, height, y, y_stride, Interleaved { uv, uv_stride })
            }
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

/// A pixel layout's RGB, as the conversion reads it.
trait Pixel {
    const BYTES: usize;
    fn rgb(p: &[u8]) -> [u8; 3];
}

struct Rgba;
struct Xrgb;
struct Rgb565;
struct Rgb1555;

impl Pixel for Rgba {
    const BYTES: usize = 4;
    #[inline(always)]
    fn rgb(p: &[u8]) -> [u8; 3] {
        [p[0], p[1], p[2]]
    }
}

impl Pixel for Xrgb {
    const BYTES: usize = 4;
    #[inline(always)]
    fn rgb(p: &[u8]) -> [u8; 3] {
        let v = u32::from_ne_bytes([p[0], p[1], p[2], p[3]]);
        [(v >> 16) as u8, (v >> 8) as u8, v as u8]
    }
}

impl Pixel for Rgb565 {
    const BYTES: usize = 2;
    #[inline(always)]
    fn rgb(p: &[u8]) -> [u8; 3] {
        let v = u16::from_ne_bytes([p[0], p[1]]);
        [expand5((v >> 11) & 0x1f), expand6((v >> 5) & 0x3f), expand5(v & 0x1f)]
    }
}

impl Pixel for Rgb1555 {
    const BYTES: usize = 2;
    #[inline(always)]
    fn rgb(p: &[u8]) -> [u8; 3] {
        let v = u16::from_ne_bytes([p[0], p[1]]);
        [expand5((v >> 10) & 0x1f), expand5((v >> 5) & 0x1f), expand5(v & 0x1f)]
    }
}

/// Scales a 5-bit channel to 8 bits so that 0 → 0 and 31 → 255 (as RetroHost does).
#[inline(always)]
fn expand5(v: u16) -> u8 {
    ((v << 3) | (v >> 2)) as u8
}

/// Scales a 6-bit channel to 8 bits so that 0 → 0 and 63 → 255 (as RetroHost does).
#[inline(always)]
fn expand6(v: u16) -> u8 {
    ((v << 2) | (v >> 4)) as u8
}

/// Where chroma goes: two planes, or one plane of U and V pairs.
trait Chroma {
    /// Row `cy`'s samples, written in order: `(u, v)` pairs.
    fn row(&mut self, cy: usize, cw: usize) -> impl Iterator<Item = (&mut u8, &mut u8)>;
}

struct Planar<'a> {
    u: &'a mut [u8],
    u_stride: usize,
    v: &'a mut [u8],
    v_stride: usize,
}

impl Chroma for Planar<'_> {
    #[inline(always)]
    fn row(&mut self, cy: usize, cw: usize) -> impl Iterator<Item = (&mut u8, &mut u8)> {
        let u = &mut self.u[cy * self.u_stride..cy * self.u_stride + cw];
        let v = &mut self.v[cy * self.v_stride..cy * self.v_stride + cw];
        u.iter_mut().zip(v.iter_mut())
    }
}

struct Interleaved<'a> {
    uv: &'a mut [u8],
    uv_stride: usize,
}

impl Chroma for Interleaved<'_> {
    #[inline(always)]
    fn row(&mut self, cy: usize, cw: usize) -> impl Iterator<Item = (&mut u8, &mut u8)> {
        let row = &mut self.uv[cy * self.uv_stride..cy * self.uv_stride + 2 * cw];
        row.as_chunks_mut::<2>().0.iter_mut().map(|[u, v]| (u, v))
    }
}

/// Converts with the widest vector instructions the CPU has.
fn convert(
    pixels: &[u8],
    layout: PixelLayout,
    width: usize,
    height: usize,
    y: &mut [u8],
    y_stride: usize,
    chroma: impl Chroma,
) {
    assert!(pixels.len() >= width * height * layout.bytes_per_pixel(), "pixel buffer too small");
    if width == 0 || height == 0 {
        return;
    }
    match layout {
        PixelLayout::Rgba8888 => dispatch::<Rgba>(pixels, width, height, y, y_stride, chroma),
        PixelLayout::Xrgb8888 => dispatch::<Xrgb>(pixels, width, height, y, y_stride, chroma),
        PixelLayout::Rgb565 => dispatch::<Rgb565>(pixels, width, height, y, y_stride, chroma),
        PixelLayout::Rgb1555 => dispatch::<Rgb1555>(pixels, width, height, y, y_stride, chroma),
    }
}

#[inline(always)]
fn dispatch<P: Pixel>(
    pixels: &[u8],
    width: usize,
    height: usize,
    y: &mut [u8],
    y_stride: usize,
    chroma: impl Chroma,
) {
    #[cfg(target_arch = "x86_64")]
    if std::arch::is_x86_feature_detected!("avx2") {
        // SAFETY: the CPU has AVX2, just checked (the result is cached after the first time).
        unsafe { convert_avx2::<P>(pixels, width, height, y, y_stride, chroma) };
        return;
    }
    convert_rows::<P>(pixels, width, height, y, y_stride, chroma);
}

/// [`convert_rows`] compiled for AVX2.
///
/// # Safety
///
/// The CPU must have AVX2.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn convert_avx2<P: Pixel>(
    pixels: &[u8],
    width: usize,
    height: usize,
    y: &mut [u8],
    y_stride: usize,
    chroma: impl Chroma,
) {
    convert_rows::<P>(pixels, width, height, y, y_stride, chroma);
}

#[inline(always)]
fn luma(r: u16, g: u16, b: u16) -> u8 {
    (((66 * r + 129 * g + 25 * b + 128) >> 8) + 16) as u8
}

/// U and V of a block's average. Never outside 16-240, so it needs no clamp.
#[inline(always)]
fn chroma(r: i16, g: i16, b: i16) -> (u8, u8) {
    let u = ((-38 * r - 74 * g + 112 * b + 128) >> 8) + 128;
    let v = ((112 * r - 94 * g - 18 * b + 128) >> 8) + 128;
    (u as u8, v as u8)
}

/// The conversion itself: luma a row at a time, chroma a pair of rows at a time, each read once.
#[inline(always)]
fn convert_rows<P: Pixel>(
    pixels: &[u8],
    width: usize,
    height: usize,
    y_plane: &mut [u8],
    y_stride: usize,
    mut chroma_out: impl Chroma,
) {
    let stride = width * P::BYTES;
    let row = |r: usize| &pixels[r * stride..(r + 1) * stride];
    for r in 0..height {
        let out = &mut y_plane[r * y_stride..r * y_stride + width];
        for (dst, p) in out.iter_mut().zip(row(r).chunks_exact(P::BYTES)) {
            let [r, g, b] = P::rgb(p);
            *dst = luma(r.into(), g.into(), b.into());
        }
    }
    let cw = chroma_len(width);
    for cy in 0..chroma_len(height) {
        let (top, bottom) = (row(cy * 2), row((cy * 2 + 1).min(height - 1)));
        let mut out = chroma_out.row(cy, cw);
        for ((a, b), (u, v)) in
            top.chunks_exact(2 * P::BYTES).zip(bottom.chunks_exact(2 * P::BYTES)).zip(&mut out)
        {
            let px = [
                P::rgb(&a[..P::BYTES]),
                P::rgb(&a[P::BYTES..]),
                P::rgb(&b[..P::BYTES]),
                P::rgb(&b[P::BYTES..]),
            ];
            let mean = |c: usize| (px.iter().map(|p| i16::from(p[c])).sum::<i16>() + 2) >> 2;
            (*u, *v) = chroma(mean(0), mean(1), mean(2));
        }
        // An odd width: the last column is its own pair.
        if width % 2 == 1
            && let Some((u, v)) = out.next()
        {
            let x = (width - 1) * P::BYTES;
            let (a, b) = (P::rgb(&top[x..x + P::BYTES]), P::rgb(&bottom[x..x + P::BYTES]));
            let mean = |c: usize| (2 * i16::from(a[c]) + 2 * i16::from(b[c]) + 2) >> 2;
            (*u, *v) = chroma(mean(0), mean(1), mean(2));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The scalar conversion this one replaced, kept as the reference it must match.
    fn reference(rgba: &[u8], w: usize, h: usize) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
        let px = |x: usize, y: usize| {
            let i = (y * w + x) * 4;
            (i32::from(rgba[i]), i32::from(rgba[i + 1]), i32::from(rgba[i + 2]))
        };
        let (cw, ch) = (chroma_len(w), chroma_len(h));
        let (mut luma, mut u, mut v) = (vec![0; w * h], vec![0; cw * ch], vec![0; cw * ch]);
        for y in 0..h {
            for x in 0..w {
                let (r, g, b) = px(x, y);
                luma[y * w + x] = (((66 * r + 129 * g + 25 * b + 128) >> 8) + 16) as u8;
            }
        }
        for cy in 0..ch {
            for cx in 0..cw {
                let (x0, y0) = (cx * 2, cy * 2);
                let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
                let (mut r, mut g, mut b) = (0, 0, 0);
                for (x, y) in [(x0, y0), (x1, y0), (x0, y1), (x1, y1)] {
                    let (pr, pg, pb) = px(x, y);
                    (r, g, b) = (r + pr, g + pg, b + pb);
                }
                let (r, g, b) = ((r + 2) / 4, (g + 2) / 4, (b + 2) / 4);
                let cu = ((-38 * r - 74 * g + 112 * b + 128) >> 8) + 128;
                let cv = ((112 * r - 94 * g - 18 * b + 128) >> 8) + 128;
                u[cy * cw + cx] = cu.clamp(0, 255) as u8;
                v[cy * cw + cx] = cv.clamp(0, 255) as u8;
            }
        }
        (luma, u, v)
    }

    /// Bytes that look like nothing in particular.
    fn noise(len: usize, seed: usize) -> Vec<u8> {
        (0..len).map(|i| ((i + seed).wrapping_mul(2_654_435_761) >> 13) as u8).collect()
    }

    /// A frame in `layout` as RGBA, widened as RetroHost widens it.
    fn as_rgba(pixels: &[u8], layout: PixelLayout) -> Vec<u8> {
        let rgb = |p: &[u8]| match layout {
            PixelLayout::Rgba8888 => Rgba::rgb(p),
            PixelLayout::Xrgb8888 => Xrgb::rgb(p),
            PixelLayout::Rgb565 => Rgb565::rgb(p),
            PixelLayout::Rgb1555 => Rgb1555::rgb(p),
        };
        pixels
            .chunks_exact(layout.bytes_per_pixel())
            .flat_map(|p| {
                let [r, g, b] = rgb(p);
                [r, g, b, 255]
            })
            .collect()
    }

    /// The frame converted by the path in use (AVX2 where the CPU has it) and by the baseline
    /// one, into I420 and into NV12: all four must agree.
    fn every_path(
        pixels: &[u8],
        layout: PixelLayout,
        w: usize,
        h: usize,
    ) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
        let (cw, ch) = (chroma_len(w), chroma_len(h));
        let (mut y, mut u, mut v) = (vec![0; w * h], vec![0; cw * ch], vec![0; cw * ch]);
        let planes =
            I420Planes { y: &mut y, y_stride: w, u: &mut u, u_stride: cw, v: &mut v, v_stride: cw };
        to_i420(pixels, layout, w, h, planes);

        let (mut by, mut bu, mut bv) = (vec![0; w * h], vec![0; cw * ch], vec![0; cw * ch]);
        let chroma = Planar { u: &mut bu, u_stride: cw, v: &mut bv, v_stride: cw };
        match layout {
            PixelLayout::Rgba8888 => convert_rows::<Rgba>(pixels, w, h, &mut by, w, chroma),
            PixelLayout::Xrgb8888 => convert_rows::<Xrgb>(pixels, w, h, &mut by, w, chroma),
            PixelLayout::Rgb565 => convert_rows::<Rgb565>(pixels, w, h, &mut by, w, chroma),
            PixelLayout::Rgb1555 => convert_rows::<Rgb1555>(pixels, w, h, &mut by, w, chroma),
        }
        assert_eq!(
            (&by, &bu, &bv),
            (&y, &u, &v),
            "{layout:?} {w}x{h}: baseline and wide paths differ"
        );

        let (mut ny, mut nuv) = (vec![0; w * h], vec![0; 2 * cw * ch]);
        Source::Packed(pixels, layout).write_nv12(w, h, &mut ny, w, &mut nuv, 2 * cw);
        let interleaved: Vec<u8> = u.iter().zip(&v).flat_map(|(&u, &v)| [u, v]).collect();
        assert_eq!(
            (&ny, &nuv),
            (&y, &interleaved),
            "{layout:?} {w}x{h}: NV12 is not I420 interleaved"
        );
        (y, u, v)
    }

    #[test]
    fn every_layout_converts_as_its_rgba_did_with_the_scalar_conversion() {
        let layouts = [
            PixelLayout::Rgba8888,
            PixelLayout::Xrgb8888,
            PixelLayout::Rgb565,
            PixelLayout::Rgb1555,
        ];
        for (w, h) in
            [(1, 1), (2, 2), (3, 1), (1, 3), (5, 3), (17, 9), (255, 223), (256, 224), (640, 480)]
        {
            for layout in layouts {
                let pixels = noise(w * h * layout.bytes_per_pixel(), w * 31 + h);
                let got = every_path(&pixels, layout, w, h);
                assert_eq!(got, reference(&as_rgba(&pixels, layout), w, h), "{layout:?} {w}x{h}");
            }
        }
    }

    #[test]
    fn every_16_bit_pixel_converts_as_its_rgba_did() {
        // All 65,536 values, each in a 2x2 block of its own (chroma then sees one colour) and
        // in a frame of them side by side (chroma mixes neighbours).
        for layout in [PixelLayout::Rgb565, PixelLayout::Rgb1555] {
            let all: Vec<u8> = (0..=u16::MAX).flat_map(u16::to_ne_bytes).collect();
            let got = every_path(&all, layout, 256, 256);
            assert_eq!(got, reference(&as_rgba(&all, layout), 256, 256), "{layout:?}");
            let blocks: Vec<u8> =
                (0..=u16::MAX).flat_map(|v| [v, v].map(u16::to_ne_bytes).concat()).collect();
            let rows = [blocks.clone(), blocks].concat();
            assert_eq!(
                every_path(&rows, layout, 2 * 65_536, 2),
                reference(&as_rgba(&rows, layout), 2 * 65_536, 2)
            );
        }
    }

    #[test]
    fn chroma_never_needs_its_clamp() {
        for r in 0..=255i16 {
            for g in 0..=255i16 {
                for b in 0..=255i16 {
                    let (u, v) = chroma(r, g, b);
                    let cu = ((-38 * i32::from(r) - 74 * i32::from(g) + 112 * i32::from(b) + 128)
                        >> 8)
                        + 128;
                    let cv = ((112 * i32::from(r) - 94 * i32::from(g) - 18 * i32::from(b) + 128)
                        >> 8)
                        + 128;
                    assert_eq!((i32::from(u), i32::from(v)), (cu, cv), "rgb {r} {g} {b}");
                    assert!((16..=240).contains(&cu) && (16..=240).contains(&cv));
                }
            }
        }
    }

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
        Source::Packed(&rgba, PixelLayout::Rgba8888).write_nv12(w, h, &mut ry, w, &mut ruv, cw * 2);
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
