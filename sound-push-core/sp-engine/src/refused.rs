//! Inbound handshakes that never finished, counted per address (plan §28.2, §30.2).
//!
//! A device whose trust store no longer holds this one's key refuses the certificate and retries
//! on its own reconnect timer. Nothing about that reaches the session layer — there is no session
//! — so without this the user sees a phone that never connects and finds nothing anywhere saying
//! why. A few tries from one address is the signal worth acting on; a single one is ordinary
//! noise (a port scan, a device shutting down mid-handshake).

use std::collections::HashMap;
use std::net::IpAddr;
use std::time::{Duration, Instant};

/// Failures from one address before it is worth telling the user.
pub const BEFORE_NOTICE: u32 = 3;
/// How long a run is counted. A device that goes quiet for this long starts a fresh run, so the
/// user hears again if it comes back — but not while it is simply still retrying.
pub const WINDOW: Duration = Duration::from_secs(600);
/// Addresses tracked at once; more than any home network has, and stale ones are dropped anyway.
const MAX_SOURCES: usize = 64;

#[derive(Debug, Default)]
pub struct RefusedHandshakes {
    sources: HashMap<IpAddr, (Instant, u32)>,
}

impl RefusedHandshakes {
    /// Count a failure from `ip`. True exactly once per run, on the failure that makes it a
    /// pattern, so the security log and the user get one report rather than one per retry.
    pub fn record(&mut self, ip: IpAddr, now: Instant) -> bool {
        let ip = ip.to_canonical();
        self.sources
            .retain(|_, (at, _)| now.saturating_duration_since(*at) < WINDOW);
        if !self.sources.contains_key(&ip) && self.sources.len() >= MAX_SOURCES {
            return false;
        }
        let (_, count) = self.sources.entry(ip).or_insert((now, 0));
        *count += 1;
        *count == BEFORE_NOTICE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(last: u8) -> IpAddr {
        IpAddr::from([192, 168, 68, last])
    }

    #[test]
    fn one_failure_is_not_worth_reporting_but_a_run_is() {
        let mut refused = RefusedHandshakes::default();
        let now = Instant::now();
        assert!(!refused.record(ip(100), now), "a single failure is noise");
        assert!(!refused.record(ip(100), now));
        assert!(refused.record(ip(100), now), "the third makes it a pattern");
        // Still trying: counted, but the user has already been told.
        for _ in 0..10 {
            assert!(!refused.record(ip(100), now));
        }
    }

    #[test]
    fn each_address_is_counted_on_its_own() {
        let mut refused = RefusedHandshakes::default();
        let now = Instant::now();
        for _ in 0..BEFORE_NOTICE - 1 {
            assert!(!refused.record(ip(100), now));
            assert!(!refused.record(ip(101), now));
        }
        assert!(refused.record(ip(100), now));
        assert!(refused.record(ip(101), now));
    }

    #[test]
    fn a_device_that_went_away_and_came_back_is_reported_again() {
        let mut refused = RefusedHandshakes::default();
        let start = Instant::now();
        for _ in 0..BEFORE_NOTICE {
            refused.record(ip(100), start);
        }
        let later = start + WINDOW + Duration::from_secs(1);
        assert!(!refused.record(ip(100), later), "a new run starts over");
        assert!(!refused.record(ip(100), later));
        assert!(refused.record(ip(100), later));
    }

    #[test]
    fn the_same_address_written_two_ways_is_one_address() {
        let mut refused = RefusedHandshakes::default();
        let now = Instant::now();
        // What an IPv4 peer looks like on the dual-stack socket, and on its own.
        let mapped: IpAddr = "::ffff:192.168.68.100".parse().unwrap();
        assert!(!refused.record(mapped, now));
        assert!(!refused.record(ip(100), now));
        assert!(refused.record(mapped, now));
    }
}
