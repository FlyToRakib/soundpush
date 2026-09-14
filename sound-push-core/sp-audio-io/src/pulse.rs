//! Linux audio through the PulseAudio protocol. PipeWire desktops serve the same protocol with
//! pipewire-pulse, so this covers PipeWire (Ubuntu, Fedora, …) and PulseAudio-only systems.
//!
//! - Devices are sinks (outputs) and sources (inputs, sink monitors excluded), identified by
//!   their description: the name desktop sound settings show.
//! - System audio is recorded from the monitor source of an output.
//! - One app's sound (or everything except one app) is recorded by moving streams to a private
//!   null sink ([`AppRouting`]).
//! - [`watch_devices`] reports added and removed devices and default device changes.
//! - [`mute_speakers`] silences the speakers on PulseAudio while system audio is still recorded.
//! - The server converts rate, format and channels, so streams are opened directly as 48 kHz
//!   `f32` with the caller's channel count.
//! - [`Pulse`] also lists and loads modules, for the desktop's virtual microphone.
//!
//! Every call fails with an error when no sound server runs; nothing here panics. Nothing here
//! needs root or changes configuration files: modules last until the sound server stops.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use libpulse_binding as pa;
use libpulse_simple_binding::Simple;
use pa::callbacks::ListResult;
use pa::context::subscribe::{Facility, InterestMaskSet, Operation as Event};
use pa::context::{Context, FlagSet as ContextFlags, State as ContextState};
use pa::def::BufferAttr;
use pa::error::PAErr;
use pa::mainloop::standard::{IterateResult, Mainloop};
use pa::operation::{Operation, State as OperationState};
use pa::proplist::{Proplist, properties};
use pa::sample::{Format, Spec};
use pa::stream::Direction;
use tracing::{debug, info, warn};

use crate::{
    AudioError, AudioStream, CaptureCallback, CaptureSource, DeviceInfo, DeviceKind, ErrorCallback,
    RenderCallback, RenderTarget, StreamInfo,
};

const APP_NAME: &str = "SoundPush";
const RATE: u32 = 48_000;
/// How long a request to the sound server may take.
const TIMEOUT: Duration = Duration::from_secs(3);
/// Capture delivers audio in chunks of this length.
const CAPTURE_CHUNK_MS: u32 = 10;
/// Render writes chunks of this length into a server buffer of `RENDER_BUFFER_MS`.
const RENDER_CHUNK_MS: u32 = 10;
const RENDER_BUFFER_MS: u32 = 40;
/// How long dropping a stream waits for its thread (a suspended server can block a read).
const STOP_TIMEOUT: Duration = Duration::from_millis(500);
/// Pause between connection attempts of the device watcher.
const WATCH_RETRY: Duration = Duration::from_secs(3);

/// Null sink that stands in for the speakers while they are muted on PulseAudio.
pub const SPEAKERS_SINK: &str = "soundpush_speakers";
const SPEAKERS_DESCRIPTION: &str = "SoundPush Speakers";
/// Module argument of the speakers sink naming the sink to return to (see [`mute_speakers`]).
const RESTORE_KEY: &str = "soundpush.restore_sink";
/// Private sinks for per-app capture, `soundpush_app_capture_<pid>_<n>`.
const APP_SINK_PREFIX: &str = "soundpush_app_capture_";
const APP_SINK_DESCRIPTION: &str = "SoundPush App Capture";
/// Stream property marking SoundPush's loopback streams, which are never moved.
const INTERNAL_PROPERTY: &str = "soundpush.internal";
/// Delay of the loopback that keeps captured streams audible on the speakers.
const LOOPBACK_LATENCY_MS: u32 = 30;

/// A sink or source on the sound server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    /// Server name, e.g. `alsa_output.pci-0000_00_1f.3.analog-stereo`.
    pub name: String,
    /// Name shown in sound settings; SoundPush uses it as the device id.
    pub description: String,
    pub index: u32,
    pub channels: u16,
    pub sample_rate: u32,
    /// Sinks: the source that carries everything played on this sink.
    pub monitor: Option<String>,
    /// Sources: this source is a sink's monitor.
    pub is_monitor: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Module {
    pub index: u32,
    pub name: String,
    pub argument: String,
}

/// An app's recording stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceOutput {
    /// Index of the source it records from.
    pub source: u32,
    /// Paused by the app.
    pub corked: bool,
    /// `application.id` (e.g. `org.PulseAudio.pavucontrol`), empty if the app sets none.
    pub application_id: String,
}

/// A playback stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SinkInput {
    pub index: u32,
    /// Index of the sink it plays on.
    pub sink: u32,
    /// The module that plays it. PulseAudio sets this for app streams too (the protocol module).
    pub owner_module: Option<u32>,
    /// The connected client; `None` for streams a module plays (loopbacks, combined sinks).
    pub client: Option<u32>,
    pub process_id: Option<u32>,
    /// `application.process.binary` (e.g. `firefox`), else `application.name`.
    pub app: String,
    /// Paused by the app.
    pub corked: bool,
    /// One of SoundPush's loopback streams.
    pub internal: bool,
}

impl SinkInput {
    /// Streams SoundPush never moves: its own playback and streams of modules.
    fn fixed(&self) -> bool {
        self.process_id == Some(std::process::id()) || self.client.is_none() || self.internal
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Server {
    /// e.g. "pulseaudio" or "PulseAudio (on PipeWire 1.0.5)".
    pub name: String,
    pub default_sink: Option<String>,
    pub default_source: Option<String>,
}

impl Server {
    pub fn is_pipewire(&self) -> bool {
        self.name.contains("PipeWire")
    }
}

/// An app playing sound, for the per-app capture picker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioApp {
    /// What capture settings store (see [`SinkInput::app`]).
    pub process: String,
    /// The app is playing sound right now.
    pub active: bool,
}

fn backend(message: impl Into<String>) -> AudioError {
    AudioError::Backend(message.into())
}

fn no_server() -> AudioError {
    backend("no PipeWire or PulseAudio sound server is running")
}

fn lost_server() -> AudioError {
    backend("lost the sound server connection")
}

/// Items collected by a list callback, and whether the server reported an error.
type Collected<T> = Rc<RefCell<(Vec<T>, bool)>>;

fn take<T>(collected: &Collected<T>) -> Result<Vec<T>, AudioError> {
    let (items, failed) = std::mem::take(&mut *collected.borrow_mut());
    if failed {
        Err(backend("the sound server could not list its devices"))
    } else {
        Ok(items)
    }
}

/// A connection to the sound server, used from one thread.
pub struct Pulse {
    // Declared before the main loop so it is dropped first.
    context: Context,
    mainloop: Mainloop,
}

impl Pulse {
    pub fn connect() -> Result<Self, AudioError> {
        Self::connect_to(None)
    }

    fn connect_to(server: Option<&str>) -> Result<Self, AudioError> {
        let mainloop =
            Mainloop::new().ok_or_else(|| backend("could not create the PulseAudio main loop"))?;
        let mut proplist = Proplist::new()
            .ok_or_else(|| backend("could not create a PulseAudio property list"))?;
        let _ = proplist.set_str(properties::APPLICATION_NAME, APP_NAME);
        let _ = proplist.set_str(properties::APPLICATION_ID, "net.soundpush.desktop");
        let context = Context::new_with_proplist(&mainloop, APP_NAME, &proplist)
            .ok_or_else(|| backend("could not create a PulseAudio context"))?;
        let mut pulse = Self { context, mainloop };
        pulse
            .context
            .connect(server, ContextFlags::NOAUTOSPAWN, None)
            .map_err(|_| no_server())?;
        let deadline = Instant::now() + TIMEOUT;
        loop {
            pulse.step()?;
            match pulse.context.get_state() {
                ContextState::Ready => return Ok(pulse),
                ContextState::Failed | ContextState::Terminated => return Err(no_server()),
                _ if Instant::now() > deadline => return Err(no_server()),
                _ => {}
            }
        }
    }

    /// Dispatch pending events without blocking; pause briefly when there were none.
    fn step(&mut self) -> Result<(), AudioError> {
        match self.mainloop.iterate(false) {
            IterateResult::Success(0) => std::thread::sleep(Duration::from_millis(1)),
            IterateResult::Success(_) => {}
            IterateResult::Quit(_) | IterateResult::Err(_) => return Err(lost_server()),
        }
        Ok(())
    }

    /// Dispatch events, waiting for one when `block` is set. Fails once the connection is gone.
    fn dispatch(&mut self, block: bool) -> Result<(), AudioError> {
        match self.mainloop.iterate(block) {
            IterateResult::Success(_)
                if matches!(self.context.get_state(), ContextState::Ready) =>
            {
                Ok(())
            }
            _ => Err(lost_server()),
        }
    }

    fn wait<C: ?Sized>(&mut self, mut op: Operation<C>) -> Result<(), AudioError> {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            match op.get_state() {
                OperationState::Done => return Ok(()),
                OperationState::Cancelled => {
                    return Err(backend("the sound server cancelled the request"));
                }
                OperationState::Running if Instant::now() > deadline => {
                    op.cancel();
                    return Err(backend("the sound server did not answer"));
                }
                OperationState::Running => self.step()?,
            }
        }
    }

    /// Wait for a request that reports success, failing with `failure` if it did not succeed.
    fn confirm(
        &mut self,
        op: Operation<dyn FnMut(bool)>,
        ok: &Cell<bool>,
        failure: impl FnOnce() -> String,
    ) -> Result<(), AudioError> {
        self.wait(op)?;
        if ok.get() {
            Ok(())
        } else {
            Err(backend(failure()))
        }
    }

    /// Call `callback` for every event of `mask`, from inside [`Self::dispatch`] and requests.
    fn subscribe(
        &mut self,
        mask: InterestMaskSet,
        mut callback: impl FnMut(Option<Facility>, Option<Event>) + 'static,
    ) -> Result<(), AudioError> {
        self.context
            .set_subscribe_callback(Some(Box::new(move |facility, event, _| {
                callback(facility, event)
            })));
        let ok = Rc::new(Cell::new(false));
        let result = ok.clone();
        let op = self.context.subscribe(mask, move |s| result.set(s));
        self.confirm(op, &ok, || "the sound server refused to send events".into())
    }

    pub fn server(&mut self) -> Result<Server, AudioError> {
        let found = Rc::new(RefCell::new(None));
        let slot = found.clone();
        let op = self.context.introspect().get_server_info(move |i| {
            *slot.borrow_mut() = Some(Server {
                name: i.server_name.as_deref().unwrap_or_default().to_string(),
                default_sink: i.default_sink_name.as_deref().map(str::to_string),
                default_source: i.default_source_name.as_deref().map(str::to_string),
            });
        });
        self.wait(op)?;
        found
            .take()
            .ok_or_else(|| backend("the sound server sent no server information"))
    }

    pub fn sinks(&mut self) -> Result<Vec<Device>, AudioError> {
        let found: Collected<Device> = Rc::default();
        let list = found.clone();
        let op = self
            .context
            .introspect()
            .get_sink_info_list(move |r| match r {
                ListResult::Item(i) => list.borrow_mut().0.push(Device {
                    name: i.name.as_deref().unwrap_or_default().to_string(),
                    description: i.description.as_deref().unwrap_or_default().to_string(),
                    index: i.index,
                    channels: i.sample_spec.channels.into(),
                    sample_rate: i.sample_spec.rate,
                    monitor: i.monitor_source_name.as_deref().map(str::to_string),
                    is_monitor: false,
                }),
                ListResult::Error => list.borrow_mut().1 = true,
                ListResult::End => {}
            });
        self.wait(op)?;
        take(&found)
    }

    pub fn sources(&mut self) -> Result<Vec<Device>, AudioError> {
        let found: Collected<Device> = Rc::default();
        let list = found.clone();
        let op = self
            .context
            .introspect()
            .get_source_info_list(move |r| match r {
                ListResult::Item(i) => list.borrow_mut().0.push(Device {
                    name: i.name.as_deref().unwrap_or_default().to_string(),
                    description: i.description.as_deref().unwrap_or_default().to_string(),
                    index: i.index,
                    channels: i.sample_spec.channels.into(),
                    sample_rate: i.sample_spec.rate,
                    monitor: None,
                    is_monitor: i.monitor_of_sink.is_some(),
                }),
                ListResult::Error => list.borrow_mut().1 = true,
                ListResult::End => {}
            });
        self.wait(op)?;
        take(&found)
    }

    pub fn modules(&mut self) -> Result<Vec<Module>, AudioError> {
        let found: Collected<Module> = Rc::default();
        let list = found.clone();
        let op = self
            .context
            .introspect()
            .get_module_info_list(move |r| match r {
                ListResult::Item(i) => list.borrow_mut().0.push(Module {
                    index: i.index,
                    name: i.name.as_deref().unwrap_or_default().to_string(),
                    argument: i.argument.as_deref().unwrap_or_default().to_string(),
                }),
                ListResult::Error => list.borrow_mut().1 = true,
                ListResult::End => {}
            });
        self.wait(op)?;
        take(&found)
    }

    pub fn source_outputs(&mut self) -> Result<Vec<SourceOutput>, AudioError> {
        let found: Collected<SourceOutput> = Rc::default();
        let list = found.clone();
        let op = self
            .context
            .introspect()
            .get_source_output_info_list(move |r| match r {
                ListResult::Item(i) => list.borrow_mut().0.push(SourceOutput {
                    source: i.source,
                    corked: i.corked,
                    application_id: i
                        .proplist
                        .get_str(properties::APPLICATION_ID)
                        .unwrap_or_default(),
                }),
                ListResult::Error => list.borrow_mut().1 = true,
                ListResult::End => {}
            });
        self.wait(op)?;
        take(&found)
    }

    pub fn sink_inputs(&mut self) -> Result<Vec<SinkInput>, AudioError> {
        let found: Collected<SinkInput> = Rc::default();
        let list = found.clone();
        let op = self
            .context
            .introspect()
            .get_sink_input_info_list(move |r| match r {
                ListResult::Item(i) => {
                    let property = |key: &str| i.proplist.get_str(key).filter(|v| !v.is_empty());
                    list.borrow_mut().0.push(SinkInput {
                        index: i.index,
                        sink: i.sink,
                        owner_module: i.owner_module,
                        client: i.client,
                        process_id: property(properties::APPLICATION_PROCESS_ID)
                            .and_then(|p| p.parse().ok()),
                        app: property(properties::APPLICATION_PROCESS_BINARY)
                            .or_else(|| property(properties::APPLICATION_NAME))
                            .unwrap_or_default(),
                        corked: i.corked,
                        internal: property(INTERNAL_PROPERTY).is_some(),
                    });
                }
                ListResult::Error => list.borrow_mut().1 = true,
                ListResult::End => {}
            });
        self.wait(op)?;
        take(&found)
    }

    /// Load a module (like `pactl load-module`) and return its index.
    pub fn load_module(&mut self, name: &str, argument: &str) -> Result<u32, AudioError> {
        let index = Rc::new(Cell::new(u32::MAX));
        let result = index.clone();
        let op = self
            .context
            .introspect()
            .load_module(name, argument, move |i| result.set(i));
        self.wait(op)?;
        match index.get() {
            u32::MAX => Err(backend(format!("the sound server could not load {name}"))),
            i => Ok(i),
        }
    }

    pub fn unload_module(&mut self, index: u32) -> Result<(), AudioError> {
        let ok = Rc::new(Cell::new(false));
        let result = ok.clone();
        let op = self
            .context
            .introspect()
            .unload_module(index, move |s| result.set(s));
        self.confirm(op, &ok, || {
            format!("the sound server could not unload module {index}")
        })
    }

    /// Move a playback stream to the sink named `sink` (like `pactl move-sink-input`).
    pub fn move_sink_input(&mut self, index: u32, sink: &str) -> Result<(), AudioError> {
        let ok = Rc::new(Cell::new(false));
        let result = ok.clone();
        let op = self.context.introspect().move_sink_input_by_name(
            index,
            sink,
            Some(Box::new(move |s| result.set(s))),
        );
        self.confirm(op, &ok, || {
            format!("the sound server could not move stream {index} to {sink}")
        })
    }

    /// Suspend and resume one of SoundPush's null sinks once streams have moved onto it. A
    /// PulseAudio null sink that starts playing keeps rendering in long blocks until it restarts,
    /// so its monitor would stay silent for about two seconds (measured 1.7 s; 60 ms after this).
    fn restart_sink(&mut self, sink: &str) {
        for suspend in [true, false] {
            let ok = Rc::new(Cell::new(false));
            let result = ok.clone();
            let op = self.context.introspect().suspend_sink_by_name(
                sink,
                suspend,
                Some(Box::new(move |s| result.set(s))),
            );
            if let Err(e) = self.confirm(op, &ok, || format!("could not restart {sink}")) {
                debug!(error = %e, "sink not restarted");
                return;
            }
        }
    }

    pub fn set_default_sink(&mut self, sink: &str) -> Result<(), AudioError> {
        let ok = Rc::new(Cell::new(false));
        let result = ok.clone();
        let op = self.context.set_default_sink(sink, move |s| result.set(s));
        self.confirm(op, &ok, || {
            format!("the sound server could not make {sink} the default output")
        })
    }

    /// Report device changes until the connection ends (see [`watch_devices`]).
    fn follow_devices(&mut self, on_change: &mut dyn FnMut(bool, bool)) -> Result<(), AudioError> {
        // (a device was added or removed, the server changed)
        let pending = Rc::new(Cell::new((false, false)));
        let flags = pending.clone();
        self.subscribe(
            InterestMaskSet::SINK | InterestMaskSet::SOURCE | InterestMaskSet::SERVER,
            move |facility, event| {
                let (devices, server) = flags.get();
                match (facility, event) {
                    // The default sink or source is a server property.
                    (Some(Facility::Server), _) => flags.set((devices, true)),
                    // Volume, mute and suspend changes are not device changes.
                    (
                        Some(Facility::Sink | Facility::Source),
                        Some(Event::New | Event::Removed),
                    ) => flags.set((true, server)),
                    _ => {}
                }
            },
        )?;
        let defaults = |s: Server| (s.default_source, s.default_sink);
        let mut last = defaults(self.server()?);
        loop {
            let (devices, server) = pending.replace((false, false));
            if !devices && !server {
                self.dispatch(true)?;
                continue;
            }
            let (mut input, mut output) = (false, false);
            if server {
                let now = defaults(self.server()?);
                (input, output) = (now.0 != last.0, now.1 != last.1);
                last = now;
            }
            if devices || input || output {
                on_change(input, output);
            }
        }
    }
}

impl Drop for Pulse {
    fn drop(&mut self) {
        self.context.disconnect();
    }
}

/// Whether a sound server answers. Cached briefly: capability checks run on every state change.
pub fn available() -> bool {
    static LAST: Mutex<Option<(Instant, bool)>> = Mutex::new(None);
    let Ok(mut last) = LAST.lock() else {
        return false;
    };
    if let Some((at, ok)) = *last
        && at.elapsed() < Duration::from_secs(5)
    {
        return ok;
    }
    let ok = Pulse::connect().is_ok();
    *last = Some((Instant::now(), ok));
    ok
}

/// Call `on_change(default_input_changed, default_output_changed)` whenever the sound server adds
/// or removes a device (both false) or changes its default input or output, for the life of the
/// process. When the server (re)starts, devices may differ, so that is reported as both changing.
pub fn watch_devices(
    mut on_change: impl FnMut(bool, bool) + Send + 'static,
) -> Result<(), AudioError> {
    std::thread::Builder::new()
        .name("sp-device-watch".into())
        .spawn(move || {
            let mut first = true;
            loop {
                if let Ok(mut pulse) = Pulse::connect() {
                    if !first {
                        on_change(true, true);
                    }
                    info!("watching sound server devices");
                    if let Err(e) = pulse.follow_devices(&mut on_change) {
                        info!(error = %e, "stopped watching sound server devices");
                    }
                }
                first = false;
                std::thread::sleep(WATCH_RETRY);
            }
        })
        .map(drop)
        .map_err(|e| backend(e.to_string()))
}

/// A device by its id (description), or by its server name.
fn find<'a>(devices: &'a [Device], id: &str) -> Option<&'a Device> {
    devices
        .iter()
        .find(|d| d.description == id)
        .or_else(|| devices.iter().find(|d| d.name == id))
}

/// Sinks SoundPush creates for itself, never offered as devices.
fn hidden_sink(name: &str) -> bool {
    name == SPEAKERS_SINK || name.starts_with(APP_SINK_PREFIX)
}

/// Sinks whose streams stay where they are: SoundPush's own (such as the virtual microphone feed).
/// The speakers sink only stands in for the user's speakers, so its streams do move.
fn own_sink(name: &str) -> bool {
    name.starts_with("soundpush_") && name != SPEAKERS_SINK
}

/// Whether a module argument string contains `pair` ("key=value") as a whole word.
fn has_argument(argument: &str, pair: &str) -> bool {
    argument.split_whitespace().any(|word| word == pair)
}

/// Sink names SoundPush writes into module arguments: nothing that needs quoting.
fn plain_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-:@".contains(c))
}

/// The same app, ignoring case.
fn same_app(a: &str, b: &str) -> bool {
    !a.trim().is_empty() && a.trim().eq_ignore_ascii_case(b.trim())
}

pub fn list_devices() -> Result<Vec<DeviceInfo>, AudioError> {
    let mut pulse = Pulse::connect()?;
    let server = pulse.server()?;
    let info = |d: Device, kind: DeviceKind, default: &Option<String>| DeviceInfo {
        id: d.description.clone(),
        is_default: default.as_deref() == Some(d.name.as_str()),
        name: d.description,
        kind,
        channels: d.channels,
        sample_rate: d.sample_rate,
    };
    let mut out: Vec<DeviceInfo> = pulse
        .sources()?
        .into_iter()
        .filter(|d| !d.is_monitor)
        .map(|d| info(d, DeviceKind::Input, &server.default_source))
        .collect();
    // While the speakers are muted the default is SoundPush Speakers; show the real speakers as
    // the default instead.
    let default_sink = match server.default_sink.as_deref() {
        Some(SPEAKERS_SINK) => speakers_restore_sink(&mut pulse),
        _ => server.default_sink.clone(),
    };
    out.extend(
        pulse
            .sinks()?
            .into_iter()
            .filter(|d| !hidden_sink(&d.name))
            .map(|d| info(d, DeviceKind::Output, &default_sink)),
    );
    Ok(out)
}

pub fn output_names() -> Result<Vec<String>, AudioError> {
    Ok(Pulse::connect()?
        .sinks()?
        .into_iter()
        .filter(|d| !hidden_sink(&d.name))
        .map(|d| d.description)
        .collect())
}

// ------------------------------------------------------------------ muting the speakers

/// The sink name stored in the speakers sink's module arguments.
fn restore_sink(argument: &str) -> Option<String> {
    let key = format!("{RESTORE_KEY}='");
    let rest = &argument[argument.find(&key)? + key.len()..];
    Some(rest[..rest.find('\'')?].to_string()).filter(|name| plain_name(name))
}

fn speakers_modules(pulse: &mut Pulse) -> Result<Vec<Module>, AudioError> {
    let pair = format!("sink_name={SPEAKERS_SINK}");
    Ok(pulse
        .modules()?
        .into_iter()
        .filter(|m| m.name == "module-null-sink" && has_argument(&m.argument, &pair))
        .collect())
}

/// While the speakers are muted, the real speakers' sink name.
fn speakers_restore_sink(pulse: &mut Pulse) -> Option<String> {
    speakers_modules(pulse)
        .ok()?
        .iter()
        .find_map(|m| restore_sink(&m.argument))
}

/// "Mute PC speakers" on PulseAudio. PulseAudio mutes a sink's monitor together with the sink, so
/// muting the speakers would also silence the system audio being sent. Instead a null sink
/// "SoundPush Speakers" becomes the default and the default sink's streams move onto it
/// (SoundPush's own playback excepted): nothing reaches the speakers, and system audio is
/// recorded from its monitor. The previous default is kept in the module's arguments, so
/// [`unmute_speakers`] restores it even after a crash. Muting again only re-applies the default.
pub fn mute_speakers() -> Result<(), AudioError> {
    let mut pulse = Pulse::connect()?;
    let sinks = pulse.sinks()?;
    let default = pulse.server()?.default_sink;
    if sinks.iter().any(|s| s.name == SPEAKERS_SINK) {
        if default.as_deref() != Some(SPEAKERS_SINK) {
            pulse.set_default_sink(SPEAKERS_SINK)?;
        }
        return Ok(());
    }
    let speakers = default
        .as_deref()
        .and_then(|name| sinks.iter().find(|s| s.name == name))
        .ok_or(AudioError::NoDefaultDevice)?;
    let restore = if plain_name(&speakers.name) {
        format!(" {RESTORE_KEY}='{}'", speakers.name)
    } else {
        String::new()
    };
    let module = pulse.load_module(
        "module-null-sink",
        &format!(
            "sink_name={SPEAKERS_SINK} rate={RATE} sink_properties=\"device.description='{SPEAKERS_DESCRIPTION}'{restore}\""
        ),
    )?;
    let moved = pulse.set_default_sink(SPEAKERS_SINK).and_then(|()| {
        let inputs = pulse.sink_inputs()?;
        let own = std::process::id();
        let mut moved = 0;
        for input in inputs
            .iter()
            .filter(|i| i.sink == speakers.index && i.process_id != Some(own))
        {
            // A stream that ended meanwhile cannot move; the others still do.
            match pulse.move_sink_input(input.index, SPEAKERS_SINK) {
                Ok(()) => moved += 1,
                Err(e) => debug!(error = %e, "stream not moved to SoundPush Speakers"),
            }
        }
        Ok(moved)
    });
    match moved {
        Ok(0) => {}
        Ok(_) => pulse.restart_sink(SPEAKERS_SINK),
        Err(e) => {
            let _ = pulse.set_default_sink(&speakers.name);
            let _ = pulse.unload_module(module);
            return Err(e);
        }
    }
    info!(speakers = %speakers.name, "speakers muted: playback moved to SoundPush Speakers");
    Ok(())
}

/// Undo [`mute_speakers`]: the previous default sink (another real sink if it is gone) becomes
/// the default again, streams move back to it, and "SoundPush Speakers" is removed. Does nothing
/// when the speakers were not muted this way.
pub fn unmute_speakers() -> Result<(), AudioError> {
    let mut pulse = Pulse::connect()?;
    let modules = speakers_modules(&mut pulse)?;
    if modules.is_empty() {
        return Ok(());
    }
    let sinks = pulse.sinks()?;
    let target = modules
        .iter()
        .find_map(|m| restore_sink(&m.argument))
        .filter(|name| sinks.iter().any(|s| &s.name == name))
        .or_else(|| {
            sinks
                .iter()
                .find(|s| !s.name.starts_with("soundpush_"))
                .map(|s| s.name.clone())
        });
    if let Some(target) = &target {
        if pulse.server()?.default_sink.as_deref() == Some(SPEAKERS_SINK) {
            pulse.set_default_sink(target)?;
        }
        if let Some(speakers) = sinks.iter().find(|s| s.name == SPEAKERS_SINK) {
            for input in pulse
                .sink_inputs()?
                .iter()
                .filter(|i| i.sink == speakers.index)
            {
                if let Err(e) = pulse.move_sink_input(input.index, target) {
                    debug!(error = %e, "stream not moved back to the speakers");
                }
            }
        }
    }
    // Streams that could not move are rescued to the default by the server.
    for module in modules {
        pulse.unload_module(module.index)?;
    }
    info!(
        speakers = target.as_deref().unwrap_or("none"),
        "speakers unmuted"
    );
    Ok(())
}

// ------------------------------------------------------------------ per-app capture

/// Per-app capture: what one app plays, or everything except one app.
///
/// The streams to send move to a private null sink whose monitor SoundPush records. A loopback
/// plays that sink on the default speakers, so the user still hears everything (about
/// `LOOPBACK_LATENCY_MS` later). Streams opened while capturing are moved as they appear. On stop
/// the streams go back and both modules are unloaded; after a crash, [`remove_stale_app_captures`]
/// does that on the next start (the loopback keeps everything audible until then).
struct AppRouting {
    process: String,
    exclude: bool,
    sink: String,
    sink_module: u32,
    loopback_module: u32,
    /// The default sink when capture started: where the loopback first played.
    playback: Option<String>,
    /// Where each moved stream played before.
    moved: HashMap<u32, String>,
}

impl AppRouting {
    fn open(pulse: &mut Pulse, process: &str, exclude: bool) -> Result<Self, AudioError> {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let sink = format!(
            "{APP_SINK_PREFIX}{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let playback = pulse.server()?.default_sink;
        let sink_module = pulse.load_module(
            "module-null-sink",
            &format!(
                "sink_name={sink} rate={RATE} sink_properties=\"device.description='{APP_SINK_DESCRIPTION}'\""
            ),
        )?;
        // Without a `sink` argument the loopback plays on the default speakers. With
        // `source_dont_move` it ends together with the capture sink instead of moving elsewhere.
        let loopback = pulse.load_module(
            "module-loopback",
            &format!(
                "source={sink}.monitor latency_msec={LOOPBACK_LATENCY_MS} source_dont_move=true sink_input_properties=\"{INTERNAL_PROPERTY}=1\""
            ),
        );
        let loopback_module = match loopback {
            Ok(index) => index,
            Err(e) => {
                let _ = pulse.unload_module(sink_module);
                return Err(e);
            }
        };
        info!(app = process, exclude, sink, "app capture started");
        Ok(Self {
            process: process.to_string(),
            exclude,
            sink,
            sink_module,
            loopback_module,
            playback,
            moved: HashMap::new(),
        })
    }

    /// Move the streams to capture that are not on the capture sink yet.
    fn route(&mut self, pulse: &mut Pulse) -> Result<(), AudioError> {
        let sinks = pulse.sinks()?;
        let ours = sinks
            .iter()
            .find(|s| s.name == self.sink)
            .ok_or(AudioError::DeviceLost)?
            .index;
        let inputs = pulse.sink_inputs()?;
        self.moved
            .retain(|index, _| inputs.iter().any(|i| i.index == *index));
        let was_empty = !inputs.iter().any(|i| i.sink == ours);
        let mut moved_any = false;
        for input in &inputs {
            if input.sink == ours
                || input.fixed()
                || same_app(&input.app, &self.process) == self.exclude
            {
                continue;
            }
            let Some(from) = sinks.iter().find(|s| s.index == input.sink) else {
                continue;
            };
            if own_sink(&from.name) {
                continue;
            }
            match pulse.move_sink_input(input.index, &self.sink) {
                Ok(()) => {
                    self.moved.insert(input.index, from.name.clone());
                    moved_any = true;
                }
                // Some streams refuse to move (`dont_move`); they are simply not captured.
                Err(e) => debug!(error = %e, app = %input.app, "stream not moved for app capture"),
            }
        }
        if was_empty && moved_any {
            pulse.restart_sink(&self.sink);
        }
        Ok(())
    }

    /// Put the streams back and remove the modules. Stopping never fails; problems are logged.
    fn close(self, pulse: &mut Pulse) {
        let released = release_streams(
            pulse,
            &self.sink,
            Some(self.loopback_module),
            self.playback.as_deref(),
            &self.moved,
        );
        if let Err(e) = released {
            warn!(error = %e, "could not move app streams back");
        }
        for module in [self.loopback_module, self.sink_module] {
            if let Err(e) = pulse.unload_module(module) {
                warn!(error = %e, "could not remove app capture");
            }
        }
        info!(sink = %self.sink, "app capture stopped");
    }
}

/// Move every stream off the capture sink `sink`, back to the sink it came from. Streams from
/// the speakers the loopback started on (`playback`), and streams of unknown origin, go wherever
/// the loopback plays now: the default speakers, or SoundPush Speakers while muted.
fn release_streams(
    pulse: &mut Pulse,
    sink: &str,
    loopback: Option<u32>,
    playback: Option<&str>,
    moved: &HashMap<u32, String>,
) -> Result<(), AudioError> {
    let sinks = pulse.sinks()?;
    let Some(ours) = sinks.iter().find(|s| s.name == sink) else {
        return Ok(());
    };
    let exists = |name: &str| name != sink && sinks.iter().any(|s| s.name == name);
    let inputs = pulse.sink_inputs()?;
    let loopback_sink = loopback
        .and_then(|module| inputs.iter().find(|i| i.owner_module == Some(module)))
        .and_then(|i| sinks.iter().find(|s| s.index == i.sink))
        .map(|s| s.name.clone());
    let fallback = match loopback_sink.filter(|name| exists(name)) {
        Some(name) => Some(name),
        None => pulse.server()?.default_sink.filter(|name| exists(name)),
    };
    for input in inputs.iter().filter(|i| i.sink == ours.index) {
        let target = moved
            .get(&input.index)
            .filter(|from| Some(from.as_str()) != playback && exists(from))
            .or(fallback.as_ref());
        // Without a target, unloading the sink lets the server move the stream.
        if let Some(target) = target
            && let Err(e) = pulse.move_sink_input(input.index, target)
        {
            debug!(error = %e, "stream not moved back from app capture");
        }
    }
    Ok(())
}

/// The process id in a per-app capture sink name.
fn app_sink_pid(sink: &str) -> Option<u32> {
    sink.strip_prefix(APP_SINK_PREFIX)?
        .split('_')
        .next()?
        .parse()
        .ok()
}

fn process_running(pid: u32) -> bool {
    pid == std::process::id() || std::path::Path::new(&format!("/proc/{pid}")).exists()
}

fn remove_stale(pulse: &mut Pulse) -> Result<(), AudioError> {
    let modules = pulse.modules()?;
    for sink_module in modules.iter().filter(|m| m.name == "module-null-sink") {
        let Some(sink) = sink_module
            .argument
            .split_whitespace()
            .find_map(|word| word.strip_prefix("sink_name="))
            .filter(|name| name.starts_with(APP_SINK_PREFIX))
        else {
            continue;
        };
        if app_sink_pid(sink).is_some_and(process_running) {
            continue;
        }
        info!(sink, "removing app capture left by a previous run");
        let source = format!("source={sink}.monitor");
        let loopback = modules
            .iter()
            .find(|m| m.name == "module-loopback" && has_argument(&m.argument, &source))
            .map(|m| m.index);
        release_streams(pulse, sink, loopback, None, &HashMap::new())?;
        if let Some(index) = loopback {
            pulse.unload_module(index)?;
        }
        pulse.unload_module(sink_module.index)?;
    }
    Ok(())
}

/// Undo per-app capture left behind by a SoundPush process that no longer runs, so apps play
/// normally again. Capture set up by running processes is left alone.
pub fn remove_stale_app_captures() -> Result<(), AudioError> {
    remove_stale(&mut Pulse::connect()?)
}

/// Apps with playback streams (SoundPush's own and module streams excluded), playing ones first.
pub fn audio_apps() -> Result<Vec<AudioApp>, AudioError> {
    let mut apps: Vec<AudioApp> = Vec::new();
    for input in Pulse::connect()?.sink_inputs()? {
        if input.fixed() || input.app.trim().is_empty() {
            continue;
        }
        match apps.iter_mut().find(|a| same_app(&a.process, &input.app)) {
            Some(app) => app.active |= !input.corked,
            None => apps.push(AudioApp {
                process: input.app,
                active: !input.corked,
            }),
        }
    }
    apps.sort_by(|a, b| {
        b.active
            .cmp(&a.active)
            .then_with(|| a.process.to_lowercase().cmp(&b.process.to_lowercase()))
    });
    Ok(apps)
}

// ------------------------------------------------------------------ streams

/// Server-side name of the source to record from (`None` for the default input), and the
/// routing that per-app capture sets up for it.
fn capture_device(
    pulse: &mut Pulse,
    source: &CaptureSource,
) -> Result<(Option<String>, Option<AppRouting>), AudioError> {
    match source {
        CaptureSource::DefaultInput => Ok((None, None)),
        CaptureSource::Application { process, exclude } => {
            if let Err(e) = remove_stale(pulse) {
                warn!(error = %e, "could not remove stale app capture");
            }
            let mut routing = AppRouting::open(pulse, process, *exclude)?;
            match routing.route(pulse) {
                Ok(()) => Ok((Some(format!("{}.monitor", routing.sink)), Some(routing))),
                Err(e) => {
                    routing.close(pulse);
                    Err(e)
                }
            }
        }
        CaptureSource::Input(id) => find(&pulse.sources()?, id)
            .map(|d| (Some(d.name.clone()), None))
            .ok_or_else(|| AudioError::DeviceNotFound(id.clone())),
        CaptureSource::SystemLoopback(id) => {
            let sinks = pulse.sinks()?;
            let sink = match id {
                Some(id) => {
                    let sink =
                        find(&sinks, id).ok_or_else(|| AudioError::DeviceNotFound(id.clone()))?;
                    // Muted speakers play into SoundPush Speakers, so their sound is recorded there.
                    match sinks.iter().find(|s| s.name == SPEAKERS_SINK) {
                        Some(speakers)
                            if speakers_restore_sink(pulse).as_deref() == Some(&sink.name) =>
                        {
                            speakers
                        }
                        _ => sink,
                    }
                }
                None => {
                    let default = pulse.server()?.default_sink;
                    match sinks.iter().find(|d| Some(&d.name) == default.as_ref()) {
                        Some(sink) => sink,
                        // PipeWire without a session manager reports "@DEFAULT_SINK@" rather than
                        // a sink name; the server resolves its own special monitor name.
                        None if !sinks.is_empty() => {
                            return Ok((Some("@DEFAULT_MONITOR@".into()), None));
                        }
                        None => return Err(AudioError::NoDefaultDevice),
                    }
                }
            };
            sink.monitor
                .clone()
                .map(|monitor| (Some(monitor), None))
                .ok_or_else(|| {
                    AudioError::DeviceNotFound(format!("monitor of {}", sink.description))
                })
        }
    }
}

fn render_device(pulse: &mut Pulse, target: &RenderTarget) -> Result<Option<String>, AudioError> {
    match target {
        // While the speakers are muted the default is SoundPush Speakers: play on the real
        // speakers, so received audio stays audible and is not sent straight back.
        RenderTarget::DefaultOutput => match pulse.server()?.default_sink.as_deref() {
            Some(SPEAKERS_SINK) => Ok(speakers_restore_sink(pulse)),
            _ => Ok(None),
        },
        RenderTarget::Output(id) => find(&pulse.sinks()?, id)
            .map(|d| Some(d.name.clone()))
            .ok_or_else(|| AudioError::DeviceNotFound(id.clone())),
    }
}

fn spec(channels: u16) -> Result<Spec, AudioError> {
    let spec = Spec {
        format: Format::FLOAT32NE,
        rate: RATE,
        channels: u8::try_from(channels).unwrap_or(0),
    };
    if spec.is_valid() {
        Ok(spec)
    } else {
        Err(AudioError::FormatUnsupported(format!(
            "{channels} channels"
        )))
    }
}

fn open_error(e: PAErr, device: Option<&str>) -> AudioError {
    // libpulse error codes: PA_ERR_ACCESS = 1, PA_ERR_CONNECTIONREFUSED = 6, PA_ERR_NOENTITY = 5.
    match e.0.abs() {
        1 => AudioError::PermissionDenied,
        5 => device.map_or(AudioError::NoDefaultDevice, |d| {
            AudioError::DeviceNotFound(d.to_string())
        }),
        6 => no_server(),
        _ => backend(format!("{e}")),
    }
}

/// A stream served by its own thread, which exits when `stop` is set.
struct PulseStream {
    stop: Arc<AtomicBool>,
    stopped: mpsc::Receiver<()>,
    info: StreamInfo,
}

impl AudioStream for PulseStream {
    fn info(&self) -> StreamInfo {
        self.info
    }
}

impl Drop for PulseStream {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // Reads and writes return every few milliseconds while the server runs. If it stalls, the
        // thread is left to finish on its own rather than blocking the caller.
        if self.stopped.recv_timeout(STOP_TIMEOUT) == Err(mpsc::RecvTimeoutError::Timeout) {
            warn!("audio stream did not stop in time");
        }
    }
}

fn spawn(
    name: &str,
    info: StreamInfo,
    body: impl FnOnce(&AtomicBool) + Send + 'static,
) -> Result<Box<dyn AudioStream>, AudioError> {
    let stop = Arc::new(AtomicBool::new(false));
    let (done, stopped) = mpsc::channel();
    let flag = stop.clone();
    std::thread::Builder::new()
        .name(name.into())
        .spawn(move || {
            body(&flag);
            let _ = done.send(());
        })
        .map_err(|e| backend(e.to_string()))?;
    Ok(Box::new(PulseStream {
        stop,
        stopped,
        info,
    }))
}

/// A connection that flags new and changed playback streams, for per-app capture.
fn follow_sink_inputs() -> Result<(Pulse, Rc<Cell<bool>>), AudioError> {
    let mut pulse = Pulse::connect()?;
    let pending = Rc::new(Cell::new(false));
    let flag = pending.clone();
    pulse.subscribe(InterestMaskSet::SINK_INPUT, move |_, event| {
        if matches!(event, Some(Event::New | Event::Changed)) {
            flag.set(true);
        }
    })?;
    Ok((pulse, pending))
}

pub fn open_capture(
    source: &CaptureSource,
    channels: u16,
    mut on_audio: CaptureCallback,
    mut on_error: ErrorCallback,
) -> Result<Box<dyn AudioStream>, AudioError> {
    let spec = spec(channels)?;
    let mut pulse = Pulse::connect()?;
    let (device, mut routing) = capture_device(&mut pulse, source)?;
    let samples = (RATE * CAPTURE_CHUNK_MS / 1000) as usize * usize::from(channels);
    let bytes = u32::try_from(samples * 4).unwrap_or(u32::MAX);
    let attr = BufferAttr {
        maxlength: u32::MAX,
        tlength: u32::MAX,
        prebuf: u32::MAX,
        minreq: u32::MAX,
        fragsize: bytes,
    };
    let stream_name = match source {
        CaptureSource::SystemLoopback(_) => "System audio",
        CaptureSource::Application { .. } => "App audio",
        _ => "Microphone",
    };
    let simple = Simple::new(
        None,
        APP_NAME,
        Direction::Record,
        device.as_deref(),
        stream_name,
        &spec,
        None,
        Some(&attr),
    )
    .map_err(|e| open_error(e, device.as_deref()));
    let simple = match simple {
        Ok(simple) => simple,
        Err(e) => {
            if let Some(routing) = routing.take() {
                routing.close(&mut pulse);
            }
            return Err(e);
        }
    };
    drop(pulse);
    info!(
        device = device.as_deref().unwrap_or("default"),
        channels, "opening capture (PulseAudio)"
    );

    let info = StreamInfo {
        device_sample_rate: RATE,
        device_channels: channels,
        channels,
        latency_ms: CAPTURE_CHUNK_MS,
    };
    spawn("sp-audio-capture", info, move |stop| {
        // Per-app capture moves streams the app opens later; the connection belongs to this thread.
        let mut events = routing.as_ref().and_then(|_| {
            follow_sink_inputs()
                .inspect_err(|e| warn!(error = %e, "app capture will not follow new streams"))
                .ok()
        });
        let mut raw = vec![0u8; samples * 4];
        let mut frames = vec![0f32; samples];
        while !stop.load(Ordering::Relaxed) {
            if let Err(e) = simple.read(&mut raw) {
                if !stop.load(Ordering::Relaxed) {
                    warn!(error = %e, "capture stream error");
                    on_error(AudioError::DeviceLost);
                }
                break;
            }
            for (sample, b) in frames.iter_mut().zip(raw.chunks_exact(4)) {
                *sample = f32::from_ne_bytes([b[0], b[1], b[2], b[3]]);
            }
            on_audio(&frames);
            if let (Some(routing), Some((pulse, pending))) = (routing.as_mut(), events.as_mut()) {
                let followed = pulse.dispatch(false).and_then(|()| {
                    if pending.replace(false) {
                        routing.route(pulse)
                    } else {
                        Ok(())
                    }
                });
                if let Err(e) = followed {
                    warn!(error = %e, "app capture stopped following new streams");
                    events = None;
                }
            }
        }
        // Stop recording before the capture sink goes away.
        drop(simple);
        if let Some(routing) = routing {
            match events
                .map(|(pulse, _)| pulse)
                .map_or_else(Pulse::connect, Ok)
            {
                Ok(mut pulse) => routing.close(&mut pulse),
                Err(e) => warn!(error = %e, "could not remove app capture"),
            }
        }
    })
}

pub fn open_render(
    target: &RenderTarget,
    channels: u16,
    mut on_audio: RenderCallback,
    mut on_error: ErrorCallback,
) -> Result<Box<dyn AudioStream>, AudioError> {
    let spec = spec(channels)?;
    let device = render_device(&mut Pulse::connect()?, target)?;
    let frame_bytes = 4 * usize::from(channels);
    let samples = (RATE * RENDER_CHUNK_MS / 1000) as usize * usize::from(channels);
    let buffer = (RATE * RENDER_BUFFER_MS / 1000) as usize * frame_bytes;
    let attr = BufferAttr {
        maxlength: u32::MAX,
        tlength: u32::try_from(buffer).unwrap_or(u32::MAX),
        prebuf: u32::MAX,
        minreq: u32::MAX,
        fragsize: u32::MAX,
    };
    let simple = Simple::new(
        None,
        APP_NAME,
        Direction::Playback,
        device.as_deref(),
        "Playback",
        &spec,
        None,
        Some(&attr),
    )
    .map_err(|e| open_error(e, device.as_deref()))?;
    info!(
        device = device.as_deref().unwrap_or("default"),
        channels, "opening render (PulseAudio)"
    );

    let info = StreamInfo {
        device_sample_rate: RATE,
        device_channels: channels,
        channels,
        latency_ms: RENDER_BUFFER_MS,
    };
    spawn("sp-audio-render", info, move |stop| {
        let mut frames = vec![0f32; samples];
        let mut raw = vec![0u8; samples * 4];
        while !stop.load(Ordering::Relaxed) {
            frames.fill(0.0);
            on_audio(&mut frames);
            for (b, sample) in raw.chunks_exact_mut(4).zip(frames.iter()) {
                b.copy_from_slice(&sample.to_ne_bytes());
            }
            // Blocks until the server has room, which paces the loop in real time.
            if let Err(e) = simple.write(&raw) {
                if !stop.load(Ordering::Relaxed) {
                    warn!(error = %e, "render stream error");
                    on_error(AudioError::DeviceLost);
                }
                break;
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(name: &str, description: &str) -> Device {
        Device {
            name: name.into(),
            description: description.into(),
            index: 0,
            channels: 2,
            sample_rate: 48_000,
            monitor: None,
            is_monitor: false,
        }
    }

    #[test]
    fn devices_are_found_by_description_then_name() {
        let devices = [
            device(
                "alsa_output.pci-0000_00_1f.3.analog-stereo",
                "Built-in Audio Analog Stereo",
            ),
            device("soundpush_microphone_feed", "SoundPush Microphone Feed"),
        ];
        assert_eq!(
            find(&devices, "SoundPush Microphone Feed").map(|d| d.name.as_str()),
            Some("soundpush_microphone_feed")
        );
        assert_eq!(
            find(&devices, "alsa_output.pci-0000_00_1f.3.analog-stereo")
                .map(|d| d.description.as_str()),
            Some("Built-in Audio Analog Stereo")
        );
        assert!(find(&devices, "HDMI").is_none());
    }

    #[test]
    fn missing_server_is_an_error() {
        assert!(Pulse::connect_to(Some("unix:/nonexistent/soundpush-test")).is_err());
    }

    #[test]
    fn stream_spec_rejects_impossible_channel_counts() {
        assert!(spec(2).is_ok());
        assert!(spec(0).is_err());
        assert!(spec(1000).is_err());
    }

    #[test]
    fn speakers_sink_remembers_the_real_speakers() {
        let argument = format!(
            "sink_name={SPEAKERS_SINK} rate=48000 sink_properties=\"device.description='SoundPush Speakers' {RESTORE_KEY}='alsa_output.pci-0000_00_1f.3.analog-stereo'\""
        );
        assert!(has_argument(&argument, "sink_name=soundpush_speakers"));
        assert_eq!(
            restore_sink(&argument).as_deref(),
            Some("alsa_output.pci-0000_00_1f.3.analog-stereo")
        );
        assert_eq!(restore_sink("sink_name=soundpush_speakers"), None);
        assert_eq!(restore_sink(&format!("{RESTORE_KEY}='unterminated")), None);
        assert!(!plain_name("bad name"));
        assert!(!plain_name("it's"));
        assert!(plain_name("bluez_output.00:11:22:33:44:55.1"));
    }

    #[test]
    fn soundpush_sinks_are_hidden_and_keep_their_streams() {
        assert!(hidden_sink(SPEAKERS_SINK));
        assert!(hidden_sink("soundpush_app_capture_42_0"));
        assert!(!hidden_sink("soundpush_microphone_feed"));
        assert!(own_sink("soundpush_microphone_feed"));
        assert!(own_sink("soundpush_app_capture_42_0"));
        assert!(!own_sink(SPEAKERS_SINK));
        assert!(!own_sink("alsa_output.usb-headset"));
    }

    #[test]
    fn app_capture_sinks_name_their_process() {
        assert_eq!(app_sink_pid("soundpush_app_capture_4242_3"), Some(4242));
        assert_eq!(app_sink_pid("soundpush_speakers"), None);
        assert!(process_running(std::process::id()));
        assert!(same_app("Firefox", "firefox"));
        assert!(!same_app("", ""));
        assert!(!same_app("firefox", "firefox-bin"));
    }
}
