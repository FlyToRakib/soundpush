//! Per-device stream profiles applied to running routes, and `RouteUpdate`.
//!
//! Negotiation rules (unchanged from route start): whoever requested a route proposes its codec,
//! bitrate, frame size and redundancy; the receiving device always keeps its own buffer bounds.
//! A profile change therefore updates latency on routes this device receives, and quality on
//! routes this device started. Bitrate and redundancy change live; a codec, channel or frame
//! change rebuilds the pipelines on both sides (`FEATURE_ROUTE_RECONFIGURE`), or restarts the
//! route when the peer is a 1.0 build.

use super::*;

fn encoding_changed(a: &StreamProfile, b: &StreamProfile) -> bool {
    a.codec != b.codec || a.channels != b.channels || a.frame_us != b.frame_us
}

fn quality_changed(a: &StreamProfile, b: &StreamProfile) -> bool {
    encoding_changed(a, b)
        || a.bitrate != b.bitrate
        || a.redundancy != b.redundancy
        || a.adaptive_bitrate != b.adaptive_bitrate
}

impl Actor {
    /// Re-apply stream settings to routes whose effective settings changed (global or per device).
    pub(super) fn reconfigure_routes(&mut self, old: &Settings) {
        let changed: Vec<String> = self
            .routes
            .iter()
            .filter(|r| r.status == RouteStatus::Active)
            .filter(|r| {
                let peer = r.peer.to_hex();
                old.stream_for(&peer) != self.settings.stream_for(&peer)
            })
            .map(Route::key)
            .collect();
        for key in changed {
            self.reconfigure_route(&key);
        }
    }

    pub(super) fn reconfigure_route(&mut self, key: &str) {
        let Some(r) = self.routes.iter().find(|r| r.key() == key) else {
            return;
        };
        let (peer, id, kind, requested_locally) = (r.peer, r.id, r.kind, r.requested_locally);
        let Some(current) = r.profile.clone() else {
            return;
        };
        let wanted = self.profile_for(kind, &peer);
        let mut next = current.clone();
        if !kind.local_is_source() {
            next.jitter_min_ms = wanted.jitter_min_ms;
            next.jitter_max_ms = wanted.jitter_max_ms;
        }
        if requested_locally {
            next.codec = wanted.codec;
            next.bitrate = wanted.bitrate;
            next.channels = wanted.channels;
            next.frame_us = wanted.frame_us;
            next.redundancy = wanted.redundancy;
            next.adaptive_bitrate = wanted.adaptive_bitrate;
        }
        sanitize_profile(&mut next);
        if next == current {
            return;
        }
        let peer_reconfigures = self.sessions.get(&peer).is_some_and(|s| {
            Capabilities(s.hello.capabilities).has(Capabilities::FEATURE_ROUTE_RECONFIGURE)
        });
        if encoding_changed(&current, &next) && !peer_reconfigures {
            // A 1.0 peer cannot switch codec or frame size mid-route: start it again instead.
            self.restart_route(key);
            return;
        }
        let send_update = requested_locally && quality_changed(&current, &next);
        self.apply_profile(key, next.clone());
        if send_update {
            if let Some(s) = self.sessions.get(&peer) {
                s.send(Body::RouteUpdate(RouteUpdate {
                    route: id as u32,
                    profile: Some(next),
                }));
            }
        }
    }

    /// Live profile change from the peer.
    pub(super) fn on_route_update(&mut self, peer: DeviceId, update: RouteUpdate) {
        let key = route_key(&peer, update.route as u8);
        let (Some(r), Some(mut next)) =
            (self.routes.iter().find(|r| r.key() == key), update.profile)
        else {
            return;
        };
        let Some(current) = r.profile.clone() else {
            return;
        };
        sanitize_profile(&mut next);
        if !r.kind.local_is_source() {
            // The receiving side's latency preference wins.
            next.jitter_min_ms = current.jitter_min_ms;
            next.jitter_max_ms = current.jitter_max_ms;
        }
        if next != current {
            self.apply_profile(&key, next);
        }
    }

    /// Put a new profile into effect on a route's pipelines.
    fn apply_profile(&mut self, key: &str, next: StreamProfile) {
        let Some(pos) = self.routes.iter().position(|r| r.key() == key) else {
            return;
        };
        let r = &mut self.routes[pos];
        let rebuild = r
            .profile
            .as_ref()
            .is_some_and(|c| encoding_changed(c, &next));
        r.profile = Some(next.clone());
        if let Some(c) = &r.receiver_controls {
            c.jitter_min_ms.store(next.jitter_min_ms, Ordering::Relaxed);
            c.jitter_max_ms.store(next.jitter_max_ms, Ordering::Relaxed);
        }
        if let Some(c) = &r.sender_controls {
            c.bitrate.store(next.bitrate, Ordering::Relaxed);
            c.redundancy.store(next.redundancy, Ordering::Relaxed);
        }
        if !rebuild || r.status != RouteStatus::Active {
            return;
        }
        let mut route = self.routes.remove(pos);
        self.release_pipelines(&mut route);
        let (peer, id) = (route.peer, route.id);
        let started = self.begin_pipelines(&mut route);
        self.routes.push(route);
        if let Err(e) = started {
            self.fail_route(peer, id, e);
        }
    }

    /// Stop a route and request it again with current settings (peers without reconfiguration).
    fn restart_route(&mut self, key: &str) {
        let Some(r) = self.routes.iter().find(|r| r.key() == key) else {
            return;
        };
        let (peer, kind, keep, requested_locally) =
            (r.peer, r.kind, r.keep_running, r.requested_locally);
        // `Superseded`: replaced by a new route, so saved-route entries stay.
        self.stop_route_by_key(key, StopReason::Superseded, true);
        if requested_locally {
            let (tx, _rx) = oneshot::channel();
            self.start_route(peer, kind, tx);
            if let Some(r) = self
                .routes
                .iter_mut()
                .rev()
                .find(|r| r.peer == peer && r.kind == kind)
            {
                r.keep_running = keep;
            }
        }
    }
}
