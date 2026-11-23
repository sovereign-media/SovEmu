//! Encoding for the SovEmu stream.
//!
//! - [`yuv`]: RGBA → I420 or NV12 (BT.601, limited range), the input every video codec wants.
//! - [`vp8`]: VP8 through the system libvpx, tuned for real time: one pass, CBR, no lag, error
//!   resilient, keyframes on demand.
//! - [`h264`]: H.264 through FFmpeg's libavcodec, on the GPU's encoder where there is one
//!   (VA-API, NVENC) and x264 where there is not: Constrained Baseline, no
//!   B-frames, no lookahead, keyframes on demand.
//! - [`opus`]: Opus through the system libopus, restricted-low-delay mode.
//! - [`audio`]: [`AudioEncoder`], which takes a core's 16-bit stereo at whatever rate it runs
//!   (32 040.5 Hz for a SNES), resamples to 48 kHz and cuts it into 10 ms Opus packets.
//!
//! This crate knows nothing about libretro: it takes RGBA frames and interleaved samples.
//! Video is encoded at the console's native resolution; clients scale it up.

mod ffi;

pub mod audio;
pub mod h264;
pub mod opus;
pub mod vp8;
pub mod yuv;

pub use audio::AudioEncoder;
pub use h264::{H264Backend, H264Config, H264Decoder, H264Encoder, H264Preference, X264_THREADS};
pub use opus::{OpusDecoder, OpusEncoder};
pub use vp8::{Vp8Config, Vp8Decoder, Vp8Encoder};

/// One encoded frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedFrame {
    pub data: Vec<u8>,
    pub keyframe: bool,
}

/// Errors from the codec libraries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// libvpx returned an error: the operation and libvpx's message.
    Vpx(&'static str, String),
    /// libopus returned an error: the operation and libopus's message.
    Opus(&'static str, String),
    /// libavcodec returned an error: the operation and FFmpeg's message.
    Av(&'static str, String),
    /// No H.264 encoder could be opened: each one tried, with why it failed.
    NoEncoder(String),
    /// The resampler could not be set up for this rate.
    Resampler(String),
    /// A frame's buffer does not match its stated size.
    BadFrame { expected: usize, actual: usize },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Vpx(op, msg) => write!(f, "libvpx {op}: {msg}"),
            Self::Opus(op, msg) => write!(f, "libopus {op}: {msg}"),
            Self::Av(op, msg) => write!(f, "libavcodec {op}: {msg}"),
            Self::NoEncoder(tried) => write!(f, "no H.264 encoder could be opened ({tried})"),
            Self::Resampler(msg) => write!(f, "resampler: {msg}"),
            Self::BadFrame { expected, actual } => {
                write!(f, "frame buffer is {actual} bytes, expected {expected}")
            }
        }
    }
}

impl std::error::Error for Error {}

/// Opus and WebRTC audio run at 48 kHz.
pub const OPUS_SAMPLE_RATE: u32 = 48_000;
