//! Reconnection policy: exponential backoff with full jitter, a fast phase, then
//! low-frequency probing that is cut short by network-change events.

use std::time::Duration;

use rand::Rng;

#[derive(Debug, Clone)]
pub struct Backoff {
    attempt: u32,
    base: Duration,
    cap: Duration,
    fast_attempts: u32,
    slow_interval: Duration,
}

impl Default for Backoff {
    fn default() -> Self {
        Self {
            attempt: 0,
            base: Duration::from_millis(500),
            cap: Duration::from_secs(8),
            fast_attempts: 10,
            slow_interval: Duration::from_secs(30),
        }
    }
}

impl Backoff {
    /// Delay before the next attempt.
    pub fn next_delay(&mut self) -> Duration {
        self.attempt = self.attempt.saturating_add(1);
        if self.attempt > self.fast_attempts {
            return self.slow_interval;
        }
        let exp = self.base.saturating_mul(1 << (self.attempt - 1).min(16));
        let max = exp.min(self.cap);
        let millis =
            rand::thread_rng().gen_range(max.as_millis() as u64 / 2..=max.as_millis() as u64);
        Duration::from_millis(millis)
    }

    /// True once the fast phase is over ("Waiting for device").
    pub fn in_slow_phase(&self) -> bool {
        self.attempt >= self.fast_attempts
    }

    /// Call on success or when a network change makes an immediate retry worthwhile.
    pub fn reset(&mut self) {
        self.attempt = 0;
    }
}

/// Detects connection flapping (too many reconnects in a short window).
#[derive(Debug, Default)]
pub struct FlapDetector {
    events: Vec<std::time::Instant>,
}

impl FlapDetector {
    pub const WINDOW: Duration = Duration::from_secs(120);
    pub const LIMIT: usize = 5;

    pub fn record(&mut self, now: std::time::Instant) -> bool {
        self.events
            .retain(|t| now.duration_since(*t) < Self::WINDOW);
        self.events.push(now);
        self.events.len() > Self::LIMIT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_grows_caps_and_slows() {
        let mut b = Backoff::default();
        let first = b.next_delay();
        assert!(first <= Duration::from_millis(500));
        for _ in 0..8 {
            assert!(b.next_delay() <= Duration::from_secs(8));
        }
        b.next_delay();
        assert!(b.in_slow_phase());
        assert_eq!(b.next_delay(), Duration::from_secs(30));
        b.reset();
        assert!(!b.in_slow_phase());
    }

    #[test]
    fn flapping_detected() {
        let mut f = FlapDetector::default();
        let t = std::time::Instant::now();
        for i in 0..5 {
            assert!(!f.record(t + Duration::from_secs(i)));
        }
        assert!(f.record(t + Duration::from_secs(6)));
    }
}
