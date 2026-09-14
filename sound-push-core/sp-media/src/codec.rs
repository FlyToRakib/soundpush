//! Audio codecs: uncompressed PCM and Opus.
//!
//! Opus uses `unsafe-libopus`, a pure-Rust translation of libopus 1.3. It needs
//! no C toolchain or CMake, so it cross-compiles to Android without extra setup.

use sp_protocol::Codec;
use sp_protocol::control::StreamProfile;
use sp_protocol::media::{MAX_ACCEPTED_MEDIA_PAYLOAD, MAX_MEDIA_PAYLOAD};
#[allow(clippy::unsafe_removed_from_name)]
// every call site below is still an explicit `unsafe` block
use unsafe_libopus as opus;

use crate::{MediaError, SAMPLE_RATE, samples_per_frame};

pub trait Encoder: Send {
    fn codec(&self) -> Codec;
    /// Encode exactly one frame of interleaved samples into `out` (cleared first).
    fn encode(&mut self, pcm: &[f32], out: &mut Vec<u8>) -> Result<(), MediaError>;
    fn set_bitrate(&mut self, _bps: u32) {}
    fn set_expected_loss(&mut self, _percent: u8) {}
}

pub trait Decoder: Send {
    /// Decode one packet into `out` (interleaved). Returns samples per channel written.
    fn decode(&mut self, payload: &[u8], out: &mut [f32]) -> Result<usize, MediaError>;
    /// Fill `out` with concealment for a lost frame. Returns samples per channel written.
    fn conceal(&mut self, out: &mut [f32]) -> usize;
}

/// Opus application mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpusApplication {
    /// Lowest delay; best for music, games, system audio.
    LowDelay,
    /// Speech-optimized; used for microphone routes.
    Voip,
}

pub fn encoder_for(
    profile: &StreamProfile,
    app: OpusApplication,
) -> Result<Box<dyn Encoder>, MediaError> {
    let channels = profile.channels.clamp(1, 2) as usize;
    let frame = samples_per_frame(profile.frame_us);
    match Codec::try_from(profile.codec as u8).map_err(|_| MediaError::InvalidParameter("codec"))? {
        Codec::PcmS16Le => Ok(Box::new(PcmEncoder { channels })),
        Codec::Opus => Ok(Box::new(OpusEncoder::new(
            channels,
            frame,
            profile.bitrate,
            app,
        )?)),
    }
}

pub fn decoder_for(profile: &StreamProfile) -> Result<Box<dyn Decoder>, MediaError> {
    let channels = profile.channels.clamp(1, 2) as usize;
    let frame = samples_per_frame(profile.frame_us);
    match Codec::try_from(profile.codec as u8).map_err(|_| MediaError::InvalidParameter("codec"))? {
        Codec::PcmS16Le => Ok(Box::new(PcmDecoder { channels, frame })),
        Codec::Opus => Ok(Box::new(OpusDecoder::new(channels, frame)?)),
    }
}

// ---------------------------------------------------------------- PCM

pub struct PcmEncoder {
    channels: usize,
}

impl Encoder for PcmEncoder {
    fn codec(&self) -> Codec {
        Codec::PcmS16Le
    }

    fn encode(&mut self, pcm: &[f32], out: &mut Vec<u8>) -> Result<(), MediaError> {
        if pcm.len() % self.channels != 0 || pcm.len() * 2 > MAX_MEDIA_PAYLOAD {
            return Err(MediaError::InvalidParameter("pcm frame size"));
        }
        out.clear();
        out.reserve(pcm.len() * 2);
        for s in pcm {
            let v = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
            out.extend_from_slice(&v.to_le_bytes());
        }
        Ok(())
    }
}

pub struct PcmDecoder {
    channels: usize,
    frame: usize,
}

impl Decoder for PcmDecoder {
    fn decode(&mut self, payload: &[u8], out: &mut [f32]) -> Result<usize, MediaError> {
        let samples = payload.len() / 2;
        if payload.len() % 2 != 0 || samples % self.channels != 0 || samples > out.len() {
            return Err(MediaError::WrongPayloadSize {
                got: payload.len(),
                expected: out.len() * 2,
            });
        }
        for (dst, chunk) in out.iter_mut().zip(payload.chunks_exact(2)) {
            *dst = i16::from_le_bytes([chunk[0], chunk[1]]) as f32 / 32768.0;
        }
        Ok(samples / self.channels)
    }

    fn conceal(&mut self, out: &mut [f32]) -> usize {
        // Fade out whatever is in the buffer from the previous frame.
        let n = (self.frame * self.channels).min(out.len());
        for (i, s) in out[..n].iter_mut().enumerate() {
            *s *= 1.0 - i as f32 / n as f32;
        }
        self.frame
    }
}

// ---------------------------------------------------------------- Opus

pub struct OpusEncoder {
    st: *mut opus::OpusEncoder,
    channels: usize,
    frame: usize,
}

// SAFETY: the encoder state is exclusively owned and only used through &mut self.
unsafe impl Send for OpusEncoder {}

impl OpusEncoder {
    pub fn new(
        channels: usize,
        frame: usize,
        bitrate: u32,
        app: OpusApplication,
    ) -> Result<Self, MediaError> {
        let application = match app {
            OpusApplication::LowDelay => opus::OPUS_APPLICATION_RESTRICTED_LOWDELAY,
            OpusApplication::Voip => opus::OPUS_APPLICATION_VOIP,
        };
        let mut err = 0i32;
        // SAFETY: valid sample rate/channel count; `err` outlives the call.
        let st = unsafe {
            opus::opus_encoder_create(SAMPLE_RATE as i32, channels as i32, application, &mut err)
        };
        if st.is_null() || err != opus::OPUS_OK {
            return Err(MediaError::Codec(format!(
                "opus_encoder_create failed ({err})"
            )));
        }
        let mut enc = Self {
            st,
            channels,
            frame,
        };
        enc.set_bitrate(bitrate.max(6_000));
        // SAFETY: `st` is a live encoder.
        unsafe {
            opus::opus_encoder_ctl!(enc.st, opus::OPUS_SET_COMPLEXITY_REQUEST, 8);
        }
        Ok(enc)
    }
}

impl Encoder for OpusEncoder {
    fn codec(&self) -> Codec {
        Codec::Opus
    }

    fn encode(&mut self, pcm: &[f32], out: &mut Vec<u8>) -> Result<(), MediaError> {
        if pcm.len() != self.frame * self.channels {
            return Err(MediaError::InvalidParameter("opus frame size"));
        }
        out.clear();
        out.resize(MAX_MEDIA_PAYLOAD, 0);
        // SAFETY: `pcm` holds frame*channels samples; `out` has MAX_MEDIA_PAYLOAD bytes.
        let n = unsafe {
            opus::opus_encode_float(
                self.st,
                pcm.as_ptr(),
                self.frame as i32,
                out.as_mut_ptr(),
                MAX_MEDIA_PAYLOAD as i32,
            )
        };
        if n < 0 {
            out.clear();
            return Err(MediaError::Codec(format!("opus_encode failed ({n})")));
        }
        out.truncate(n as usize);
        Ok(())
    }

    fn set_bitrate(&mut self, bps: u32) {
        // SAFETY: `st` is a live encoder.
        unsafe {
            opus::opus_encoder_ctl!(
                self.st,
                opus::OPUS_SET_BITRATE_REQUEST,
                bps.clamp(6_000, 510_000) as i32
            );
        }
    }

    fn set_expected_loss(&mut self, percent: u8) {
        // SAFETY: `st` is a live encoder.
        unsafe {
            opus::opus_encoder_ctl!(
                self.st,
                opus::OPUS_SET_PACKET_LOSS_PERC_REQUEST,
                percent.min(100) as i32
            );
            opus::opus_encoder_ctl!(
                self.st,
                opus::OPUS_SET_INBAND_FEC_REQUEST,
                i32::from(percent > 0)
            );
        }
    }
}

impl Drop for OpusEncoder {
    fn drop(&mut self) {
        // SAFETY: created by opus_encoder_create and destroyed exactly once.
        unsafe { opus::opus_encoder_destroy(self.st) }
    }
}

pub struct OpusDecoder {
    st: *mut opus::OpusDecoder,
    channels: usize,
    frame: usize,
}

// SAFETY: the decoder state is exclusively owned and only used through &mut self.
unsafe impl Send for OpusDecoder {}

impl OpusDecoder {
    pub fn new(channels: usize, frame: usize) -> Result<Self, MediaError> {
        let mut err = 0i32;
        // SAFETY: valid sample rate/channel count; `err` outlives the call.
        let st =
            unsafe { opus::opus_decoder_create(SAMPLE_RATE as i32, channels as i32, &mut err) };
        if st.is_null() || err != opus::OPUS_OK {
            return Err(MediaError::Codec(format!(
                "opus_decoder_create failed ({err})"
            )));
        }
        Ok(Self {
            st,
            channels,
            frame,
        })
    }
}

impl Decoder for OpusDecoder {
    fn decode(&mut self, payload: &[u8], out: &mut [f32]) -> Result<usize, MediaError> {
        if payload.is_empty() || payload.len() > MAX_ACCEPTED_MEDIA_PAYLOAD {
            return Err(MediaError::WrongPayloadSize {
                got: payload.len(),
                expected: 1,
            });
        }
        let max_frame = out.len() / self.channels;
        // SAFETY: `payload` is valid for its length; `out` holds max_frame*channels samples.
        let n = unsafe {
            opus::opus_decode_float(
                self.st,
                payload.as_ptr(),
                payload.len() as i32,
                out.as_mut_ptr(),
                max_frame as i32,
                0,
            )
        };
        if n < 0 {
            return Err(MediaError::Codec(format!("opus_decode failed ({n})")));
        }
        Ok(n as usize)
    }

    fn conceal(&mut self, out: &mut [f32]) -> usize {
        let frame = self.frame.min(out.len() / self.channels);
        // SAFETY: a null payload requests packet loss concealment for `frame` samples.
        let n = unsafe {
            opus::opus_decode_float(
                self.st,
                std::ptr::null(),
                0,
                out.as_mut_ptr(),
                frame as i32,
                0,
            )
        };
        if n < 0 {
            out[..frame * self.channels].fill(0.0);
            return frame;
        }
        n as usize
    }
}

impl Drop for OpusDecoder {
    fn drop(&mut self) {
        // SAFETY: created by opus_decoder_create and destroyed exactly once.
        unsafe { opus::opus_decoder_destroy(self.st) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::{LatencyProfile, Quality, build_profile};

    fn sine(frames: usize, channels: usize, freq: f32, offset: usize) -> Vec<f32> {
        (0..frames * channels)
            .map(|i| {
                let n = (i / channels + offset) as f32;
                (2.0 * std::f32::consts::PI * freq * n / SAMPLE_RATE as f32).sin() * 0.5
            })
            .collect()
    }

    #[test]
    fn pcm_roundtrip_is_near_lossless() {
        let profile = build_profile(LatencyProfile::LowLatency, Quality::Lossless, 2, false);
        let frame = samples_per_frame(profile.frame_us);
        let mut enc = encoder_for(&profile, OpusApplication::LowDelay).unwrap();
        let mut dec = decoder_for(&profile).unwrap();
        let input = sine(frame, 2, 440.0, 0);
        let mut packet = Vec::new();
        enc.encode(&input, &mut packet).unwrap();
        let mut out = vec![0.0; frame * 2];
        assert_eq!(dec.decode(&packet, &mut out).unwrap(), frame);
        for (a, b) in input.iter().zip(out.iter()) {
            assert!((a - b).abs() < 1e-4);
        }
    }

    #[test]
    fn opus_encodes_and_decodes_with_low_error() {
        let profile = build_profile(LatencyProfile::Balanced, Quality::Opus(128_000), 2, false);
        let frame = samples_per_frame(profile.frame_us);
        let mut enc = encoder_for(&profile, OpusApplication::LowDelay).unwrap();
        let mut dec = decoder_for(&profile).unwrap();
        let mut packet = Vec::new();
        let mut out = vec![0.0; frame * 2];
        let mut err_energy = 0.0f64;
        let mut sig_energy = 0.0f64;
        // Opus has algorithmic delay; compare against delayed input after warmup.
        let delay = 120; // ≈ 2.5 ms look-ahead at 48 kHz for RESTRICTED_LOWDELAY
        let mut history: Vec<f32> = Vec::new();
        let mut decoded: Vec<f32> = Vec::new();
        for i in 0..50 {
            let input = sine(frame, 2, 440.0, i * frame);
            enc.encode(&input, &mut packet).unwrap();
            assert!(packet.len() < MAX_MEDIA_PAYLOAD && !packet.is_empty());
            let n = dec.decode(&packet, &mut out).unwrap();
            assert_eq!(n, frame);
            history.extend_from_slice(&input);
            decoded.extend_from_slice(&out);
        }
        for i in (20 * frame * 2)..(history.len() - delay * 2) {
            let e = (decoded[i + delay * 2] - history[i]) as f64;
            err_energy += e * e;
            sig_energy += (history[i] as f64).powi(2);
        }
        let snr_db = 10.0 * (sig_energy / err_energy.max(1e-12)).log10();
        assert!(snr_db > 10.0, "SNR {snr_db:.1} dB");
    }

    #[test]
    fn opus_concealment_produces_a_full_frame() {
        let profile = build_profile(LatencyProfile::Balanced, Quality::Auto, 1, false);
        let frame = samples_per_frame(profile.frame_us);
        let mut dec = decoder_for(&profile).unwrap();
        let mut out = vec![0.0; frame];
        assert_eq!(dec.conceal(&mut out), frame);
    }

    #[test]
    fn decoders_reject_bad_payloads() {
        let profile = build_profile(LatencyProfile::Balanced, Quality::Lossless, 1, false);
        let mut dec = decoder_for(&profile).unwrap();
        let mut out = vec![0.0; 10];
        assert!(dec.decode(&[1, 2, 3], &mut out).is_err());
        let opus_profile = build_profile(LatencyProfile::Balanced, Quality::Auto, 1, false);
        let mut odec = decoder_for(&opus_profile).unwrap();
        assert!(odec.decode(&[], &mut out).is_err());
    }
}
