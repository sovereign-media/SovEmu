//! Testing only: a recorded clip shown instead of the game's picture
//! (`SovEmu__Streaming__Debug__ReplayFrames`, sovereign-media/SovEmu#263).
//!
//! The clip is a folder of `frame_NNNNNN.png`, the core's own frames as the encoder bench
//! captured them. Each time the game draws, the next clip frame is encoded in its place and the
//! clip loops at its end, so a client decodes exactly the frames the bench scored, through the
//! real session: its encoder, rate control, network and decoder. The game itself (its sound, its
//! inputs and its saves) runs as usual underneath. Frames are read as they are needed: one PNG at
//! console size decodes in well under a millisecond.

use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

#[derive(Debug)]
pub(crate) struct Replay {
    frames: Vec<PathBuf>,
    next: usize,
}

impl Replay {
    pub(crate) fn open(dir: &Path) -> Result<Self> {
        let mut frames: Vec<PathBuf> = std::fs::read_dir(dir)
            .with_context(|| format!("reading the replay frames in {}", dir.display()))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("frame_") && n.ends_with(".png"))
            })
            .collect();
        if frames.is_empty() {
            bail!("no frame_*.png in {}", dir.display());
        }
        frames.sort();
        tracing::warn!(
            frames = frames.len(),
            dir = %dir.display(),
            "replaying recorded frames instead of the game's picture (testing only)"
        );
        Ok(Self { frames, next: 0 })
    }

    /// Writes the next frame into `rgba` (tightly packed) and returns its size.
    pub(crate) fn next(&mut self, rgba: &mut Vec<u8>) -> Result<(u32, u32)> {
        let path = &self.frames[self.next];
        self.next = (self.next + 1) % self.frames.len();
        let file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
        let mut reader = png::Decoder::new(BufReader::new(file)).read_info()?;
        let mut buf = vec![0; reader.output_buffer_size().context("a PNG too large")?];
        let info = reader.next_frame(&mut buf)?;
        let pixels = info.width as usize * info.height as usize;
        rgba.clear();
        match info.color_type {
            png::ColorType::Rgba => rgba.extend_from_slice(&buf[..pixels * 4]),
            png::ColorType::Rgb => {
                for p in buf[..pixels * 3].as_chunks::<3>().0 {
                    rgba.extend_from_slice(&[p[0], p[1], p[2], 255]);
                }
            }
            other => bail!("{}: {other:?} is not RGB or RGBA", path.display()),
        }
        Ok((info.width, info.height))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_png(path: &Path, rgba: &[u8], w: u32, h: u32) {
        let mut e = png::Encoder::new(File::create(path).unwrap(), w, h);
        e.set_color(png::ColorType::Rgba);
        e.set_depth(png::BitDepth::Eight);
        e.write_header().unwrap().write_image_data(rgba).unwrap();
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sovemu-replay-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn plays_the_frames_in_order_and_loops() {
        let dir = scratch("loops");
        for n in 0..3u8 {
            let path = dir.join(format!("frame_{n:06}.png"));
            write_png(&path, &[n, 0, 0, 255].repeat(4 * 2), 4, 2);
        }
        std::fs::write(dir.join("command.txt"), "not a frame").unwrap();
        let mut replay = Replay::open(&dir).unwrap();
        let mut rgba = Vec::new();
        let firsts: Vec<u8> = (0..5)
            .map(|_| {
                assert_eq!(replay.next(&mut rgba).unwrap(), (4, 2));
                assert_eq!(rgba.len(), 4 * 2 * 4);
                rgba[0]
            })
            .collect();
        assert_eq!(firsts, [0, 1, 2, 0, 1]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_empty_folder_is_refused() {
        let dir = scratch("empty");
        assert!(Replay::open(&dir).is_err());
    }
}
