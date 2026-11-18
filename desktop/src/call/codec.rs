//! Safe wrappers around the (c2rust-translated, pure Rust) libopus encoder/decoder.
//! Calls use 48 kHz mono with 20 ms frames, like browsers do by default.

use unsafe_libopus as opus;

pub const SAMPLE_RATE: u32 = 48_000;
/// 20 ms at 48 kHz.
pub const FRAME_SAMPLES: usize = 960;
const OPUS_APPLICATION_VOIP: i32 = 2048;
const MAX_PACKET: usize = 1500;

pub struct Encoder(*mut opus::OpusEncoder);

// SAFETY: the encoder state is plain memory owned exclusively by this value.
unsafe impl Send for Encoder {}

impl Encoder {
    pub fn new() -> Result<Self, String> {
        let mut error = 0;
        // SAFETY: valid arguments; the returned pointer is checked below.
        let state = unsafe { opus::opus_encoder_create(SAMPLE_RATE as i32, 1, OPUS_APPLICATION_VOIP, &mut error) };
        if state.is_null() || error != opus::OPUS_OK {
            return Err(format!("Opus encoder could not be created ({error})"));
        }
        Ok(Self(state))
    }

    /// Encodes exactly one 20 ms frame.
    pub fn encode(&mut self, pcm: &[i16]) -> Result<Vec<u8>, String> {
        assert_eq!(pcm.len(), FRAME_SAMPLES, "Opus frames are 20 ms");
        let mut packet = vec![0u8; MAX_PACKET];
        // SAFETY: buffers are valid for the given lengths; state is live.
        let written = unsafe { opus::opus_encode(self.0, pcm.as_ptr(), FRAME_SAMPLES as i32, packet.as_mut_ptr(), MAX_PACKET as i32) };
        if written < 0 {
            return Err(format!("Opus encoding failed ({written})"));
        }
        packet.truncate(written as usize);
        Ok(packet)
    }
}

impl Drop for Encoder {
    fn drop(&mut self) {
        // SAFETY: created by opus_encoder_create and destroyed exactly once.
        unsafe { opus::opus_encoder_destroy(self.0) }
    }
}

pub struct Decoder(*mut opus::OpusDecoder);

// SAFETY: see Encoder.
unsafe impl Send for Decoder {}

impl Decoder {
    pub fn new() -> Result<Self, String> {
        let mut error = 0;
        // SAFETY: valid arguments; checked below.
        let state = unsafe { opus::opus_decoder_create(SAMPLE_RATE as i32, 1, &mut error) };
        if state.is_null() || error != opus::OPUS_OK {
            return Err(format!("Opus decoder could not be created ({error})"));
        }
        Ok(Self(state))
    }

    /// Decodes one packet (mono, 48 kHz). Stereo packets are downmixed by libopus.
    pub fn decode(&mut self, packet: &[u8]) -> Result<Vec<i16>, String> {
        // Up to 120 ms per packet.
        let mut pcm = vec![0i16; 5760];
        // SAFETY: buffers are valid for the given lengths; state is live.
        let samples = unsafe { opus::opus_decode(self.0, packet.as_ptr(), packet.len() as i32, pcm.as_mut_ptr(), pcm.len() as i32, 0) };
        if samples < 0 {
            return Err(format!("Opus decoding failed ({samples})"));
        }
        pcm.truncate(samples as usize);
        Ok(pcm)
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        // SAFETY: created by opus_decoder_create and destroyed exactly once.
        unsafe { opus::opus_decoder_destroy(self.0) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_a_tone() {
        let mut encoder = Encoder::new().unwrap();
        let mut decoder = Decoder::new().unwrap();
        let mut energy = 0.0f64;
        for frame in 0..25 {
            let pcm: Vec<i16> = (0..FRAME_SAMPLES)
                .map(|i| {
                    let t = (frame * FRAME_SAMPLES + i) as f64 / SAMPLE_RATE as f64;
                    ((t * 440.0 * std::f64::consts::TAU).sin() * 8000.0) as i16
                })
                .collect();
            let packet = encoder.encode(&pcm).unwrap();
            assert!(!packet.is_empty() && packet.len() < 400);
            let decoded = decoder.decode(&packet).unwrap();
            assert_eq!(decoded.len(), FRAME_SAMPLES);
            if frame > 5 {
                energy += decoded.iter().map(|s| (*s as f64).powi(2)).sum::<f64>() / FRAME_SAMPLES as f64;
            }
        }
        let rms = (energy / 19.0).sqrt();
        assert!(rms > 3000.0, "decoded tone should keep its level, rms={rms}");
    }
}
