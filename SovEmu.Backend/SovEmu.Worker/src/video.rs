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
}

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
}

impl Video {
    pub(crate) fn new(settings: VideoSettings) -> Self {
        let codec = match settings.codec {
            CodecSetting::Only(VideoCodec::Vp8) => VideoCodec::Vp8,
            _ => VideoCodec::H264,
        };
        let encoder = Self::make(&settings, codec, settings.bitrate_kbps);
        Self { client_codec: settings.codec, settings, encoder, logged: None }
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
            width,
            height,
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
        }
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
