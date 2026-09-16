//! Supervision of microphone noise suppression (plan §8.3).
//!
//! RNNoise is the most expensive stage in the capture chain. When the machine stops keeping up —
//! a game pinning every core, a laptop throttling — the capture ring overruns and the stream
//! stutters. Rather than let that happen, noise suppression switches itself off, the user is told
//! once, and it comes back after a stretch of clean capture. Nothing is written to the settings,
//! so it is on again at the next start as well.

/// Capture overruns within one supervision step that mean the machine cannot keep up. Healthy
/// capture has none; a handful while an audio device changes is not a problem.
const OVERRUN_LIMIT: u64 = 10;
/// Clean steps before a suspension is lifted.
const RECOVERY_STEPS: u32 = 20;

/// What a supervision step decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DenoiseChange {
    /// Noise suppression just switched itself off; tell the user.
    Suspended,
    /// Capture is healthy again; noise suppression is back on.
    Resumed,
}

#[derive(Debug, Default)]
pub(crate) struct DenoiseSupervisor {
    /// Total capture overruns seen at the previous step, once one has been seen.
    overruns: Option<u64>,
    healthy_steps: u32,
    suspended: bool,
}

impl DenoiseSupervisor {
    /// One step (the engine calls this once a second). `overruns` is the running total across
    /// every microphone capture, `enabled` the user's setting. Returns a change worth reporting.
    pub(crate) fn step(&mut self, overruns: u64, enabled: bool) -> Option<DenoiseChange> {
        // Groups come and go and take their counters with them; that is not progress. The first
        // step only records where the count stands.
        let missed = overruns.saturating_sub(self.overruns.replace(overruns).unwrap_or(overruns));
        if !enabled {
            // Nothing to report: the user turned it off, we did not.
            self.reset();
            self.overruns = Some(overruns);
            return None;
        }
        if missed >= OVERRUN_LIMIT {
            self.healthy_steps = 0;
            return (!std::mem::replace(&mut self.suspended, true))
                .then_some(DenoiseChange::Suspended);
        }
        if !self.suspended {
            return None;
        }
        self.healthy_steps += 1;
        if self.healthy_steps < RECOVERY_STEPS {
            return None;
        }
        self.healthy_steps = 0;
        self.suspended = false;
        Some(DenoiseChange::Resumed)
    }

    pub(crate) fn suspended(&self) -> bool {
        self.suspended
    }

    /// Forget a suspension (the user changed the setting, so the next switch-on must take effect).
    pub(crate) fn reset(&mut self) {
        self.healthy_steps = 0;
        self.suspended = false;
        self.overruns = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suspends_after_overruns_and_recovers_when_capture_is_healthy() {
        let mut s = DenoiseSupervisor::default();
        // A healthy capture never trips it, whatever the starting count.
        assert_eq!(s.step(1_000, true), None);
        for _ in 0..50 {
            assert_eq!(s.step(1_000, true), None);
        }
        assert!(!s.suspended());

        // Overruns pile up: off once, then quiet however long it lasts.
        assert_eq!(s.step(1_020, true), Some(DenoiseChange::Suspended));
        assert!(s.suspended());
        assert_eq!(s.step(1_040, true), None);

        // Clean again, but not for long enough yet.
        for _ in 0..(RECOVERY_STEPS - 1) {
            assert_eq!(s.step(1_040, true), None);
            assert!(s.suspended());
        }
        assert_eq!(s.step(1_040, true), Some(DenoiseChange::Resumed));
        assert!(!s.suspended());

        // A few overruns while a device changes are not enough to trip it.
        assert_eq!(s.step(1_043, true), None);
        assert!(!s.suspended());
    }

    #[test]
    fn a_group_going_away_is_not_progress_and_the_setting_wins() {
        let mut s = DenoiseSupervisor::default();
        assert_eq!(s.step(500, true), None, "the first step only takes stock");
        // The only microphone route stopped, so the total went backwards.
        assert_eq!(s.step(0, true), None);
        assert!(!s.suspended());

        assert_eq!(s.step(100, true), Some(DenoiseChange::Suspended));
        // Switching the setting off clears the suspension without a second notice.
        assert_eq!(s.step(100, false), None);
        assert!(!s.suspended());
        assert_eq!(s.step(100, true), None, "back on because the user said so");
        assert!(!s.suspended());
    }
}
