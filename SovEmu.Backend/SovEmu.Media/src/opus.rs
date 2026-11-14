//! Opus through the system libopus, at 48 kHz stereo.
//!
//! The encoder runs in `OPUS_APPLICATION_RESTRICTED_LOWDELAY` mode: CELT only, which drops
//! 2.5 ms of algorithmic delay and suits game music and effects better than the speech modes.

use std::ffi::{CStr, c_int};
use std::ptr::NonNull;

use crate::{Error, OPUS_SAMPLE_RATE, ffi};

const CHANNELS: c_int = 2;
/// Large enough for any single Opus packet.
const MAX_PACKET: usize = 4000;

fn opus_error(op: &'static str, code: c_int) -> Error {
    // SAFETY: opus_strerror returns a static C string for any value.
    let msg = unsafe { CStr::from_ptr(ffi::opus_strerror(code)) };
    Error::Opus(op, msg.to_string_lossy().into_owned())
}

/// A stereo 48 kHz Opus encoder.
#[derive(Debug)]
pub struct OpusEncoder {
    enc: NonNull<ffi::OpusEncoder>,
    out: Vec<u8>,
}

// SAFETY: the encoder state is only used through `&mut self`.
unsafe impl Send for OpusEncoder {}

impl OpusEncoder {
    pub fn new(bitrate_bps: u32) -> Result<Self, Error> {
        let mut err: c_int = 0;
        // SAFETY: valid arguments; `err` is a valid out-parameter.
        let enc = unsafe {
            ffi::opus_encoder_create(
                OPUS_SAMPLE_RATE as i32,
                CHANNELS,
                ffi::OPUS_APPLICATION_RESTRICTED_LOWDELAY as c_int,
                &mut err,
            )
        };
        let enc = NonNull::new(enc).ok_or_else(|| opus_error("create", err))?;
        let encoder = Self { enc, out: vec![0; MAX_PACKET] };
        // SAFETY: OPUS_SET_BITRATE takes one opus_int32.
        let rc = unsafe {
            ffi::opus_encoder_ctl(
                encoder.enc.as_ptr(),
                ffi::OPUS_SET_BITRATE_REQUEST as c_int,
                bitrate_bps as i32,
            )
        };
        if rc != ffi::OPUS_OK as c_int {
            return Err(opus_error("set bitrate", rc));
        }
        Ok(encoder)
    }

    /// Encodes one frame of interleaved stereo samples. The frame must be a duration Opus
    /// supports: 2.5, 5, 10, 20, 40 or 60 ms (120 to 2880 samples per channel at 48 kHz).
    pub fn encode(&mut self, pcm: &[i16]) -> Result<Vec<u8>, Error> {
        let frame_size = (pcm.len() / CHANNELS as usize) as c_int;
        // SAFETY: `pcm` holds `frame_size` interleaved stereo frames; `out` holds MAX_PACKET.
        let n = unsafe {
            ffi::opus_encode(
                self.enc.as_ptr(),
                pcm.as_ptr(),
                frame_size,
                self.out.as_mut_ptr(),
                MAX_PACKET as i32,
            )
        };
        if n < 0 {
            return Err(opus_error("encode", n));
        }
        Ok(self.out[..n as usize].to_vec())
    }
}

impl Drop for OpusEncoder {
    fn drop(&mut self) {
        // SAFETY: created in `new`, destroyed once.
        unsafe { ffi::opus_encoder_destroy(self.enc.as_ptr()) };
    }
}

/// A stereo 48 kHz Opus decoder, used by tests.
#[derive(Debug)]
pub struct OpusDecoder {
    dec: NonNull<ffi::OpusDecoder>,
}

// SAFETY: the decoder state is only used through `&mut self`.
unsafe impl Send for OpusDecoder {}

impl OpusDecoder {
    pub fn new() -> Result<Self, Error> {
        let mut err: c_int = 0;
        // SAFETY: valid arguments; `err` is a valid out-parameter.
        let dec = unsafe { ffi::opus_decoder_create(OPUS_SAMPLE_RATE as i32, CHANNELS, &mut err) };
        Ok(Self { dec: NonNull::new(dec).ok_or_else(|| opus_error("create decoder", err))? })
    }

    /// Decodes one packet into interleaved stereo samples.
    pub fn decode(&mut self, packet: &[u8]) -> Result<Vec<i16>, Error> {
        let mut pcm = vec![0i16; 5760 * CHANNELS as usize];
        // SAFETY: `packet` is readable; `pcm` holds 5760 stereo frames (120 ms, the maximum).
        let n = unsafe {
            ffi::opus_decode(
                self.dec.as_ptr(),
                packet.as_ptr(),
                packet.len() as i32,
                pcm.as_mut_ptr(),
                5760,
                0,
            )
        };
        if n < 0 {
            return Err(opus_error("decode", n));
        }
        pcm.truncate(n as usize * CHANNELS as usize);
        Ok(pcm)
    }
}

impl Drop for OpusDecoder {
    fn drop(&mut self) {
        // SAFETY: created in `new`, destroyed once.
        unsafe { ffi::opus_decoder_destroy(self.dec.as_ptr()) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_a_10ms_frame() {
        let mut enc = OpusEncoder::new(128_000).unwrap();
        let mut dec = OpusDecoder::new().unwrap();
        let pcm: Vec<i16> = (0..480)
            .flat_map(|i| {
                let v =
                    ((i as f32 * 440.0 / 48_000.0 * std::f32::consts::TAU).sin() * 8000.0) as i16;
                [v, v]
            })
            .collect();
        for _ in 0..5 {
            let packet = enc.encode(&pcm).unwrap();
            assert!(!packet.is_empty() && packet.len() < 400, "{} bytes", packet.len());
            assert_eq!(dec.decode(&packet).unwrap().len(), 960);
        }
    }

    #[test]
    fn rejects_an_invalid_frame_size() {
        let mut enc = OpusEncoder::new(128_000).unwrap();
        assert!(matches!(enc.encode(&[0; 2 * 100]), Err(Error::Opus("encode", _))));
    }
}
