//! From a core's audio to Opus packets.
//!
//! A core produces 16-bit interleaved stereo at its own rate (NES ≈ 48 000 or 44 100, SNES
//! 32 040.5 Hz, …) in frame-sized bursts. [`AudioEncoder`] resamples that to 48 kHz with a
//! polynomial resampler (cheap, and fine for console audio) and cuts it into 10 ms Opus packets.
//! The resampler is the asynchronous kind, so a later phase can nudge its ratio to track clock
//! drift between the emulated and real time.

use audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Async, FixedAsync, PolynomialDegree, Resampler};

use crate::opus::OpusEncoder;
use crate::{Error, OPUS_SAMPLE_RATE};

const CHANNELS: usize = 2;
/// Samples per channel in one Opus packet: 10 ms at 48 kHz.
pub const PACKET_FRAMES: usize = 480;
/// Input frames per resampler call.
const CHUNK: usize = 256;

/// Resamples a core's audio to 48 kHz and encodes it as 10 ms Opus packets.
#[derive(Debug)]
pub struct AudioEncoder {
    input_rate: f64,
    resampler: Option<Box<Async<f32>>>,
    /// Interleaved input not yet resampled.
    pending_in: Vec<f32>,
    /// Interleaved 48 kHz output not yet encoded.
    pending_out: Vec<i16>,
    resampled: Vec<f32>,
    opus: OpusEncoder,
}

impl AudioEncoder {
    /// `input_rate` is the core's sample rate in Hz.
    pub fn new(input_rate: f64, bitrate_bps: u32) -> Result<Self, Error> {
        let mut encoder = Self {
            input_rate: 0.0,
            resampler: None,
            pending_in: Vec::new(),
            pending_out: Vec::new(),
            resampled: Vec::new(),
            opus: OpusEncoder::new(bitrate_bps)?,
        };
        encoder.set_input_rate(input_rate)?;
        Ok(encoder)
    }

    /// The core's sample rate this encoder expects.
    pub fn input_rate(&self) -> f64 {
        self.input_rate
    }

    /// Changes the input rate (the core changed its timing). Buffered audio is dropped.
    pub fn set_input_rate(&mut self, input_rate: f64) -> Result<(), Error> {
        if !(1_000.0..=384_000.0).contains(&input_rate) {
            return Err(Error::Resampler(format!("unsupported sample rate {input_rate}")));
        }
        self.input_rate = input_rate;
        self.pending_in.clear();
        self.pending_out.clear();
        self.resampler = if (input_rate - OPUS_SAMPLE_RATE as f64).abs() < 0.5 {
            None
        } else {
            let resampler = Async::<f32>::new_poly(
                OPUS_SAMPLE_RATE as f64 / input_rate,
                1.1,
                PolynomialDegree::Quintic,
                CHUNK,
                CHANNELS,
                FixedAsync::Input,
            )
            .map_err(|e| Error::Resampler(e.to_string()))?;
            self.resampled = vec![0.0; resampler.output_frames_max() * CHANNELS];
            Some(Box::new(resampler))
        };
        Ok(())
    }

    /// Feeds interleaved stereo samples at the input rate and returns every Opus packet that
    /// is now complete, oldest first. Each packet is [`PACKET_FRAMES`] samples per channel.
    pub fn push(&mut self, samples: &[i16]) -> Result<Vec<Vec<u8>>, Error> {
        match &mut self.resampler {
            None => self.pending_out.extend_from_slice(samples),
            Some(resampler) => {
                self.pending_in.extend(samples.iter().map(|&s| s as f32 / 32_768.0));
                let mut consumed = 0;
                loop {
                    let needed = resampler.input_frames_next();
                    let available = (self.pending_in.len() - consumed) / CHANNELS;
                    if available < needed {
                        break;
                    }
                    let input = InterleavedSlice::new(
                        &self.pending_in[consumed..consumed + needed * CHANNELS],
                        CHANNELS,
                        needed,
                    )
                    .map_err(|e| Error::Resampler(e.to_string()))?;
                    let capacity = self.resampled.len() / CHANNELS;
                    let mut output =
                        InterleavedSlice::new_mut(&mut self.resampled, CHANNELS, capacity)
                            .map_err(|e| Error::Resampler(e.to_string()))?;
                    let (read, written) = resampler
                        .process_into_buffer(&input, &mut output, None)
                        .map_err(|e| Error::Resampler(e.to_string()))?;
                    consumed += read * CHANNELS;
                    self.pending_out.extend(
                        self.resampled[..written * CHANNELS]
                            .iter()
                            .map(|&s| (s * 32_768.0).clamp(-32_768.0, 32_767.0) as i16),
                    );
                }
                self.pending_in.drain(..consumed);
            }
        }

        let packet_len = PACKET_FRAMES * CHANNELS;
        let mut packets = Vec::with_capacity(self.pending_out.len() / packet_len);
        let mut start = 0;
        while self.pending_out.len() - start >= packet_len {
            packets.push(self.opus.encode(&self.pending_out[start..start + packet_len])?);
            start += packet_len;
        }
        self.pending_out.drain(..start);
        Ok(packets)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::opus::OpusDecoder;

    fn tone(rate: f64, frames: usize, offset: usize) -> Vec<i16> {
        (offset..offset + frames)
            .flat_map(|i| {
                let v = ((i as f64 * 440.0 / rate * std::f64::consts::TAU).sin() * 8000.0) as i16;
                [v, v]
            })
            .collect()
    }

    /// Feeds `seconds` of audio at `rate` in 60 fps bursts; returns the packets produced.
    fn run(rate: f64, seconds: f64) -> Vec<Vec<u8>> {
        let mut enc = AudioEncoder::new(rate, 128_000).unwrap();
        let mut packets = Vec::new();
        let mut fed = 0usize;
        for frame in 1..=(seconds * 60.0) as usize {
            let target = (frame as f64 * rate / 60.0) as usize;
            packets.extend(enc.push(&tone(rate, target - fed, fed)).unwrap());
            fed = target;
        }
        packets
    }

    #[test]
    fn native_48k_passes_straight_through() {
        // 2 s at 48 kHz = 96 000 frames = 200 packets of 480.
        assert_eq!(run(48_000.0, 2.0).len(), 200);
    }

    #[test]
    fn resamples_snes_and_cd_rates_to_48k() {
        for rate in [32_040.5, 44_100.0] {
            let packets = run(rate, 2.0);
            // Within a packet or two of 200: the resampler holds back a little as delay.
            assert!((197..=200).contains(&packets.len()), "{rate} Hz: {} packets", packets.len());
            let mut dec = OpusDecoder::new().unwrap();
            let pcm = dec.decode(&packets[100]).unwrap();
            assert_eq!(pcm.len(), PACKET_FRAMES * 2);
            assert!(pcm.iter().any(|&s| s.abs() > 1000), "{rate} Hz: tone survived");
        }
    }

    #[test]
    fn rejects_absurd_rates() {
        assert!(AudioEncoder::new(0.0, 128_000).is_err());
        assert!(AudioEncoder::new(f64::NAN, 128_000).is_err());
    }
}
