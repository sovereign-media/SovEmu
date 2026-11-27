//! The session's video encoder: H.264 or VP8, whichever the client and `SovEmu__Streaming__Video__Codec`
//! agree on.
//!
//! H.264 is preferred: every client decodes it in hardware (a TV's or a phone's decoder is
//! made for it), and it can be encoded on the GPU. VP8 stays for a client that offers no
//! H.264. The codec is decided per client, from its offer, while the encoder belongs to the
//! session (it outlives clients), so a client that negotiates the other codec gets a new
//! encoder, starting with a keyframe.

use sovemu_media::{EncodedFrame, H264Config, H264Encoder, H264Preference, Vp8Config, Vp8Encoder};
use sovemu_protocol::{Fallback, VideoPath};

/// A video codec the worker can send.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VideoCodec {
    H264,
    Vp8,
}

impl VideoCodec {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::H264 => "h264",
            Self::Vp8 => "vp8",
        }
    }
}

/// `--video-codec`: `auto` (H.264 if the client offers it, else VP8), `h264` or `vp8`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum CodecSetting {
    #[default]
    Auto,
    Only(VideoCodec),
}

impl std::str::FromStr for CodecSetting {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.trim() {
            "" | "auto" => Self::Auto,
            "h264" => Self::Only(VideoCodec::H264),
            "vp8" => Self::Only(VideoCodec::Vp8),
            other => return Err(format!("unknown video codec {other:?} (auto, h264 or vp8)")),
        })
    }
}

impl CodecSetting {
    /// Whether to offer this codec in the answer at all.
    pub(crate) fn allows(self, codec: VideoCodec) -> bool {
        self == Self::Auto || self == Self::Only(codec)
    }

    /// The codec to send, given what the client's offer has.
    fn pick(self, h264: bool, vp8: bool) -> Option<VideoCodec> {
        match self {
            Self::Auto if h264 => Some(VideoCodec::H264),
            Self::Auto if vp8 => Some(VideoCodec::Vp8),
            Self::Only(c) if (c == VideoCodec::H264 && h264) || (c == VideoCodec::Vp8 && vp8) => {
                Some(c)
            }
            _ => None,
        }
    }
}

/// What the session's encoders are made with.
#[derive(Debug, Clone)]
pub(crate) struct VideoSettings {
    pub(crate) codec: CodecSetting,
    pub(crate) encoder: H264Preference,
    /// The render node for VA-API (the GPU the server chose), if there is one.
    pub(crate) device: Option<String>,
    pub(crate) bitrate_kbps: u32,
    pub(crate) fps: u32,
    pub(crate) keyframe_interval: u32,
    /// Pictures are scaled up this many times (1 or 2) by repeating pixels before encoding
    /// (`SovEmu__Systems__<id>__Prescale`), while the result fits [`PRESCALE_MAX`].
    pub(crate) prescale: u32,
    /// VP8's screen-content mode, 0 to 2 (`SovEmu__Streaming__Video__ScreenContent`).
    pub(crate) screen_content: u8,
}

/// The largest picture a pre-scale makes, width by height: a PC Engine's 352×240 doubled, the
/// largest measured (sovereign-media/SovEmu#266). A frame larger than half of it (a SNES game's
/// 512×448 hi-res screens, a PlayStation's 368-wide and 640×480 modes) is encoded at its own
/// size: four times its pixels were never measured, and such a frame already has a pixel per
/// chroma sample's worth of detail to spare. A SNES game switching to hi-res then keeps its
/// stream's size, 512×448 either way.
const PRESCALE_MAX: (u32, u32) = (704, 480);

enum Encoder {
    H264(H264Encoder),
    Vp8(Vp8Encoder),
}

pub(crate) struct Video {
    settings: VideoSettings,
    /// The codecs allowed for the client being answered: the server's choice for it, when it
    /// made one, else the setting.
    client_codec: CodecSetting,
    encoder: Encoder,
    /// The encoder last logged, so a change (a fallback) is logged once.
    logged: Option<&'static str>,
    /// The pre-scaled picture, kept between frames (empty at a prescale of 1).
    scaled: Vec<u8>,
}

impl Video {
    pub(crate) fn new(settings: VideoSettings) -> Self {
        let codec = match settings.codec {
            CodecSetting::Only(VideoCodec::Vp8) => VideoCodec::Vp8,
            _ => VideoCodec::H264,
        };
        let encoder = Self::make(&settings, codec, settings.bitrate_kbps);
        Self { client_codec: settings.codec, settings, encoder, logged: None, scaled: Vec::new() }
    }

    fn make(settings: &VideoSettings, codec: VideoCodec, bitrate_kbps: u32) -> Encoder {
        match codec {
            VideoCodec::H264 => Encoder::H264(H264Encoder::new(H264Config {
                bitrate_kbps,
                fps: settings.fps,
                keyframe_interval: settings.keyframe_interval,
                preference: settings.encoder,
                device: settings.device.clone(),
            })),
            VideoCodec::Vp8 => Encoder::Vp8(Vp8Encoder::new(Vp8Config {
                bitrate_kbps,
                fps: settings.fps,
                keyframe_interval: settings.keyframe_interval,
                screen_content: settings.screen_content,
                ..Vp8Config::default()
            })),
        }
    }

    pub(crate) fn codec_setting(&self) -> CodecSetting {
        self.settings.codec
    }

    /// The codecs the next client may be sent: what its answer offered it.
    pub(crate) fn allow_for_client(&mut self, codecs: CodecSetting) {
        self.client_codec = codecs;
    }

    pub(crate) fn codec(&self) -> VideoCodec {
        match self.encoder {
            Encoder::H264(_) => VideoCodec::H264,
            Encoder::Vp8(_) => VideoCodec::Vp8,
        }
    }

    /// Picks the codec for a client whose offer has (or lacks) each one, switching encoders if
    /// the last client used the other. `None`: nothing in common.
    pub(crate) fn negotiate(&mut self, h264: bool, vp8: bool) -> Option<VideoCodec> {
        let codec = self.client_codec.pick(h264, vp8)?;
        if codec != self.codec() {
            self.encoder = Self::make(&self.settings, codec, self.bitrate_kbps());
            self.logged = None;
        }
        Some(codec)
    }

    pub(crate) fn bitrate_kbps(&self) -> u32 {
        match &self.encoder {
            Encoder::H264(e) => e.config().bitrate_kbps,
            Encoder::Vp8(e) => e.config().bitrate_kbps,
        }
    }

    /// The encoder in use: `vaapi`, `nvenc`, `x264` or `libvpx` (none until H.264 has opened
    /// one on its first frame).
    pub(crate) fn encoder_name(&self) -> Option<&'static str> {
        match &self.encoder {
            Encoder::H264(e) => e.backend().map(|b| b.name()),
            Encoder::Vp8(_) => Some("libvpx"),
        }
    }

    /// How many times a `width`×`height` picture is scaled up before it is encoded.
    fn scale(&self, width: u32, height: u32) -> u32 {
        let p = self.settings.prescale.max(1);
        if p > 1 && width * p <= PRESCALE_MAX.0 && height * p <= PRESCALE_MAX.1 { p } else { 1 }
    }

    /// The video half of the pipeline the client and the server are told about.
    pub(crate) fn path(&self, width: u32, height: u32, fps: f64) -> VideoPath {
        let (hardware, fallbacks) = match &self.encoder {
            Encoder::H264(e) => (
                e.backend().is_some_and(|b| b.is_hardware()),
                e.fallbacks()
                    .map(|(b, why)| Fallback { encoder: b.name().into(), reason: why.into() })
                    .collect(),
            ),
            Encoder::Vp8(_) => (false, Vec::new()),
        };
        VideoPath {
            codec: self.codec().name().into(),
            encoder: self.encoder_name().unwrap_or_default().into(),
            hardware,
            fallbacks,
            width: width * self.scale(width, height),
            height: height * self.scale(width, height),
            fps: (fps * 100.0).round() / 100.0,
            // The session's, which knows what the player was last told.
            kbps: 0,
            limited_by: String::new(),
            estimate_kbps: 0,
        }
    }

    pub(crate) fn request_keyframe(&mut self) {
        match &mut self.encoder {
            Encoder::H264(e) => e.request_keyframe(),
            Encoder::Vp8(e) => e.request_keyframe(),
        }
    }

    /// Whether changing the bitrate reopens the encoder (with a keyframe): the hardware
    /// encoders take their rate control at open.
    pub(crate) fn rate_change_reopens(&self) -> bool {
        match &self.encoder {
            Encoder::H264(e) => e.backend().is_some_and(|b| b.is_hardware()),
            Encoder::Vp8(_) => false,
        }
    }

    pub(crate) fn set_bitrate(&mut self, kbps: u32) -> Result<(), sovemu_media::Error> {
        match &mut self.encoder {
            Encoder::H264(e) => e.set_bitrate(kbps),
            Encoder::Vp8(e) => e.set_bitrate(kbps),
        }
    }

    pub(crate) fn encode(
        &mut self,
        rgba: &[u8],
        width: u32,
        height: u32,
    ) -> Result<Option<EncodedFrame>, sovemu_media::Error> {
        let (rgba, width, height) = match self.scale(width, height) {
            2 => {
                prescale_2x(rgba, width as usize, height as usize, &mut self.scaled);
                (self.scaled.as_slice(), width * 2, height * 2)
            }
            _ => (rgba, width, height),
        };
        let encoded = match &mut self.encoder {
            Encoder::H264(e) => e.encode(rgba, width, height),
            Encoder::Vp8(e) => e.encode(rgba, width, height),
        }?;
        let name = self.encoder_name();
        if name.is_some() && name != self.logged {
            self.logged = name;
            tracing::info!(
                codec = self.codec().name(),
                encoder = name.unwrap_or_default(),
                hardware = !matches!(name, Some("x264" | "libvpx")),
                "video encoder"
            );
        }
        Ok(encoded)
    }
}

/// Scales tightly packed RGBA up 2× by repeating each pixel into a 2×2 block.
fn prescale_2x(rgba: &[u8], width: usize, height: usize, out: &mut Vec<u8>) {
    out.clear();
    out.reserve(width * height * 16);
    for row in rgba.chunks_exact(width * 4).take(height) {
        let start = out.len();
        for pixel in row.as_chunks::<4>().0 {
            out.extend_from_slice(pixel);
            out.extend_from_slice(pixel);
        }
        out.extend_from_within(start..);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_prefers_h264_and_falls_back_to_vp8() {
        let auto = CodecSetting::Auto;
        assert_eq!(auto.pick(true, true), Some(VideoCodec::H264));
        assert_eq!(auto.pick(false, true), Some(VideoCodec::Vp8));
        assert_eq!(auto.pick(false, false), None);
        let vp8: CodecSetting = "vp8".parse().unwrap();
        assert_eq!(vp8.pick(true, true), Some(VideoCodec::Vp8));
        assert!(!vp8.allows(VideoCodec::H264));
        let h264: CodecSetting = "h264".parse().unwrap();
        assert_eq!(h264.pick(false, true), None, "only H.264: a VP8-only client gets no video");
        assert!("av1".parse::<CodecSetting>().is_err());
    }

    fn settings(codec: CodecSetting) -> VideoSettings {
        VideoSettings {
            codec,
            encoder: "software".parse().unwrap(),
            device: None,
            bitrate_kbps: 1000,
            fps: 60,
            keyframe_interval: 600,
            prescale: 1,
            screen_content: 0,
        }
    }

    #[test]
    fn prescale_2x_makes_each_pixel_a_block() {
        let rgba = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
        let mut out = Vec::new();
        prescale_2x(&rgba, 2, 2, &mut out);
        let p = |i: u8| [i * 4 + 1, i * 4 + 2, i * 4 + 3, i * 4 + 4];
        let row = |a: u8, b: u8| [p(a), p(a), p(b), p(b)].concat();
        assert_eq!(out, [row(0, 1), row(0, 1), row(2, 3), row(2, 3)].concat());
    }

    #[test]
    fn a_prescaled_stream_is_twice_the_size_and_says_so() {
        for codec in [VideoCodec::Vp8, VideoCodec::H264] {
            let mut video =
                Video::new(VideoSettings { prescale: 2, ..settings(CodecSetting::Only(codec)) });
            let rgba = [200u8, 30, 30, 255].repeat(64 * 48);
            let frame = video.encode(&rgba, 64, 48).unwrap().expect("a frame");
            let size = match codec {
                VideoCodec::Vp8 => {
                    let pic = sovemu_media::Vp8Decoder::new().unwrap().decode(&frame.data);
                    pic.unwrap().map(|p| (p.width, p.height))
                }
                VideoCodec::H264 => {
                    let pic = sovemu_media::H264Decoder::new().unwrap().decode(&frame.data);
                    pic.unwrap().map(|p| (p.width, p.height))
                }
            };
            assert_eq!(size, Some((128, 96)), "{codec:?}");
            let path = video.path(64, 48, 60.0);
            assert_eq!((path.width, path.height), (128, 96), "{codec:?}");
        }
    }

    #[test]
    fn only_frames_that_fit_once_doubled_are_prescaled() {
        let video = Video::new(VideoSettings {
            prescale: 2,
            ..settings(CodecSetting::Only(VideoCodec::Vp8))
        });
        // NES, SNES, GBA, Game Boy, Genesis (and PAL), PC Engine.
        for (w, h) in
            [(256, 240), (256, 224), (240, 160), (160, 144), (320, 224), (320, 240), (352, 240)]
        {
            assert_eq!(video.scale(w, h), 2, "{w}×{h}");
        }
        // SNES hi-res, and its interlaced-only and wide-only modes; a PlayStation's 368-wide and
        // 640×480 modes.
        for (w, h) in [(512, 448), (256, 448), (512, 224), (368, 240), (640, 480)] {
            assert_eq!(video.scale(w, h), 1, "{w}×{h}");
        }
        let path = video.path(512, 448, 60.0);
        assert_eq!((path.width, path.height), (512, 448), "hi-res keeps the 2× stream's size");
        let off = Video::new(settings(CodecSetting::Only(VideoCodec::Vp8)));
        assert_eq!(off.scale(160, 144), 1);
    }

    #[test]
    fn the_servers_choice_for_a_client_is_used_over_the_setting() {
        // The setting says VP8; the server chose H.264 for this client, so its answer offered
        // only H.264, and that is what it is sent. The next client is the setting's again.
        let mut video = Video::new(settings(CodecSetting::Only(VideoCodec::Vp8)));
        assert_eq!(video.codec(), VideoCodec::Vp8);
        video.allow_for_client("h264".parse().unwrap());
        assert_eq!(video.negotiate(true, false), Some(VideoCodec::H264));
        assert_eq!(video.codec(), VideoCodec::H264);
        video.allow_for_client(video.codec_setting());
        assert_eq!(video.negotiate(true, true), Some(VideoCodec::Vp8));
    }

    #[test]
    fn a_client_with_the_other_codec_gets_a_new_encoder_at_the_same_bitrate() {
        let mut video = Video::new(settings(CodecSetting::Auto));
        assert_eq!(video.codec(), VideoCodec::H264);
        video.set_bitrate(700).unwrap();
        assert_eq!(video.negotiate(false, true), Some(VideoCodec::Vp8));
        assert_eq!((video.codec(), video.bitrate_kbps()), (VideoCodec::Vp8, 700));
        let rgba = vec![128; 64 * 64 * 4];
        assert!(video.encode(&rgba, 64, 64).unwrap().unwrap().keyframe);
        assert_eq!(video.encoder_name(), Some("libvpx"));
        assert_eq!(video.negotiate(true, true), Some(VideoCodec::H264));
        assert!(
            video.encode(&rgba, 64, 64).unwrap().unwrap().keyframe,
            "a new encoder starts with a keyframe"
        );
        assert_eq!(video.encoder_name(), Some("x264"));
    }
}
