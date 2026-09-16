//! Audio cues ("Play a sound on connect and disconnect", plan §7 U9): short, quiet tones
//! generated in code, so a screen-reader user hears what changed without looking.
//!
//! Cues play on the default output through the same audio backend as streams. Each cue opens
//! and closes its own stream on a short-lived thread; nothing is kept open between cues.

use std::sync::Arc;
use std::time::Duration;

use sp_audio_io::cpal_backend::CpalBackend;
use sp_audio_io::{AudioBackend, RenderTarget};
use sp_engine::EngineState;
use sp_engine::state::RouteStatus;

const RATE: f32 = 48_000.0;
/// Quiet on purpose: about -20 dBFS.
const LEVEL: f32 = 0.1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cue {
    Connected,
    Disconnected,
    StreamStarted,
    StreamStopped,
    Muted,
    Unmuted,
    /// "Test tone" on the Audio page (plan §5.1): long enough to hear which device is playing,
    /// short enough not to startle anyone.
    Test,
}

impl Cue {
    /// Notes as (frequency Hz, duration ms). Rising for "on", falling for "off".
    fn notes(self) -> &'static [(f32, u32)] {
        match self {
            Self::Connected => &[(660.0, 70), (880.0, 90)],
            Self::Disconnected => &[(880.0, 70), (660.0, 90)],
            Self::StreamStarted => &[(990.0, 60)],
            Self::StreamStopped => &[(550.0, 60)],
            Self::Muted => &[(440.0, 50)],
            Self::Unmuted => &[(740.0, 50)],
            Self::Test => &[(440.0, 250), (554.37, 250), (659.25, 250), (880.0, 300)],
        }
    }
}

/// Mono samples for a cue, with a short fade on each note so there are no clicks.
fn render(cue: Cue) -> Vec<f32> {
    let mut out = Vec::new();
    for &(freq, ms) in cue.notes() {
        let len = (RATE * ms as f32 / 1000.0) as usize;
        let fade = (RATE * 0.008) as usize;
        for i in 0..len {
            let envelope = (i.min(len - i) as f32 / fade as f32).min(1.0);
            let phase = 2.0 * std::f32::consts::PI * freq * i as f32 / RATE;
            out.push(phase.sin() * LEVEL * envelope);
        }
    }
    out
}

/// The cues a state change should play, in order.
pub fn changes(previous: &EngineState, current: &EngineState) -> Vec<Cue> {
    let connected = |s: &EngineState| {
        s.peers
            .iter()
            .filter(|p| p.trusted && p.connection.is_connected())
            .count()
    };
    let active = |s: &EngineState| {
        s.routes
            .iter()
            .filter(|r| r.status == RouteStatus::Active)
            .count()
    };
    let mut cues = Vec::new();
    match connected(current).cmp(&connected(previous)) {
        std::cmp::Ordering::Greater => cues.push(Cue::Connected),
        std::cmp::Ordering::Less => cues.push(Cue::Disconnected),
        std::cmp::Ordering::Equal => {}
    }
    match active(current).cmp(&active(previous)) {
        std::cmp::Ordering::Greater => cues.push(Cue::StreamStarted),
        // A disconnect already has its own cue.
        std::cmp::Ordering::Less if cues.is_empty() => cues.push(Cue::StreamStopped),
        _ => {}
    }
    if current.mic_muted != previous.mic_muted {
        cues.push(if current.mic_muted {
            Cue::Muted
        } else {
            Cue::Unmuted
        });
    }
    cues
}

/// Play cues one after another on the default output, without blocking the caller.
pub fn play(backend: Arc<CpalBackend>, cues: Vec<Cue>) {
    play_on(backend, cues, RenderTarget::DefaultOutput);
}

/// Like [`play`], on a chosen device. The Audio page's test tone uses the output the user picked,
/// so the tone proves which device streams will actually come out of.
pub fn play_on(backend: Arc<CpalBackend>, cues: Vec<Cue>, target: RenderTarget) {
    if cues.is_empty() {
        return;
    }
    let _ = std::thread::Builder::new()
        .name("sp-cue".into())
        .spawn(move || {
            for cue in cues {
                let samples = render(cue);
                let duration =
                    Duration::from_millis(samples.len() as u64 * 1000 / RATE as u64 + 80);
                let mut position = 0;
                let stream = backend.open_render(
                    &target,
                    1,
                    Box::new(move |out: &mut [f32]| {
                        for s in out.iter_mut() {
                            *s = samples.get(position).copied().unwrap_or(0.0);
                            position += 1;
                        }
                    }),
                    Box::new(|_| {}),
                );
                match stream {
                    Ok(stream) => {
                        std::thread::sleep(duration);
                        drop(stream);
                    }
                    Err(e) => {
                        tracing::debug!(error = %e, "could not play audio cue");
                        return;
                    }
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cues_are_short_quiet_and_click_free() {
        for cue in [
            Cue::Connected,
            Cue::Disconnected,
            Cue::StreamStarted,
            Cue::StreamStopped,
            Cue::Muted,
            Cue::Unmuted,
        ] {
            let s = render(cue);
            assert!(!s.is_empty() && s.len() < RATE as usize / 4);
            assert!(s.iter().all(|v| v.abs() <= LEVEL));
            assert!(s[0].abs() < 0.01 && s[s.len() - 1].abs() < 0.02);
        }
    }

    #[test]
    fn the_test_tone_is_audible_but_short() {
        let s = render(Cue::Test);
        // Around a second: long enough to find the speaker, short enough not to annoy.
        assert!(s.len() > RATE as usize / 2 && s.len() < RATE as usize * 2);
        assert!(s.iter().all(|v| v.abs() <= LEVEL));
        assert!(s[0].abs() < 0.01 && s[s.len() - 1].abs() < 0.02);
        assert!(s.iter().any(|v| v.abs() > LEVEL / 2.0), "it makes a sound");
    }

    #[test]
    fn mute_change_plays_a_cue() {
        let before = EngineState::default();
        let after = EngineState {
            mic_muted: true,
            ..EngineState::default()
        };
        assert_eq!(changes(&before, &after), vec![Cue::Muted]);
        assert!(changes(&after, &after).is_empty());
    }
}
