//! Local microphone monitoring ("Listen to my mic").
//!
//! Captures the microphone and plays it on this device with minimal buffering.
//!
//! The captured audio also goes through a feedback detector: when the monitor plays on speakers
//! rather than headphones, the loop can start to howl, and the engine then warns the user and
//! suggests headphones or Headset mode (plan §8.2).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use sp_audio_io::{AudioBackend, AudioStream, CaptureSource, RenderTarget};
use sp_media::dsp::{FeedbackDetector, Gain, db_to_gain};

use super::controls::AtomicF32;
use crate::EngineError;

pub struct MicMonitor {
    _capture: Box<dyn AudioStream>,
    _render: Box<dyn AudioStream>,
    pub gain_db: Arc<AtomicF32>,
    /// True while the monitor is howling; the engine warns once and then clears it.
    feedback: Arc<AtomicBool>,
}

impl MicMonitor {
    pub fn start(
        backend: &dyn AudioBackend,
        source: CaptureSource,
        target: RenderTarget,
        gain_db: f32,
    ) -> Result<Self, EngineError> {
        // ~40 ms of mono audio: enough to absorb callback scheduling, small enough to stay responsive.
        let (mut producer, mut consumer) = rtrb::RingBuffer::<f32>::new(1920);
        let gain_ctl = Arc::new(AtomicF32::new(gain_db));
        let feedback = Arc::new(AtomicBool::new(false));

        let heard = feedback.clone();
        let mut detector = FeedbackDetector::new();
        let capture = backend.open_capture(
            &source,
            1,
            Box::new(move |samples: &[f32]| {
                if detector.process(samples) {
                    heard.store(true, Ordering::Relaxed);
                }
                for s in samples {
                    if producer.push(*s).is_err() {
                        break;
                    }
                }
            }),
            Box::new(|_| {}),
        )?;

        let render_gain = gain_ctl.clone();
        let mut gain = Gain::new(db_to_gain(gain_db));
        let render = backend.open_render(
            &target,
            1,
            Box::new(move |out: &mut [f32]| {
                // Keep latency bounded: skip ahead if too much is queued.
                while consumer.slots() > out.len() * 2 {
                    let _ = consumer.pop();
                }
                for s in out.iter_mut() {
                    *s = consumer.pop().unwrap_or(0.0);
                }
                gain.set(db_to_gain(render_gain.get()));
                gain.process(out);
            }),
            Box::new(|_| {}),
        )?;

        Ok(Self {
            _capture: capture,
            _render: render,
            gain_db: gain_ctl,
            feedback,
        })
    }

    /// Whether a feedback loop has been heard since this was last read; reading clears it, so the
    /// user is warned once per loop rather than on every tick.
    pub fn take_feedback(&self) -> bool {
        self.feedback.swap(false, Ordering::Relaxed)
    }
}
