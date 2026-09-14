//! Connection health: the `Degraded` state of a session (plan §19.1).
//!
//! The plan leaves the thresholds open. They match the "Poor" link quality badge, so the status
//! and the badge agree. Separate thresholds and durations for entering and leaving keep a link
//! that hovers around one value from flipping every second.

/// Loss (%) or jitter (ms) above these for [`ENTER_SECS`] consecutive seconds: `Degraded`.
pub const DEGRADED_LOSS_PCT: f64 = 3.0;
pub const DEGRADED_JITTER_MS: f64 = 30.0;
/// Loss and jitter below these for [`RECOVER_SECS`] consecutive seconds: `Connected` again.
pub const RECOVERED_LOSS_PCT: f64 = 1.0;
pub const RECOVERED_JITTER_MS: f64 = 15.0;
pub const ENTER_SECS: u32 = 3;
pub const RECOVER_SECS: u32 = 5;

#[derive(Debug, Clone, Default)]
pub struct LinkHealth {
    degraded: bool,
    bad_secs: u32,
    good_secs: u32,
}

impl LinkHealth {
    pub fn is_degraded(&self) -> bool {
        self.degraded
    }

    /// Feed one second of measurements (the worst of the path and the session's routes).
    /// Returns true when the state changed.
    pub fn update(&mut self, loss_pct: f64, jitter_ms: f64) -> bool {
        // NaN compares false everywhere: it counts as neither bad nor good.
        let bad = loss_pct > DEGRADED_LOSS_PCT || jitter_ms > DEGRADED_JITTER_MS;
        let good = loss_pct < RECOVERED_LOSS_PCT && jitter_ms < RECOVERED_JITTER_MS;
        self.bad_secs = if bad {
            self.bad_secs.saturating_add(1)
        } else {
            0
        };
        self.good_secs = if good {
            self.good_secs.saturating_add(1)
        } else {
            0
        };
        let next = if self.degraded {
            self.good_secs < RECOVER_SECS
        } else {
            self.bad_secs >= ENTER_SECS
        };
        let changed = next != self.degraded;
        self.degraded = next;
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enters_after_sustained_trouble_and_recovers_with_hysteresis() {
        let mut h = LinkHealth::default();
        // Short spikes do not count.
        assert!(!h.update(10.0, 0.0));
        assert!(!h.update(10.0, 0.0));
        assert!(!h.update(0.0, 0.0));
        assert!(!h.is_degraded());

        // Three bad seconds in a row (loss or jitter).
        assert!(!h.update(5.0, 0.0));
        assert!(!h.update(0.0, 45.0));
        assert!(h.update(4.0, 0.0));
        assert!(h.is_degraded());

        // In between the thresholds: stays degraded however long it lasts.
        for _ in 0..20 {
            assert!(!h.update(2.0, 20.0));
        }
        assert!(h.is_degraded());

        // Five clean seconds, interrupted once, then five in a row.
        for _ in 0..4 {
            h.update(0.0, 5.0);
        }
        h.update(1.5, 5.0);
        for _ in 0..4 {
            assert!(!h.update(0.2, 5.0));
        }
        assert!(h.update(0.2, 5.0));
        assert!(!h.is_degraded());

        assert!(!h.update(f64::NAN, f64::NAN));
    }
}
