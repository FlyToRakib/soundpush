//! Session resume tokens (plan §20 "Instant resume").
//!
//! After a session is established each side issues the peer an opaque 256-bit token
//! (`SessionTicket`). The next connection presents it in `Hello.resume_token`. A token is:
//! - bound to the peer's full public key (it is useless on any other authenticated connection),
//! - valid for 10 minutes,
//! - single use: it is removed the first time it is presented, whether or not it matched.
//!
//! Redeeming a token never replaces authentication or trust checks: the connection is already
//! mutually authenticated and the peer is trusted before a token is looked at. It only lets the
//! engine restore routes the user already approved (no second "Ask" prompt) during a short grant.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use bytes::Bytes;
use rand::RngCore;
use sp_security::{DeviceId, Fingerprint};

pub const TOKEN_LEN: usize = 32;
pub const TOKEN_LIFETIME: Duration = Duration::from_secs(600);
/// How long a redeemed token lets paused routes restart without prompting again.
pub const RESUME_GRANT: Duration = Duration::from_secs(30);
/// Bound on stored tokens (one per trusted peer in practice).
const MAX_TOKENS: usize = 64;

struct Issued {
    peer_key: [u8; 32],
    expires: Instant,
}

struct Held {
    token: Bytes,
    expires: Instant,
}

#[derive(Default)]
pub struct ResumeTokens {
    /// Tokens this device issued, by token value.
    issued: HashMap<[u8; TOKEN_LEN], Issued>,
    /// Tokens peers issued to this device, to present on the next connection.
    held: HashMap<DeviceId, Held>,
    /// Peers whose last connection resumed a session, until the grant expires.
    grants: HashMap<DeviceId, Instant>,
}

impl ResumeTokens {
    /// Issue a fresh token for `peer_key`, replacing any earlier one for the same peer.
    pub fn issue(&mut self, peer_key: [u8; 32], now: Instant) -> [u8; TOKEN_LEN] {
        self.prune(now);
        self.issued.retain(|_, i| i.peer_key != peer_key);
        if self.issued.len() >= MAX_TOKENS {
            // Evict the token closest to expiry.
            if let Some(oldest) = self
                .issued
                .iter()
                .min_by_key(|(_, i)| i.expires)
                .map(|(t, _)| *t)
            {
                self.issued.remove(&oldest);
            }
        }
        let mut token = [0u8; TOKEN_LEN];
        rand::rngs::OsRng.fill_bytes(&mut token);
        self.issued.insert(
            token,
            Issued {
                peer_key,
                expires: now + TOKEN_LIFETIME,
            },
        );
        token
    }

    /// Check a token presented by an authenticated peer. Consumes it either way.
    pub fn redeem(&mut self, peer_key: &[u8; 32], token: &[u8], now: Instant) -> bool {
        let Ok(token) = <[u8; TOKEN_LEN]>::try_from(token) else {
            return false;
        };
        let Some(issued) = self.issued.remove(&token) else {
            return false;
        };
        let valid = &issued.peer_key == peer_key && now < issued.expires;
        if valid {
            let id = Fingerprint::of_public_key(peer_key).device_id();
            self.grants.insert(id, now + RESUME_GRANT);
        }
        valid
    }

    /// Store a ticket received from a trusted peer.
    pub fn hold(&mut self, peer: DeviceId, token: Bytes, lifetime_secs: u32, now: Instant) {
        if token.len() != TOKEN_LEN {
            return;
        }
        if !self.held.contains_key(&peer) && self.held.len() >= MAX_TOKENS {
            return;
        }
        let lifetime = Duration::from_secs(lifetime_secs.into()).min(TOKEN_LIFETIME);
        self.held.insert(
            peer,
            Held {
                token,
                expires: now + lifetime,
            },
        );
    }

    /// Token to present to `peer` on a new connection. It stays held until the peer issues a
    /// new one: both directions of a simultaneous dial may present it, and the issuer accepts
    /// it once.
    pub fn held_for(&self, peer: &DeviceId, now: Instant) -> Option<Bytes> {
        self.held
            .get(peer)
            .filter(|h| now < h.expires)
            .map(|h| h.token.clone())
    }

    /// True while `peer`'s last connection resumed its session.
    pub fn resumed_recently(&self, peer: &DeviceId, now: Instant) -> bool {
        self.grants.get(peer).is_some_and(|until| now < *until)
    }

    /// Drop every token and grant for a peer (forgotten, blocked or revoked).
    pub fn forget(&mut self, peer: &DeviceId) {
        self.held.remove(peer);
        self.grants.remove(peer);
        self.issued
            .retain(|_, i| Fingerprint::of_public_key(&i.peer_key).device_id() != *peer);
    }

    pub fn prune(&mut self, now: Instant) {
        self.issued.retain(|_, i| now < i.expires);
        self.held.retain(|_, h| now < h.expires);
        self.grants.retain(|_, until| now < *until);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sp_security::DeviceIdentity;

    #[test]
    fn tokens_are_bound_single_use_and_expire() {
        let alice = DeviceIdentity::generate();
        let mallory = DeviceIdentity::generate();
        let now = Instant::now();
        let mut store = ResumeTokens::default();

        let token = store.issue(alice.public_key(), now);
        // Another authenticated device cannot use it, and the attempt burns it.
        assert!(!store.redeem(&mallory.public_key(), &token, now));
        assert!(!store.redeem(&alice.public_key(), &token, now));

        let token = store.issue(alice.public_key(), now);
        assert!(store.redeem(&alice.public_key(), &token, now));
        assert!(store.resumed_recently(&alice.device_id(), now));
        assert!(
            !store.redeem(&alice.public_key(), &token, now),
            "single use"
        );
        assert!(!store.resumed_recently(&alice.device_id(), now + RESUME_GRANT));

        let token = store.issue(alice.public_key(), now);
        assert!(!store.redeem(&alice.public_key(), &token, now + TOKEN_LIFETIME));

        // Re-issuing replaces the previous token for the same peer.
        let first = store.issue(alice.public_key(), now);
        let second = store.issue(alice.public_key(), now);
        assert!(!store.redeem(&alice.public_key(), &first, now));
        assert!(store.redeem(&alice.public_key(), &second, now));
        assert!(!store.redeem(&alice.public_key(), &[1, 2, 3], now));
    }

    #[test]
    fn held_tokens_are_validated_bounded_and_forgotten() {
        let peer = DeviceIdentity::generate().device_id();
        let now = Instant::now();
        let mut store = ResumeTokens::default();
        store.hold(peer, Bytes::from_static(&[1; 5]), 600, now);
        assert!(store.held_for(&peer, now).is_none(), "wrong length ignored");
        store.hold(peer, Bytes::from_static(&[1; TOKEN_LEN]), u32::MAX, now);
        assert!(store.held_for(&peer, now).is_some());
        assert!(
            store.held_for(&peer, now + TOKEN_LIFETIME).is_none(),
            "lifetime capped"
        );
        for _ in 0..200 {
            store.hold(
                DeviceIdentity::generate().device_id(),
                Bytes::from_static(&[2; TOKEN_LEN]),
                60,
                now,
            );
            store.issue(DeviceIdentity::generate().public_key(), now);
        }
        assert!(store.held.len() <= MAX_TOKENS && store.issued.len() <= MAX_TOKENS);
        store.forget(&peer);
        assert!(store.held_for(&peer, now).is_none());
    }
}
