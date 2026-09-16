//! Route pipelines: encoder groups shared by routes (plan §15.2) and opening audio devices off
//! the actor task.
//!
//! Opening a device can block for seconds (WASAPI, Bluetooth, sleeping USB interfaces), so it
//! runs on the blocking pool and reports back through `Internal`. A route is `Starting` until its
//! pipeline is ready; only then is `RouteAccept` sent (accepting side) or the caller answered
//! (requesting side). Every start carries an id, so a result for a route that was stopped or
//! restarted meanwhile is recognized as stale and released.

use super::*;

/// What an encoder group captures.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum SourceKey {
    System(Option<String>),
    /// Per-app capture of this computer's audio (one app, or all but one).
    Application {
        process: String,
        exclude: bool,
    },
    Apps,
    Mic(Option<String>),
}

/// Routes with equal keys share one capture and encoder.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct EncoderKey {
    source: SourceKey,
    codec: u32,
    bitrate: u32,
    channels: u32,
    frame_us: u32,
}

impl EncoderKey {
    fn new(source: &CaptureSource, is_mic: bool, apps: bool, profile: &StreamProfile) -> Self {
        let source = match source {
            _ if apps => SourceKey::Apps,
            CaptureSource::SystemLoopback(device) => SourceKey::System(device.clone()),
            CaptureSource::Application { process, exclude } => SourceKey::Application {
                process: process.clone(),
                exclude: *exclude,
            },
            CaptureSource::Input(device) if is_mic => SourceKey::Mic(Some(device.clone())),
            CaptureSource::Input(device) => SourceKey::System(Some(device.clone())),
            CaptureSource::DefaultInput => SourceKey::Mic(None),
        };
        Self {
            source,
            codec: profile.codec,
            bitrate: profile.bitrate,
            channels: profile.channels,
            frame_us: profile.frame_us,
        }
    }

    pub(crate) fn is_mic(&self) -> bool {
        matches!(self.source, SourceKey::Mic(_))
    }
}

pub(crate) enum EncoderSlot {
    /// The capture is opening; these routes subscribe once it is ready.
    Starting {
        start_id: u64,
        waiting: Vec<(DeviceId, u8)>,
    },
    Running(Sender),
}

/// Drop something whose destructor joins threads (audio streams, encoders) away from the actor.
fn drop_off_actor<T: Send + 'static>(value: T) {
    tokio::task::spawn_blocking(move || drop(value));
}

impl Actor {
    fn next_start_id(&mut self) -> u64 {
        self.next_start_id += 1;
        self.next_start_id
    }

    /// Start a route's pipeline. `Ok(true)` when it is ready immediately (joined a running encoder).
    pub(super) fn begin_pipelines(&mut self, route: &mut Route) -> Result<bool, EngineError> {
        if !self.sessions.contains_key(&route.peer) {
            return Err(EngineError::Unreachable);
        }
        let profile = route
            .profile
            .clone()
            .ok_or_else(|| EngineError::Internal("missing profile".into()))?;
        if route.kind.local_is_source() {
            self.begin_sender(route, profile)
        } else {
            self.begin_receiver(route, profile)
        }
    }

    fn begin_receiver(
        &mut self,
        route: &mut Route,
        profile: StreamProfile,
    ) -> Result<bool, EngineError> {
        let virtual_mic = route.kind.endpoints().1 == "virtual-mic";
        let target = if virtual_mic {
            self.hooks
                .virtual_mic_target(self.settings.desktop.virtual_mic_device.as_deref())
                .ok_or(EngineError::VirtualMicMissing)?
        } else {
            self.speaker_target()
        };
        // A rebuild (codec change) keeps the route's controls, so volume and mute survive it.
        let controls = match &route.receiver_controls {
            Some(c) => {
                c.jitter_min_ms
                    .store(profile.jitter_min_ms, Ordering::Relaxed);
                c.jitter_max_ms
                    .store(profile.jitter_max_ms, Ordering::Relaxed);
                c.clone()
            }
            None => {
                let c = Arc::new(ReceiverControls::new(
                    if route.kind.is_mic() {
                        1.0
                    } else {
                        route.volume
                    },
                    profile.jitter_min_ms,
                    profile.jitter_max_ms,
                ));
                // The microphone mute also silences a phone microphone arriving here.
                c.muted.store(
                    route.muted || (route.kind.is_mic() && self.mic_muted),
                    Ordering::Relaxed,
                );
                if !route.kind.is_mic() {
                    c.balance.set(self.settings.output.balance);
                    c.mono.store(self.settings.output.mono, Ordering::Relaxed);
                    c.av_offset_ms
                        .store(self.settings.output.av_offset_ms, Ordering::Relaxed);
                }
                route.receiver_controls = Some(c.clone());
                c
            }
        };
        // Noise suppression the sending device asked us to run for it (plan §15.7).
        controls
            .noise_suppression
            .store(profile.denoise, Ordering::Relaxed);
        // Only audio going to this device's own speakers can be picked up by its microphone.
        let echo = (!virtual_mic).then(|| self.echo.clone());

        let start_id = self.next_start_id();
        route.start_id = start_id;
        let (peer, id) = (route.peer, route.id);
        let failed_tx = self.internal_tx.clone();
        let on_error = Box::new(move |e: sp_audio_io::AudioError| {
            let _ = failed_tx.send(Internal::AudioFailed {
                peer,
                route: id,
                error: e.into(),
            });
        });
        let backend = self.backend.clone();
        let tx = self.internal_tx.clone();
        tokio::task::spawn_blocking(move || {
            let result = Receiver::start(
                backend.as_ref(),
                ReceiverConfig {
                    profile,
                    target,
                    echo,
                },
                controls,
                on_error,
            );
            let _ = tx.send(Internal::ReceiverReady {
                peer,
                route: id,
                start_id,
                result,
            });
        });
        Ok(false)
    }

    fn begin_sender(
        &mut self,
        route: &mut Route,
        profile: StreamProfile,
    ) -> Result<bool, EngineError> {
        let apps = route.kind.endpoints().0 == "apps";
        let source = match route.kind.endpoints().0 {
            "system" => self.system_audio_source(),
            "apps" => self
                .hooks
                .app_audio_source()
                .ok_or(EngineError::LoopbackUnsupported)?,
            _ => self.mic_source(),
        };
        let is_mic = route.kind.is_mic();
        let key = EncoderKey::new(&source, is_mic, apps, &profile);

        // Per-route controls; the group has its own for gain, noise suppression and mic mute.
        let controls = Arc::new(SenderControls::new(0.0, false, profile.bitrate));
        controls
            .redundancy
            .store(profile.redundancy, Ordering::Relaxed);
        controls
            .muted
            .store(route.muted || (is_mic && self.mic_muted), Ordering::Relaxed);
        route.sender_controls = Some(controls.clone());
        route.encoder = Some(key.clone());
        route.subscription = None;
        if route.kind == RouteKind::SendSystemAudio && self.settings.capture.mute_local_speakers {
            self.hooks.set_speakers_muted(true);
        }

        match self.encoders.get_mut(&key) {
            Some(EncoderSlot::Running(sender)) => {
                let session = self
                    .sessions
                    .get(&route.peer)
                    .ok_or(EngineError::Unreachable)?;
                route.subscription = Some(sender.subscribe(Subscriber {
                    route: route.id,
                    sink: Arc::new(session.conn.clone()),
                    dtx: Capabilities(session.hello.capabilities).has(Capabilities::FEATURE_DTX),
                    controls,
                }));
                Ok(true)
            }
            Some(EncoderSlot::Starting { waiting, .. }) => {
                waiting.push((route.peer, route.id));
                Ok(false)
            }
            None => {
                let start_id = self.next_start_id();
                let group = Arc::new(SenderControls::new(
                    if is_mic {
                        self.settings.mic.gain_db
                    } else {
                        0.0
                    },
                    is_mic && self.denoise_here(&profile),
                    profile.bitrate,
                ));
                group
                    .muted
                    .store(is_mic && self.mic_muted, Ordering::Relaxed);
                group
                    .high_pass
                    .store(is_mic && self.settings.mic.high_pass, Ordering::Relaxed);
                group
                    .echo_ducking
                    .store(is_mic && self.settings.mic.echo_ducking, Ordering::Relaxed);
                // Only a microphone can pick up this device's own speakers.
                let echo = is_mic.then(|| self.echo.clone());
                let failed_tx = self.internal_tx.clone();
                let failed_key = key.clone();
                let on_error = Box::new(move |e: sp_audio_io::AudioError| {
                    let _ = failed_tx.send(Internal::EncoderFailed {
                        key: failed_key.clone(),
                        error: e.into(),
                    });
                });
                self.encoders.insert(
                    key.clone(),
                    EncoderSlot::Starting {
                        start_id,
                        waiting: vec![(route.peer, route.id)],
                    },
                );
                let backend = self.backend.clone();
                let tx = self.internal_tx.clone();
                let application = if is_mic {
                    OpusApplication::Voip
                } else {
                    OpusApplication::LowDelay
                };
                tokio::task::spawn_blocking(move || {
                    let result = Sender::start(
                        backend.as_ref(),
                        SenderConfig {
                            profile,
                            application,
                            source,
                            echo,
                        },
                        group,
                        on_error,
                    );
                    let _ = tx.send(Internal::SenderReady {
                        key,
                        start_id,
                        result,
                    });
                });
                Ok(false)
            }
        }
    }

    pub(super) fn on_sender_ready(
        &mut self,
        key: EncoderKey,
        start_id: u64,
        result: Result<Sender, EngineError>,
    ) {
        let current = matches!(self.encoders.get(&key), Some(EncoderSlot::Starting { start_id: s, .. }) if *s == start_id);
        if !current {
            if let Ok(sender) = result {
                drop_off_actor(sender);
            }
            return;
        }
        let Some(EncoderSlot::Starting { waiting, .. }) = self.encoders.remove(&key) else {
            return;
        };
        match result {
            Ok(sender) => {
                let mut ready = Vec::new();
                for (peer, id) in waiting {
                    let route_id = route_key(&peer, id);
                    let Some(r) = self.routes.iter_mut().find(|r| {
                        r.key() == route_id
                            && r.encoder.as_ref() == Some(&key)
                            && r.subscription.is_none()
                    }) else {
                        continue;
                    };
                    let (Some(session), Some(controls)) =
                        (self.sessions.get(&peer), r.sender_controls.clone())
                    else {
                        continue;
                    };
                    r.subscription =
                        Some(
                            sender.subscribe(Subscriber {
                                route: id,
                                sink: Arc::new(session.conn.clone()),
                                dtx: Capabilities(session.hello.capabilities)
                                    .has(Capabilities::FEATURE_DTX),
                                controls,
                            }),
                        );
                    ready.push((peer, id));
                }
                self.encoders.insert(key, EncoderSlot::Running(sender));
                // Settings may have changed while the device was opening.
                self.update_mic_groups();
                for (peer, id) in ready {
                    self.activate_route(peer, id);
                }
                self.prune_encoders();
            }
            Err(error) => {
                for (peer, id) in waiting {
                    let route_id = route_key(&peer, id);
                    if self
                        .routes
                        .iter()
                        .any(|r| r.key() == route_id && r.encoder.as_ref() == Some(&key))
                    {
                        self.fail_route(peer, id, error.clone());
                    }
                }
            }
        }
    }

    pub(super) fn on_receiver_ready(
        &mut self,
        peer: DeviceId,
        id: u8,
        start_id: u64,
        result: Result<(Receiver, PacketSink), EngineError>,
    ) {
        let route_id = route_key(&peer, id);
        let Some(pos) = self
            .routes
            .iter()
            .position(|r| r.key() == route_id && r.start_id == start_id)
        else {
            if let Ok((receiver, _)) = result {
                drop_off_actor(receiver);
            }
            return;
        };
        match result {
            Ok((receiver, sink)) => {
                let Some(session) = self.sessions.get(&peer) else {
                    drop_off_actor(receiver);
                    self.routes[pos].start_id = 0;
                    return;
                };
                let _ = session.tx.send(SessionCmd::AddSink(id, sink));
                let r = &mut self.routes[pos];
                r.start_id = 0;
                if let Some(old) = r.receiver.replace(receiver) {
                    drop_off_actor(old);
                }
                self.activate_route(peer, id);
            }
            Err(error) => self.fail_route(peer, id, error),
        }
    }

    /// A shared capture failed (device unplugged). The group is closed; routes on the system
    /// default device reopen on the new default, the others stop.
    pub(super) fn on_encoder_failed(&mut self, key: EncoderKey, error: EngineError) {
        self.close_encoder(&key);
        let affected: Vec<(String, DeviceId, u8)> = self
            .routes
            .iter()
            .filter(|r| r.encoder.as_ref() == Some(&key))
            .map(|r| (r.key(), r.peer, r.id))
            .collect();
        let mut notified = false;
        for (route_id, peer, id) in affected {
            if self.reopen_on_default_device(peer, id)
                || !self.routes.iter().any(|r| r.key() == route_id)
            {
                continue;
            }
            if !notified {
                let name = self.peer_name(&peer);
                self.error_notice(&error, vec![name]);
                notified = true;
            }
            self.stop_route_by_key(&route_id, StopReason::AudioDeviceLost, true);
        }
    }

    /// Close a running encoder group now (its device failed or changed). Routes that used it keep
    /// their stale subscription until they are restarted or stopped.
    pub(super) fn close_encoder(&mut self, key: &EncoderKey) {
        // A group still opening opens on the current device anyway: only running ones close.
        if matches!(self.encoders.get(key), Some(EncoderSlot::Running(_))) {
            if let Some(EncoderSlot::Running(sender)) = self.encoders.remove(key) {
                drop_off_actor(sender);
            }
        }
    }

    /// The pipeline is ready: finish starting the route. Rebuilds of active routes need nothing.
    pub(super) fn activate_route(&mut self, peer: DeviceId, id: u8) {
        let route_id = route_key(&peer, id);
        let Some(r) = self.routes.iter_mut().find(|r| r.key() == route_id) else {
            return;
        };
        if r.status != RouteStatus::Starting {
            return;
        }
        r.status = RouteStatus::Active;
        r.started = Instant::now();
        r.started_unix = now_unix();
        r.deadline = None;
        let (requested_locally, kind) = (r.requested_locally, r.kind);
        if requested_locally {
            if let Some(reply) = r.reply.take() {
                let _ = reply.send(Ok(route_id));
            }
        } else if let Some(s) = self.sessions.get(&peer) {
            s.send(Body::RouteAccept(RouteAccept {
                route: id as u32,
                profile: r.profile.clone(),
            }));
        }
        // Who started it: this device, or the peer.
        let starter = if requested_locally { "local" } else { "peer" };
        self.audit_peer(AuditKind::RouteStarted, &peer, Some(kind), starter);
        if requested_locally && self.settings.resume_routes_on_start {
            let peer_id = peer.to_hex();
            if !self
                .settings
                .saved_routes
                .iter()
                .any(|s| s.matches(&peer_id, kind))
            {
                self.settings.saved_routes.push(SavedRoute {
                    peer_id,
                    kind,
                    keep: false,
                });
                self.save_settings();
            }
        }
        self.update_keep_alive();
    }

    /// The pipeline could not start (or restart): tell the peer and the user.
    pub(super) fn fail_route(&mut self, peer: DeviceId, id: u8, error: EngineError) {
        let route_id = route_key(&peer, id);
        let Some(pos) = self.routes.iter().position(|r| r.key() == route_id) else {
            return;
        };
        let mut route = self.routes.remove(pos);
        self.release_pipelines(&mut route);
        if let Some(s) = self.sessions.get(&peer) {
            if route.requested_locally || route.status == RouteStatus::Active {
                s.send(Body::RouteStop(RouteStop {
                    route: id as u32,
                    reason: StopReason::AudioDeviceLost as i32,
                }));
            } else {
                let reason = match error {
                    EngineError::MicPermissionDenied => StopReason::PermissionDenied,
                    EngineError::VirtualMicMissing | EngineError::LoopbackUnsupported => {
                        StopReason::UnsupportedEndpoint
                    }
                    _ => StopReason::AudioDeviceLost,
                };
                s.send(Body::RouteReject(RouteReject {
                    route: id as u32,
                    reason: reason as i32,
                }));
            }
        }
        if let Some(reply) = route.reply.take() {
            let _ = reply.send(Err(error.clone()));
        }
        // A peer's "Mute PC" keeps the speakers muted until it is turned off or the peer leaves.
        if route.kind == RouteKind::SendSystemAudio
            && self.settings.capture.mute_local_speakers
            && self.speakers_muted_by.is_empty()
        {
            self.hooks.set_speakers_muted(false);
        }
        let name = self.peer_name(&peer);
        self.error_notice(&error, vec![name]);
        self.update_keep_alive();
    }

    /// Stop a route's audio (it stays in the route list, e.g. paused).
    pub(super) fn release_pipelines(&mut self, route: &mut Route) {
        let (subscription, receiver) = take_pipelines(route);
        self.finish_release(route.peer, route.id, subscription, receiver);
    }

    pub(super) fn release_pipelines_at(&mut self, index: usize) {
        let Some(route) = self.routes.get_mut(index) else {
            return;
        };
        let (peer, id) = (route.peer, route.id);
        let (subscription, receiver) = take_pipelines(route);
        self.finish_release(peer, id, subscription, receiver);
    }

    fn finish_release(
        &mut self,
        peer: DeviceId,
        id: u8,
        subscription: Option<Subscription>,
        receiver: Option<Receiver>,
    ) {
        drop(subscription);
        if let Some(receiver) = receiver {
            drop_off_actor(receiver);
            if let Some(s) = self.sessions.get(&peer) {
                let _ = s.tx.send(SessionCmd::RemoveSink(id));
            }
        }
        self.prune_encoders();
    }

    /// Close encoder groups nobody listens to any more.
    pub(super) fn prune_encoders(&mut self) {
        let idle: Vec<EncoderKey> = self
            .encoders
            .iter()
            .filter(|(_, slot)| matches!(slot, EncoderSlot::Running(s) if s.subscribers() == 0))
            .map(|(k, _)| k.clone())
            .collect();
        for key in idle {
            if let Some(EncoderSlot::Running(sender)) = self.encoders.remove(&key) {
                drop_off_actor(sender);
            }
        }
    }

    /// Whether this device denoises a microphone stream itself: the setting is on, the user did
    /// not move it to the other device, and noise suppression has not switched itself off because
    /// the machine could not keep up (plan §8.3).
    pub(super) fn denoise_here(&self, profile: &StreamProfile) -> bool {
        self.settings.mic.noise_suppression && !profile.denoise && !self.denoise.suspended()
    }

    /// Push microphone settings to running microphone groups.
    pub(super) fn update_mic_groups(&self) {
        for (key, slot) in &self.encoders {
            let (true, EncoderSlot::Running(sender)) = (key.is_mic(), slot) else {
                continue;
            };
            // A group is shared by every route with the same encoding. It denoises here unless
            // every route it feeds is denoised by the device that plays it.
            let denoise = self
                .routes
                .iter()
                .filter(|r| r.encoder.as_ref() == Some(key))
                .filter_map(|r| r.profile.as_ref())
                .fold(None, |acc: Option<bool>, p| {
                    Some(acc.unwrap_or(false) || self.denoise_here(p))
                })
                .unwrap_or(self.settings.mic.noise_suppression && !self.denoise.suspended());
            let c = &sender.controls;
            c.gain_db.set(self.settings.mic.gain_db);
            c.noise_suppression.store(denoise, Ordering::Relaxed);
            c.high_pass
                .store(self.settings.mic.high_pass, Ordering::Relaxed);
            c.echo_ducking
                .store(self.settings.mic.echo_ducking, Ordering::Relaxed);
            c.muted.store(self.mic_muted, Ordering::Relaxed);
        }
    }
}

fn take_pipelines(route: &mut Route) -> (Option<Subscription>, Option<Receiver>) {
    route.start_id = 0;
    route.encoder = None;
    (route.subscription.take(), route.receiver.take())
}
