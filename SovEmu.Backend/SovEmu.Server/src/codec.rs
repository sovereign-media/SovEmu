//! Which video codec a client's session is sent, decided when it offers (sovereign-media/SovEmu#184).
//!
//! `SOVEMU_VIDEO_CODEC` names a codec for everyone, or leaves it to `auto`: then each codec this
//! server can encode and the client can decode is priced, in milliseconds a frame, and the
//! cheapest is sent.
//!
//! - **Encoding** is what the startup encoder test measured on this host
//!   (`sovemu_sessions::encoders`), for the encoder a session would get, at the tested picture
//!   size nearest the game's. An encoder that did not work there is never chosen.
//! - **Decoding** is what this device measured the last time it played with that codec
//!   (`decode_times`), when it has; until then it is judged from what the device declares
//!   ([`ClientCapabilities`]). A declared hardware decoder can still be the slow one (a TV's
//!   H.264 decoder took 23 ms a frame where its software VP8 took 4), so a measurement always
//!   replaces the judgement.
//!
//! Prices closer than [`TIE_MS`] are a tie, since a short test at startup is not that exact: a
//! tie goes to the codec encoded on the GPU (it leaves the CPU to the emulator), then to the
//! one decoded in hardware, then to VP8.
//!
//! A plain function of its inputs, so it is tested with a table of devices and can be asked
//! again of a stored document.

use sovemu_sessions::encoders::{Encoder, EncoderTest};

use crate::capabilities::{ClientCapabilities, VideoCodec, VideoDecoder};

/// Prices within this of each other are a tie.
const TIE_MS: f64 = 1.0;
/// What a hardware decoder with a low-latency mode is taken to cost a frame, until measured.
const HARDWARE_DECODE_MS: f64 = 4.0;
/// What a hardware decoder without one adds: it may hold a frame back, at 60 a second.
const HELD_FRAME_MS: f64 = 16.7;
/// What a software decoder is taken to cost a 640×448 frame, until measured; other sizes in
/// proportion, within [`SOFTWARE_DECODE_RANGE_MS`].
const SOFTWARE_DECODE_MS: f64 = 6.0;
const SOFTWARE_DECODE_RANGE_MS: (f64, f64) = (2.0, 30.0);

/// The codecs the worker can encode, in the order a tie between equals falls.
const ENCODED: [VideoCodec; 2] = [VideoCodec::H264, VideoCodec::Vp8];

/// What the decision reads.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Inputs<'a> {
    /// `SOVEMU_VIDEO_CODEC`: `auto`, `vp8` or `h264`.
    pub(crate) setting: &'a str,
    /// `SOVEMU_VIDEO_ENCODER`: `auto`, `vaapi`, `nvenc` or `software`.
    pub(crate) encoder: &'a str,
    pub(crate) test: &'a EncoderTest,
    /// What the client sent with its offer, if it sent a document.
    pub(crate) capabilities: Option<&'a ClientCapabilities>,
    /// What this device's decoder has measured, by codec name (`vp8`), in ms a frame.
    pub(crate) measured: &'a [(String, f64)],
    /// The game's picture, in pixels.
    pub(crate) size: (u32, u32),
}

/// The decision: the codec to tell the worker for this client (none leaves the worker to its
/// own `--video-codec`, which a setting that names a codec already is), and why, in a line.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Choice {
    pub(crate) codec: Option<VideoCodec>,
    pub(crate) reason: String,
}

/// One codec priced for a client.
struct Priced<'a> {
    codec: VideoCodec,
    encoder: &'a Encoder,
    encode_ms: f64,
    decoder: &'a VideoDecoder,
    decode_ms: f64,
    measured: bool,
}

impl Priced<'_> {
    fn total(&self) -> f64 {
        self.encode_ms + self.decode_ms
    }

    /// Lower wins a tie: encoded on the GPU, then decoded in hardware, then [`ENCODED`]'s order
    /// turned round (VP8 first).
    fn tie_rank(&self) -> (bool, bool, bool) {
        (!self.encoder.hardware, !self.decoder.hardware, self.codec != VideoCodec::Vp8)
    }

    fn describe(&self) -> String {
        let decode = if self.measured {
            "measured".to_owned()
        } else {
            let how = if self.decoder.hardware { "hardware" } else { "software" };
            let latency = if self.decoder.low_latency { ", low latency" } else { "" };
            format!("declared {how}{latency}")
        };
        format!(
            "{} on {}: {:.1} ms to encode + {:.1} ms to decode ({decode}) = {:.1} ms",
            self.codec.name(),
            self.encoder.encoder,
            self.encode_ms,
            self.decode_ms,
            self.total()
        )
    }
}

/// The encoder a session would get for `codec`, if one works: VP8 is libvpx's; H.264 follows
/// `SOVEMU_VIDEO_ENCODER` as the worker does (the GPU's, falling back to x264).
fn session_encoder<'a>(
    test: &'a EncoderTest,
    codec: VideoCodec,
    preference: &str,
) -> Option<&'a Encoder> {
    let working = |name: &str| test.encoders.iter().find(|e| e.encoder == name && e.works);
    match codec {
        VideoCodec::Vp8 => working("libvpx"),
        VideoCodec::H264 => {
            let order: &[&str] = match preference.trim() {
                "vaapi" => &["vaapi", "x264"],
                "nvenc" => &["nvenc", "x264"],
                "software" | "x264" => &["x264"],
                _ => &["vaapi", "nvenc", "x264"],
            };
            order.iter().find_map(|name| working(name))
        }
        _ => None,
    }
}

/// What `encoder` took a frame at the tested size nearest `size`, by area.
fn encode_ms(encoder: &Encoder, size: (u32, u32)) -> f64 {
    let pixels = f64::from(size.0) * f64::from(size.1);
    encoder
        .timings
        .iter()
        .min_by(|a, b| {
            let distance = |t: &sovemu_sessions::encoders::Timing| {
                (f64::from(t.width) * f64::from(t.height) - pixels).abs()
            };
            distance(a).total_cmp(&distance(b))
        })
        .map_or(0.0, |t| t.mean_ms)
}

/// What a decoder is taken to cost a frame of `size` from what the device declares of it.
fn declared_decode_ms(decoder: &VideoDecoder, size: (u32, u32)) -> f64 {
    if decoder.hardware {
        HARDWARE_DECODE_MS + if decoder.low_latency { 0.0 } else { HELD_FRAME_MS }
    } else {
        let scale = f64::from(size.0) * f64::from(size.1) / (640.0 * 448.0);
        (SOFTWARE_DECODE_MS * scale).clamp(SOFTWARE_DECODE_RANGE_MS.0, SOFTWARE_DECODE_RANGE_MS.1)
    }
}

pub(crate) fn choose(i: &Inputs<'_>) -> Choice {
    let setting = i.setting.trim();
    if setting != "auto" && !setting.is_empty() {
        return Choice { codec: None, reason: format!("SOVEMU_VIDEO_CODEC={setting}") };
    }
    let vp8 = |reason: &str| Choice { codec: Some(VideoCodec::Vp8), reason: reason.to_owned() };
    let Some(capabilities) = i.capabilities else {
        return vp8("the client sent no capabilities: VP8, which every client decodes");
    };
    if i.test.encoders.is_empty() {
        return vp8("the startup encoder test gave no answer: VP8, which needs no GPU");
    }

    let mut priced: Vec<Priced<'_>> = ENCODED
        .iter()
        .filter_map(|&codec| {
            let decoder = capabilities.video_decoders.iter().find(|d| d.codec == codec)?;
            let encoder = session_encoder(i.test, codec, i.encoder)?;
            let measured = i.measured.iter().find(|(name, _)| name == codec.name()).map(|m| m.1);
            Some(Priced {
                codec,
                encoder,
                encode_ms: encode_ms(encoder, i.size),
                decoder,
                decode_ms: measured.unwrap_or_else(|| declared_decode_ms(decoder, i.size)),
                measured: measured.is_some(),
            })
        })
        .collect();
    if priced.is_empty() {
        return vp8("no codec this server encodes is one the client says it decodes: VP8");
    }
    let cheapest = priced.iter().map(Priced::total).fold(f64::INFINITY, f64::min);
    // The cheapest and everything within a tie of it, then the tie's order.
    priced.sort_by(|a, b| {
        let beyond = |p: &Priced<'_>| p.total() - cheapest > TIE_MS;
        let tie = |p: &Priced<'_>| if beyond(p) { (true, true, true) } else { p.tie_rank() };
        (beyond(a), tie(a)).cmp(&(beyond(b), tie(b))).then(a.total().total_cmp(&b.total()))
    });
    let reason = priced.iter().map(Priced::describe).collect::<Vec<_>>().join("; ");
    Choice { codec: Some(priced[0].codec), reason }
}

#[cfg(test)]
mod tests {
    use sovemu_sessions::encoders::Timing;

    use super::*;
    use crate::capabilities::tests::mi_tv;
    use crate::capabilities::{Display, VERSION};

    fn encoder(codec: &str, name: &str, hardware: bool, works: bool, ms_at_448p: f64) -> Encoder {
        let timing = |width, height, scale: f64| Timing {
            width,
            height,
            frames: 60,
            mean_ms: ms_at_448p * scale,
            p95_ms: ms_at_448p * scale * 1.2,
        };
        Encoder {
            codec: codec.into(),
            encoder: name.into(),
            hardware,
            threads: (!hardware).then_some(4),
            works,
            error: (!works).then(|| "it did not open".into()),
            timings: if works {
                vec![timing(256, 224, 0.3), timing(640, 448, 1.0), timing(1280, 720, 3.0)]
            } else {
                Vec::new()
            },
        }
    }

    fn test_of(encoders: Vec<Encoder>) -> EncoderTest {
        EncoderTest { encoders, took_ms: 1000, note: None, tested_at: None }
    }

    /// The cluster's server: an Intel GPU that encodes H.264, beside both software encoders.
    fn intel_server() -> EncoderTest {
        test_of(vec![
            encoder("vp8", "libvpx", false, true, 3.0),
            encoder("h264", "vaapi", true, true, 2.0),
            encoder("h264", "nvenc", true, false, 0.0),
            encoder("h264", "x264", false, true, 1.0),
        ])
    }

    /// A server with no GPU.
    fn cpu_server() -> EncoderTest {
        test_of(vec![
            encoder("vp8", "libvpx", false, true, 3.0),
            encoder("h264", "vaapi", true, false, 0.0),
            encoder("h264", "x264", false, true, 1.0),
        ])
    }

    fn decoder(codec: VideoCodec, hardware: bool, low_latency: bool) -> VideoDecoder {
        VideoDecoder { codec, hardware, low_latency, name: None }
    }

    fn device(decoders: Vec<VideoDecoder>) -> ClientCapabilities {
        ClientCapabilities {
            version: VERSION,
            video_decoders: decoders,
            display: Display { width: 1920, height: 1080, refresh_hz: 60.0 },
        }
    }

    fn choose_for(
        setting: &str,
        test: &EncoderTest,
        capabilities: Option<&ClientCapabilities>,
        measured: &[(&str, f64)],
    ) -> Choice {
        let measured: Vec<(String, f64)> =
            measured.iter().map(|(c, ms)| ((*c).to_owned(), *ms)).collect();
        choose(&Inputs {
            setting,
            encoder: "auto",
            test,
            capabilities,
            measured: &measured,
            size: (640, 448),
        })
    }

    #[test]
    fn a_setting_that_names_a_codec_decides_for_everyone() {
        let tv = mi_tv();
        for setting in ["vp8", "h264"] {
            let choice = choose_for(setting, &intel_server(), Some(&tv), &[]);
            assert_eq!(choice.codec, None, "the worker's own --video-codec is that setting");
            assert_eq!(choice.reason, format!("SOVEMU_VIDEO_CODEC={setting}"));
        }
    }

    #[test]
    fn a_client_with_no_document_or_a_server_with_no_test_gets_vp8() {
        let choice = choose_for("auto", &intel_server(), None, &[]);
        assert_eq!(choice.codec, Some(VideoCodec::Vp8));
        assert!(choice.reason.contains("no capabilities"));
        let unanswered = EncoderTest { note: Some("timed out".into()), ..EncoderTest::default() };
        let choice = choose_for("auto", &unanswered, Some(&mi_tv()), &[]);
        assert_eq!(choice.codec, Some(VideoCodec::Vp8));
        assert!(choice.reason.contains("gave no answer"));
    }

    #[test]
    fn two_devices_on_one_server_get_different_codecs() {
        let server = intel_server();
        // A phone that decodes H.264 in hardware with a low-latency mode: 2 + 4 against 3 + 6.
        let phone = device(vec![
            decoder(VideoCodec::Vp8, false, true),
            decoder(VideoCodec::H264, true, true),
        ]);
        let choice = choose_for("auto", &server, Some(&phone), &[]);
        assert_eq!(choice.codec, Some(VideoCodec::H264));
        assert!(choice.reason.starts_with("h264 on vaapi: 2.0 ms to encode + 4.0 ms to decode (declared hardware, low latency) = 6.0 ms; vp8 on libvpx: 3.0"), "{}", choice.reason);
        // A browser on a laptop that decodes both on the CPU: x264's millisecond is not in
        // play (the GPU encodes H.264 here), so 2 + 6 against 3 + 6 is a tie, and the GPU's wins.
        let laptop = device(vec![
            decoder(VideoCodec::Vp8, false, true),
            decoder(VideoCodec::H264, false, true),
        ]);
        assert_eq!(choose_for("auto", &server, Some(&laptop), &[]).codec, Some(VideoCodec::H264));
        // A device whose hardware H.264 decoder has no low-latency mode may hold a frame.
        let old_tv = device(vec![
            decoder(VideoCodec::Vp8, false, false),
            decoder(VideoCodec::H264, true, false),
        ]);
        assert_eq!(choose_for("auto", &server, Some(&old_tv), &[]).codec, Some(VideoCodec::Vp8));
        // A device that decodes only VP8.
        let vp8_only = device(vec![decoder(VideoCodec::Vp8, false, true)]);
        assert_eq!(choose_for("auto", &server, Some(&vp8_only), &[]).codec, Some(VideoCodec::Vp8));
    }

    #[test]
    fn a_measurement_replaces_what_the_device_declares() {
        // The Mi TV declares hardware H.264 with a low-latency mode, so its first session is
        // sent H.264; its decoder then measures 23 ms a frame, and the next gets VP8.
        let (server, tv) = (intel_server(), mi_tv());
        assert_eq!(choose_for("auto", &server, Some(&tv), &[]).codec, Some(VideoCodec::H264));
        let choice = choose_for("auto", &server, Some(&tv), &[("h264", 23.0)]);
        assert_eq!(choice.codec, Some(VideoCodec::Vp8));
        assert!(
            choice.reason.contains(
                "h264 on vaapi: 2.0 ms to encode + 23.0 ms to decode (measured) = 25.0 ms"
            ),
            "{}",
            choice.reason
        );
        // With both measured (VP8 at 7 ms on its CPU), VP8 stays.
        let choice = choose_for("auto", &server, Some(&tv), &[("h264", 23.0), ("vp8", 7.0)]);
        assert_eq!(choice.codec, Some(VideoCodec::Vp8));
        // A device that measures H.264 fast keeps it.
        let choice = choose_for("auto", &server, Some(&tv), &[("h264", 3.0), ("vp8", 7.0)]);
        assert_eq!(choice.codec, Some(VideoCodec::H264));
    }

    #[test]
    fn an_encoder_that_does_not_work_is_never_chosen() {
        let phone = device(vec![
            decoder(VideoCodec::Vp8, false, true),
            decoder(VideoCodec::H264, true, true),
        ]);
        // No GPU: H.264 is x264's, which is the cheaper pair (1 + 4 against 3 + 6).
        let choice = choose_for("auto", &cpu_server(), Some(&phone), &[]);
        assert_eq!(choice.codec, Some(VideoCodec::H264));
        assert!(choice.reason.starts_with("h264 on x264"), "{}", choice.reason);
        // No H.264 encoder at all: VP8, whatever the device would rather decode.
        let vp8_server = test_of(vec![
            encoder("vp8", "libvpx", false, true, 3.0),
            encoder("h264", "vaapi", true, false, 0.0),
            encoder("h264", "x264", false, false, 0.0),
        ]);
        let choice = choose_for("auto", &vp8_server, Some(&phone), &[]);
        assert_eq!(choice.codec, Some(VideoCodec::Vp8));
        assert!(!choice.reason.contains("h264"), "{}", choice.reason);
        // SOVEMU_VIDEO_ENCODER=software prices H.264 as x264 even where the GPU works.
        let measured: Vec<(String, f64)> = Vec::new();
        let choice = choose(&Inputs {
            setting: "auto",
            encoder: "software",
            test: &intel_server(),
            capabilities: Some(&phone),
            measured: &measured,
            size: (640, 448),
        });
        assert!(choice.reason.starts_with("h264 on x264: 1.0 ms"), "{}", choice.reason);
    }

    #[test]
    fn a_picture_is_priced_at_the_tested_size_nearest_its_own() {
        let phone = device(vec![decoder(VideoCodec::Vp8, false, true)]);
        let measured: Vec<(String, f64)> = Vec::new();
        let at = |size| {
            choose(&Inputs {
                setting: "auto",
                encoder: "auto",
                test: &intel_server(),
                capabilities: Some(&phone),
                measured: &measured,
                size,
            })
            .reason
        };
        // A SNES picture: the 256×224 timing, and a small software decode.
        assert!(
            at((256, 239)).starts_with("vp8 on libvpx: 0.9 ms to encode + 2.0 ms to decode"),
            "{}",
            at((256, 239))
        );
        // 720p: three times the 448p timing, and a larger decode.
        assert!(
            at((1280, 720)).starts_with("vp8 on libvpx: 9.0 ms to encode + 19.3 ms"),
            "{}",
            at((1280, 720))
        );
    }
}
