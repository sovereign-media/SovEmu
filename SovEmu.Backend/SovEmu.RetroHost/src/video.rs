use sovemu_libretro_ffi::retro_pixel_format as fmt;

/// The pixel formats a software-rendering libretro core can output.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PixelFormat {
    /// 16-bit, `0RRRRRGGGGGBBBBB`. libretro's default until the core asks for another.
    #[default]
    Rgb1555,
    /// 32-bit, `0x00RRGGBB` in native byte order.
    Xrgb8888,
    /// 16-bit, `RRRRRGGGGGGBBBBB`.
    Rgb565,
    /// 32-bit, bytes R, G, B, A: what SovEmu reads back from a GPU core's framebuffer.
    Rgba8888,
}

impl PixelFormat {
    pub(crate) fn from_raw(raw: fmt::Type) -> Option<Self> {
        match raw {
            fmt::RETRO_PIXEL_FORMAT_0RGB1555 => Some(Self::Rgb1555),
            fmt::RETRO_PIXEL_FORMAT_XRGB8888 => Some(Self::Xrgb8888),
            fmt::RETRO_PIXEL_FORMAT_RGB565 => Some(Self::Rgb565),
            _ => None,
        }
    }

    /// Bytes per pixel.
    pub fn bytes_per_pixel(self) -> usize {
        match self {
            Self::Rgb1555 | Self::Rgb565 => 2,
            Self::Xrgb8888 | Self::Rgba8888 => 4,
        }
    }
}

/// One frame of video exactly as the core produced it, minus the row padding: rows are packed,
/// `width * format.bytes_per_pixel()` bytes each.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VideoFrame {
    pub width: u32,
    pub height: u32,
    pub format: PixelFormat,
    pub data: Vec<u8>,
}

impl VideoFrame {
    /// Copies a frame in from a core's buffer, dropping each row's padding.
    ///
    /// # Safety
    ///
    /// `data` must point to at least `height` rows of `pitch` bytes, each holding `width` pixels
    /// in `format`.
    pub(crate) unsafe fn copy_from_core(
        &mut self,
        data: *const u8,
        width: u32,
        height: u32,
        pitch: usize,
        format: PixelFormat,
    ) {
        let row = width as usize * format.bytes_per_pixel();
        self.width = width;
        self.height = height;
        self.format = format;
        self.data.clear();
        self.data.reserve(row * height as usize);
        for y in 0..height as usize {
            // SAFETY: the caller guarantees `height` rows of `pitch` bytes, each at least `row`
            // bytes long.
            let src = unsafe { std::slice::from_raw_parts(data.add(y * pitch), row) };
            self.data.extend_from_slice(src);
        }
    }

    /// Converts the frame to 8-bit RGBA (alpha always 255), replacing the contents of `out`.
    pub fn write_rgba8(&self, out: &mut Vec<u8>) {
        out.clear();
        out.reserve(self.width as usize * self.height as usize * 4);
        match self.format {
            PixelFormat::Rgba8888 => {
                for px in self.data.as_chunks::<4>().0 {
                    out.extend_from_slice(&[px[0], px[1], px[2], 255]);
                }
            }
            PixelFormat::Xrgb8888 => {
                for px in self.data.as_chunks::<4>().0 {
                    let v = u32::from_ne_bytes(*px);
                    out.extend_from_slice(&[(v >> 16) as u8, (v >> 8) as u8, v as u8, 255]);
                }
            }
            PixelFormat::Rgb565 => {
                for px in self.data.as_chunks::<2>().0 {
                    let v = u16::from_ne_bytes(*px);
                    out.extend_from_slice(&[
                        expand5((v >> 11) & 0x1f),
                        expand6((v >> 5) & 0x3f),
                        expand5(v & 0x1f),
                        255,
                    ]);
                }
            }
            PixelFormat::Rgb1555 => {
                for px in self.data.as_chunks::<2>().0 {
                    let v = u16::from_ne_bytes(*px);
                    out.extend_from_slice(&[
                        expand5((v >> 10) & 0x1f),
                        expand5((v >> 5) & 0x1f),
                        expand5(v & 0x1f),
                        255,
                    ]);
                }
            }
        }
    }

    /// The frame as 8-bit RGBA.
    pub fn to_rgba8(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.write_rgba8(&mut out);
        out
    }
}

/// Scales a 5-bit channel to 8 bits so that 0 → 0 and 31 → 255.
fn expand5(v: u16) -> u8 {
    ((v << 3) | (v >> 2)) as u8
}

/// Scales a 6-bit channel to 8 bits so that 0 → 0 and 63 → 255.
fn expand6(v: u16) -> u8 {
    ((v << 2) | (v >> 4)) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(format: PixelFormat, data: Vec<u8>) -> VideoFrame {
        let width = (data.len() / format.bytes_per_pixel()) as u32;
        VideoFrame { width, height: 1, format, data }
    }

    #[test]
    fn xrgb8888_to_rgba() {
        let px = 0x00_12_34_56u32.to_ne_bytes().to_vec();
        assert_eq!(frame(PixelFormat::Xrgb8888, px).to_rgba8(), [0x12, 0x34, 0x56, 255]);
    }

    #[test]
    fn rgb565_to_rgba() {
        let white = 0xffffu16.to_ne_bytes();
        let red = 0xf800u16.to_ne_bytes();
        let data = [white, red].concat();
        assert_eq!(
            frame(PixelFormat::Rgb565, data).to_rgba8(),
            [255, 255, 255, 255, 255, 0, 0, 255]
        );
    }

    #[test]
    fn rgb1555_to_rgba() {
        let green = 0x03e0u16.to_ne_bytes();
        let black = 0u16.to_ne_bytes();
        let data = [green, black].concat();
        assert_eq!(frame(PixelFormat::Rgb1555, data).to_rgba8(), [0, 255, 0, 255, 0, 0, 0, 255]);
    }

    #[test]
    fn copy_drops_row_padding() {
        // 2x2 XRGB8888 frame with 4 bytes of padding per row.
        let mut src = Vec::new();
        for row in [[1u32, 2], [3, 4]] {
            for px in row {
                src.extend_from_slice(&px.to_ne_bytes());
            }
            src.extend_from_slice(&[0xee; 4]);
        }
        let mut f = VideoFrame::default();
        // SAFETY: `src` holds 2 rows of 12 bytes, each with 2 XRGB8888 pixels.
        unsafe { f.copy_from_core(src.as_ptr(), 2, 2, 12, PixelFormat::Xrgb8888) };
        let pixels: Vec<u32> =
            f.data.as_chunks::<4>().0.iter().map(|b| u32::from_ne_bytes(*b)).collect();
        assert_eq!(pixels, [1, 2, 3, 4]);
    }
}
