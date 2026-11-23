//! What a client says it can decode and show: one complete, versioned document, sent whole with
//! every `sessionOffer` and kept against the sign-in's device (`SovEmu.Identity`, as JSON). As
//! Sovereign's `ClientCapabilities`, but far smaller: there are no source files here, so only the
//! decoders and the display are left.
//!
//! Every reader is strict: a document of another version is refused with a message for the
//! player, and a stored one that no longer reads is treated as absent, never guessed at.

use async_graphql::{Enum, InputObject, SimpleObject};
use serde::{Deserialize, Serialize};

/// The document's shape this server reads.
pub(crate) const VERSION: i32 = 1;

/// Most decoder rows a document may carry: one per codec, with room for codecs yet to come.
const MAX_DECODERS: usize = 16;
/// Longest decoder name kept.
const MAX_NAME: usize = 80;
/// Widest and tallest display believed, in pixels.
const MAX_DISPLAY: i32 = 16_384;

/// A video codec a client may decode. The worker encodes VP8 and H.264 today; the rest are
/// named so a client's document need not change when their encoders arrive.
#[derive(Enum, Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum VideoCodec {
    #[graphql(name = "VP8")]
    Vp8,
    #[graphql(name = "H264")]
    H264,
    #[graphql(name = "VP9")]
    Vp9,
    #[graphql(name = "AV1")]
    Av1,
    #[graphql(name = "H265")]
    H265,
}

impl VideoCodec {
    /// Its name in logs and settings (`vp8`, `h264`).
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Vp8 => "vp8",
            Self::H264 => "h264",
            Self::Vp9 => "vp9",
            Self::Av1 => "av1",
            Self::H265 => "h265",
        }
    }
}

/// How a client decodes one codec: the decoder it would use for a game's stream.
#[derive(SimpleObject, InputObject, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[graphql(input_name = "VideoDecoderInput")]
#[serde(rename_all = "camelCase")]
pub(crate) struct VideoDecoder {
    pub(crate) codec: VideoCodec,
    /// The device's hardware decodes it; false for a decoder that runs on the CPU.
    pub(crate) hardware: bool,
    /// The decoder hands over each frame as soon as it is decoded, holding none back.
    pub(crate) low_latency: bool,
    /// The decoder's own name (`c2.amlogic.avc.decoder`, `libvpx`), for logs and the admin's
    /// view: nothing is decided from it. Kept to 80 characters.
    pub(crate) name: Option<String>,
}

/// The screen a client shows games on.
#[derive(SimpleObject, InputObject, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[graphql(input_name = "DisplayInput")]
#[serde(rename_all = "camelCase")]
pub(crate) struct Display {
    /// In physical pixels.
    pub(crate) width: i32,
    pub(crate) height: i32,
    /// Pictures a second.
    pub(crate) refresh_hz: f64,
}

/// Everything a client declares about what it can play: one complete document, sent whole with
/// every `sessionOffer`. The server never reads a missing fact as permission.
#[derive(SimpleObject, InputObject, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[graphql(input_name = "ClientCapabilitiesInput")]
#[serde(rename_all = "camelCase")]
pub(crate) struct ClientCapabilities {
    /// The document's shape: 1. A document of any other version is refused (`BAD_REQUEST`,
    /// with a message asking the player to update the app).
    pub(crate) version: i32,
    /// Every video codec the client can decode, one row each: the decoder it would use.
    pub(crate) video_decoders: Vec<VideoDecoder>,
    pub(crate) display: Display,
}

impl ClientCapabilities {
    /// Checks a document a client sent and tidies its decoder names. The error is a message
    /// for the player.
    pub(crate) fn checked(mut self) -> Result<Self, String> {
        if self.version != VERSION {
            return Err(format!(
                "This app reports its capabilities as version {}, and this server reads version \
                 {VERSION}. Update the app.",
                self.version
            ));
        }
        if self.video_decoders.is_empty() || self.video_decoders.len() > MAX_DECODERS {
            return Err(format!("capabilities name 1 to {MAX_DECODERS} video decoders"));
        }
        for (i, decoder) in self.video_decoders.iter().enumerate() {
            if self.video_decoders[..i].iter().any(|d| d.codec == decoder.codec) {
                return Err(format!(
                    "capabilities name {} twice: one decoder for each codec",
                    decoder.codec.name()
                ));
            }
        }
        for decoder in &mut self.video_decoders {
            decoder.name = decoder.name.take().and_then(|n| clean(&n));
        }
        let Display { width, height, refresh_hz } = self.display;
        let sized = (1..=MAX_DISPLAY).contains(&width) && (1..=MAX_DISPLAY).contains(&height);
        if !sized || !refresh_hz.is_finite() || !(1.0..=1000.0).contains(&refresh_hz) {
            return Err("capabilities describe a display that cannot be".into());
        }
        Ok(self)
    }

    /// The document as it is kept.
    pub(crate) fn to_json(&self) -> String {
        serde_json::to_string(self).expect("a capabilities document is plain data")
    }

    /// A kept document, if it still reads as one of this server's version.
    pub(crate) fn from_json(json: &str) -> Option<Self> {
        serde_json::from_str::<Self>(json).ok()?.checked().ok()
    }

    /// The decoders in a line, for logs: `vp8 software, h264 hardware low-latency`.
    pub(crate) fn summary(&self) -> String {
        let rows: Vec<String> = self
            .video_decoders
            .iter()
            .map(|d| {
                let how = if d.hardware { "hardware" } else { "software" };
                let latency = if d.low_latency { " low-latency" } else { "" };
                format!("{} {how}{latency}", d.codec.name())
            })
            .collect();
        rows.join(", ")
    }
}

/// A decoder's name as kept: one line, control characters gone, at most [`MAX_NAME`]
/// characters; none if nothing is left.
fn clean(name: &str) -> Option<String> {
    let name: String = name
        .split(|c: char| c.is_whitespace() || c.is_control())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(MAX_NAME)
        .collect();
    (!name.is_empty()).then_some(name)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn decoder(codec: VideoCodec, hardware: bool, low_latency: bool, name: &str) -> VideoDecoder {
        VideoDecoder { codec, hardware, low_latency, name: Some(name.into()) }
    }

    /// The Mi TV (Amlogic S7D, Android 14) the server was first tuned on: every codec but VP8
    /// in hardware with a low-latency mode, VP8 on the CPU.
    pub(crate) fn mi_tv() -> ClientCapabilities {
        ClientCapabilities {
            version: VERSION,
            video_decoders: vec![
                decoder(VideoCodec::Vp8, false, false, "c2.amlogic.vp8.decoder.sw"),
                decoder(VideoCodec::H264, true, true, "c2.amlogic.avc.decoder"),
                decoder(VideoCodec::Vp9, true, true, "c2.amlogic.vp9.decoder"),
                decoder(VideoCodec::Av1, true, true, "c2.amlogic.av1.decoder"),
                decoder(VideoCodec::H265, true, true, "c2.amlogic.hevc.decoder"),
            ],
            display: Display { width: 3840, height: 2160, refresh_hz: 60.0 },
        }
    }

    #[test]
    fn a_whole_document_of_this_version_is_kept_and_read_back() {
        let document = mi_tv().checked().unwrap();
        assert_eq!(ClientCapabilities::from_json(&document.to_json()), Some(document.clone()));
        assert!(document.to_json().contains(r#""codec":"h264","hardware":true,"lowLatency":true"#));
        assert_eq!(
            document.summary(),
            "vp8 software, h264 hardware low-latency, vp9 hardware low-latency, \
             av1 hardware low-latency, h265 hardware low-latency"
        );
    }

    #[test]
    fn another_version_is_refused_with_a_message_for_the_player() {
        let error = ClientCapabilities { version: 2, ..mi_tv() }.checked().unwrap_err();
        assert!(error.contains("version 2") && error.ends_with("Update the app."), "{error}");
        // A kept document of another version is absent, not guessed at.
        let kept = ClientCapabilities { version: 2, ..mi_tv() }.to_json();
        assert_eq!(ClientCapabilities::from_json(&kept), None);
        assert_eq!(ClientCapabilities::from_json("{}"), None);
    }

    #[test]
    fn a_document_names_each_codec_once_and_a_display_that_can_be() {
        assert!(ClientCapabilities { video_decoders: vec![], ..mi_tv() }.checked().is_err());
        let mut twice = mi_tv();
        twice.video_decoders.push(decoder(VideoCodec::Vp8, true, true, "another"));
        assert!(twice.checked().unwrap_err().contains("vp8 twice"));
        for display in [
            Display { width: 0, height: 1080, refresh_hz: 60.0 },
            Display { width: 1920, height: 100_000, refresh_hz: 60.0 },
            Display { width: 1920, height: 1080, refresh_hz: f64::NAN },
            Display { width: 1920, height: 1080, refresh_hz: 0.0 },
        ] {
            assert!(ClientCapabilities { display, ..mi_tv() }.checked().is_err());
        }
    }

    #[test]
    fn decoder_names_are_one_short_line() {
        let mut document = mi_tv();
        document.video_decoders[0].name = Some(format!(" c2.vendor\n{} ", "x".repeat(200)));
        document.video_decoders[1].name = Some(" \u{7} ".into());
        let checked = document.checked().unwrap();
        let name = checked.video_decoders[0].name.as_deref().unwrap();
        assert!(name.starts_with("c2.vendor x") && name.chars().count() == MAX_NAME);
        assert_eq!(checked.video_decoders[1].name, None);
    }
}
