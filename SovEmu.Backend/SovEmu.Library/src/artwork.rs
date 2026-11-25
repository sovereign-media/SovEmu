//! Artwork made smaller: lossy WebP copies of box art, screenshots and photos, at the size a
//! client shows them. Nothing here is hand-rolled: `image` decodes (JPEG, PNG, WebP),
//! `fast_image_resize` scales, and libwebp (the `webp` crate) encodes.
//!
//! Every client decodes WebP: browsers, Android (since API 14; the app needs 28) and iOS (since
//! 14; the app needs 18). Copies are only ever scaled down, never up, and are written whole (a
//! temp file, then a rename), so a reader never sees half of one.
//!
//! Widths sit on a grid ([`grid_width`]: 16 px steps, at least [`MIN_WIDTH`]), so a few copies
//! of each picture serve every screen, and only widths on it are made ([`on_grid`]).

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use fast_image_resize::images::Image;
use fast_image_resize::{PixelType, ResizeAlg, ResizeOptions, Resizer};

/// The narrowest copy made.
pub const MIN_WIDTH: u32 = 144;
/// Copies' widths are multiples of this.
pub const STEP: u32 = 16;
/// The widest copy made: anything wider is served at this width.
pub const MAX_WIDTH: u32 = 4096;

/// A requested width on the grid: rounded up to a multiple of [`STEP`], at least
/// [`MIN_WIDTH`], at most [`MAX_WIDTH`].
pub fn grid_width(requested: u32) -> u32 {
    requested.clamp(MIN_WIDTH, MAX_WIDTH).div_ceil(STEP) * STEP
}

/// Whether `width` is one the grid makes (a URL with any other is refused).
pub fn on_grid(width: u32) -> bool {
    grid_width(width) == width
}

#[derive(Debug)]
pub enum ArtError {
    /// Not a picture `image` reads.
    Decode(String),
    Encode(String),
    Io(std::io::Error),
}

impl std::fmt::Display for ArtError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Decode(e) => write!(f, "cannot read the picture: {e}"),
            Self::Encode(e) => write!(f, "cannot encode WebP: {e}"),
            Self::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ArtError {}

impl From<std::io::Error> for ArtError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// The size `(width, height)` scaled down, keeping its shape, to fit within `max_width` and
/// `max_height` (either may be absent); never scaled up.
pub(crate) fn fitted(
    width: u32,
    height: u32,
    max_width: Option<u32>,
    max_height: Option<u32>,
) -> (u32, u32) {
    let scale_w = max_width.map_or(1.0, |m| f64::from(m) / f64::from(width.max(1)));
    let scale_h = max_height.map_or(1.0, |m| f64::from(m) / f64::from(height.max(1)));
    let scale = scale_w.min(scale_h).min(1.0);
    if scale >= 1.0 {
        return (width, height);
    }
    let w = (f64::from(width) * scale).round().max(1.0) as u32;
    let h = (f64::from(height) * scale).round().max(1.0) as u32;
    (w, h)
}

/// `bytes` (a JPEG, PNG or WebP) as lossy WebP at `quality` (0-100), scaled down to fit within
/// `max_width` and `max_height`. Transparency is kept where the picture has any.
pub fn to_webp(
    bytes: &[u8],
    max_width: Option<u32>,
    max_height: Option<u32>,
    quality: f32,
) -> Result<Vec<u8>, ArtError> {
    let picture = image::load_from_memory(bytes).map_err(|e| ArtError::Decode(e.to_string()))?;
    let rgba = picture.into_rgba8();
    let (width, height) = rgba.dimensions();
    let (w, h) = fitted(width, height, max_width, max_height);
    let pixels = if (w, h) == (width, height) {
        rgba.into_raw()
    } else {
        let src = Image::from_vec_u8(width, height, rgba.into_raw(), PixelType::U8x4)
            .map_err(|e| ArtError::Encode(e.to_string()))?;
        let mut dst = Image::new(w, h, PixelType::U8x4);
        // Lanczos, with alpha premultiplied while scaling (the default), so edges stay clean.
        let options = ResizeOptions::new()
            .resize_alg(ResizeAlg::Convolution(fast_image_resize::FilterType::Lanczos3));
        Resizer::new()
            .resize(&src, &mut dst, &options)
            .map_err(|e| ArtError::Encode(e.to_string()))?;
        dst.into_vec()
    };
    let opaque = pixels.as_chunks::<4>().0.iter().all(|p| p[3] == 255);
    let encoded = if opaque {
        // No transparency: encode without the alpha plane, which is smaller.
        let rgb: Vec<u8> =
            pixels.as_chunks::<4>().0.iter().flat_map(|p| [p[0], p[1], p[2]]).collect();
        webp::Encoder::from_rgb(&rgb, w, h).encode(quality.clamp(0.0, 100.0)).to_vec()
    } else {
        webp::Encoder::from_rgba(&pixels, w, h).encode(quality.clamp(0.0, 100.0)).to_vec()
    };
    if encoded.is_empty() {
        return Err(ArtError::Encode("libwebp wrote nothing".into()));
    }
    Ok(encoded)
}

/// Writes `bytes` to `path` whole: a temp file beside it, then a rename.
pub fn write_whole(path: &Path, bytes: &[u8]) -> Result<(), ArtError> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let temp = path.with_extension(format!("part-{}-{n}", std::process::id()));
    std::fs::write(&temp, bytes)?;
    std::fs::rename(&temp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&temp);
    })?;
    Ok(())
}

/// Makes `dst`, a WebP copy of the picture at `src` at most `width` wide, unless `dst` is already
/// there and newer than `src` (a picture replaced since is copied again).
pub fn webp_copy(src: &Path, dst: &Path, width: u32, quality: f32) -> Result<(), ArtError> {
    let modified = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    if let (Some(copy), Some(original)) = (modified(dst), modified(src))
        && copy >= original
    {
        return Ok(());
    }
    let bytes = std::fs::read(src)?;
    write_whole(dst, &to_webp(&bytes, Some(width), None, quality)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32, alpha: u8) -> Vec<u8> {
        let img = image::RgbaImage::from_fn(width, height, |x, y| {
            image::Rgba([(x % 256) as u8, (y % 256) as u8, 128, alpha])
        });
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    fn size_of(webp: &[u8]) -> (u32, u32) {
        assert_eq!(&webp[..4], b"RIFF");
        assert_eq!(&webp[8..12], b"WEBP");
        let img = image::load_from_memory(webp).unwrap();
        (img.width(), img.height())
    }

    #[test]
    fn widths_sit_on_a_grid() {
        assert_eq!(grid_width(1), MIN_WIDTH);
        assert_eq!(grid_width(144), 144);
        assert_eq!(grid_width(145), 160);
        assert_eq!(grid_width(320), 320);
        assert_eq!(grid_width(321), 336);
        assert_eq!(grid_width(100_000), MAX_WIDTH);
        assert!(on_grid(320) && !on_grid(321) && !on_grid(100));
    }

    #[test]
    fn fits_within_never_enlarging() {
        assert_eq!(fitted(2000, 3000, Some(1000), Some(1000)), (667, 1000));
        assert_eq!(fitted(2000, 1000, Some(320), None), (320, 160));
        assert_eq!(fitted(200, 300, Some(1000), Some(1000)), (200, 300), "never up");
        assert_eq!(fitted(200, 300, None, None), (200, 300));
    }

    #[test]
    fn makes_smaller_lossy_webp() {
        let big = png(1200, 1600, 255);
        let small = to_webp(&big, Some(320), None, 80.0).unwrap();
        assert_eq!(size_of(&small), (320, 427));
        assert!(small.len() < big.len() / 4, "{} vs {}", small.len(), big.len());
        // Transparency survives; a picture is never made larger than it is.
        let clear = to_webp(&png(100, 50, 128), Some(320), Some(320), 80.0).unwrap();
        assert_eq!(size_of(&clear), (100, 50));
        assert!(image::load_from_memory(&clear).unwrap().color().has_alpha());
        assert!(matches!(to_webp(b"<html>", None, None, 80.0), Err(ArtError::Decode(_))));
    }

    #[test]
    fn copies_once_and_again_when_the_original_changes() {
        let dir = std::env::temp_dir().join(format!("sovemu-artwork-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (src, dst) = (dir.join("box.png"), dir.join("w/320.webp"));
        std::fs::write(&src, png(640, 480, 255)).unwrap();
        webp_copy(&src, &dst, 320, 80.0).unwrap();
        assert_eq!(size_of(&std::fs::read(&dst).unwrap()), (320, 240));
        let made = std::fs::metadata(&dst).unwrap().modified().unwrap();
        webp_copy(&src, &dst, 320, 80.0).unwrap();
        assert_eq!(std::fs::metadata(&dst).unwrap().modified().unwrap(), made, "kept");
        // A newer original is copied again.
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&src, png(640, 640, 255)).unwrap();
        webp_copy(&src, &dst, 320, 80.0).unwrap();
        assert_eq!(size_of(&std::fs::read(&dst).unwrap()), (320, 320));
        assert!(std::fs::read_dir(dir.join("w")).unwrap().count() == 1, "no temp files left");
        std::fs::remove_dir_all(&dir).ok();
    }
}
