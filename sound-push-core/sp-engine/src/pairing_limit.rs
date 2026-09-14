//! Pairing rate limit per remote address (plan §21.1: "pairing rate limits (5/min per source)").
//!
//! Every connection from a device that is not paired counts as an attempt, whether or not it goes
//! on to send a pairing request. Over the limit the connection is closed with `RateLimited`
//! before any pairing work (SAS derivation, prompts) happens.

use std::collections::{HashMap, VecDeque};
use std::net::IpAddr;
use std::time::{Duration, Instant};

pub const ATTEMPTS_PER_WINDOW: usize = 5;
pub const WINDOW: Duration = Duration::from_secs(60);
/// Addresses tracked at once. Beyond this the least recently seen one is forgotten.
const MAX_SOURCES: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Allowed,
    /// Over the limit. `first` is set on the first refusal since the address was last allowed,
    /// so the user is told (and the audit log written) once, not for every retry.
    Refused {
        first: bool,
    },
}

#[derive(Debug)]
struct Source {
    attempts: VecDeque<Instant>,
    refused_reported: bool,
    last_seen: Instant,
}

#[derive(Debug, Default)]
pub struct PairingLimiter {
    sources: HashMap<IpAddr, Source>,
}

impl PairingLimiter {
    /// Count an attempt from `ip`. Refused attempts do not count against the window.
    pub fn check(&mut self, ip: IpAddr, now: Instant) -> Decision {
        let ip = canonical(ip);
        if !self.sources.contains_key(&ip) && self.sources.len() >= MAX_SOURCES {
            self.sources.retain(|_, s| {
                s.attempts
                    .back()
                    .is_some_and(|t| now.saturating_duration_since(*t) < WINDOW)
            });
            if self.sources.len() >= MAX_SOURCES {
                if let Some(oldest) = self
                    .sources
                    .iter()
                    .min_by_key(|(_, s)| s.last_seen)
                    .map(|(ip, _)| *ip)
                {
                    self.sources.remove(&oldest);
                }
            }
        }
        let source = self.sources.entry(ip).or_insert_with(|| Source {
            attempts: VecDeque::with_capacity(ATTEMPTS_PER_WINDOW),
            refused_reported: false,
            last_seen: now,
        });
        source.last_seen = now;
        while source
            .attempts
            .front()
            .is_some_and(|t| now.saturating_duration_since(*t) >= WINDOW)
        {
            source.attempts.pop_front();
        }
        if source.attempts.len() >= ATTEMPTS_PER_WINDOW {
            let first = !source.refused_reported;
            source.refused_reported = true;
            return Decision::Refused { first };
        }
        source.refused_reported = false;
        source.attempts.push_back(now);
        Decision::Allowed
    }
}

/// One key per host: an IPv4 peer reached through a dual-stack socket appears IPv4-mapped.
fn canonical(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(IpAddr::V6(v6), IpAddr::V4),
        v4 => v4,
    }
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, Ipv6Addr};

    use super::*;

    #[test]
    fn five_per_minute_per_address() {
        let mut limiter = PairingLimiter::default();
        let t = Instant::now();
        let a = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 20));
        let b = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 21));
        for i in 0..5 {
            assert_eq!(
                limiter.check(a, t + Duration::from_secs(i)),
                Decision::Allowed
            );
        }
        assert_eq!(
            limiter.check(a, t + Duration::from_secs(10)),
            Decision::Refused { first: true }
        );
        // The same host through a dual-stack socket is the same source.
        let mapped = IpAddr::V6(Ipv4Addr::new(192, 168, 1, 20).to_ipv6_mapped());
        assert_eq!(
            limiter.check(mapped, t + Duration::from_secs(11)),
            Decision::Refused { first: false }
        );
        assert_eq!(
            limiter.check(b, t + Duration::from_secs(11)),
            Decision::Allowed
        );
        // A minute after the first attempt, one slot is free again.
        assert_eq!(
            limiter.check(a, t + Duration::from_secs(60)),
            Decision::Allowed
        );
        assert_eq!(
            limiter.check(a, t + Duration::from_secs(60)),
            Decision::Refused { first: true }
        );
    }

    #[test]
    fn tracked_addresses_stay_bounded() {
        let mut limiter = PairingLimiter::default();
        let t = Instant::now();
        for i in 0..2000u32 {
            let ip = IpAddr::V6(Ipv6Addr::from(u128::from(i) + 1));
            assert_eq!(limiter.check(ip, t), Decision::Allowed);
        }
        assert!(limiter.sources.len() <= MAX_SOURCES);
    }
}
