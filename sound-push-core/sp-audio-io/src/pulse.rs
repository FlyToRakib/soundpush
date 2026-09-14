//! Linux audio through the PulseAudio protocol. PipeWire desktops serve the same protocol with
//! pipewire-pulse, so this covers PipeWire (Ubuntu, Fedora, …) and PulseAudio-only systems.
//!
//! - Devices are sinks (outputs) and sources (inputs, sink monitors excluded), identified by
//!   their description: the name desktop sound settings show.
//! - System audio is recorded from the monitor source of an output.
//! - The server converts rate, format and channels, so streams are opened directly as 48 kHz
//!   `f32` with the caller's channel count.
//! - [`Pulse`] also lists and loads modules, for the desktop's virtual microphone.
//!
//! Every call fails with an error when no sound server runs; nothing here panics.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use libpulse_binding as pa;
use libpulse_simple_binding::Simple;
use pa::callbacks::ListResult;
use pa::context::{Context, FlagSet as ContextFlags, State as ContextState};
use pa::def::BufferAttr;
use pa::error::PAErr;
use pa::mainloop::standard::{IterateResult, Mainloop};
use pa::operation::{Operation, State as OperationState};
use pa::proplist::{Proplist, properties};
use pa::sample::{Format, Spec};
use pa::stream::Direction;
use tracing::{info, warn};

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

fn backend(message: impl Into<String>) -> AudioError {
    AudioError::Backend(message.into())
}

fn no_server() -> AudioError {
    backend("no PipeWire or PulseAudio sound server is running")
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
            IterateResult::Quit(_) | IterateResult::Err(_) => {
                return Err(backend("lost the sound server connection"));
            }
        }
        Ok(())
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
        self.wait(op)?;
        if ok.get() {
            Ok(())
        } else {
            Err(backend(format!(
                "the sound server could not unload module {index}"
            )))
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

/// A device by its id (description), or by its server name.
fn find<'a>(devices: &'a [Device], id: &str) -> Option<&'a Device> {
    devices
        .iter()
        .find(|d| d.description == id)
        .or_else(|| devices.iter().find(|d| d.name == id))
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
    out.extend(
        pulse
            .sinks()?
            .into_iter()
            .map(|d| info(d, DeviceKind::Output, &server.default_sink)),
    );
    Ok(out)
}

pub fn output_names() -> Result<Vec<String>, AudioError> {
    Ok(Pulse::connect()?
        .sinks()?
        .into_iter()
        .map(|d| d.description)
        .collect())
}

/// Server-side name of the source to record from; `None` for the default input.
fn capture_device(pulse: &mut Pulse, source: &CaptureSource) -> Result<Option<String>, AudioError> {
    match source {
        CaptureSource::DefaultInput => Ok(None),
        // Per-app capture is Windows-only for now (WASAPI process loopback).
        CaptureSource::Application { .. } => Err(AudioError::LoopbackUnsupported),
        CaptureSource::Input(id) => find(&pulse.sources()?, id)
            .map(|d| Some(d.name.clone()))
            .ok_or_else(|| AudioError::DeviceNotFound(id.clone())),
        CaptureSource::SystemLoopback(id) => {
            let sinks = pulse.sinks()?;
            let sink = match id {
                Some(id) => {
                    find(&sinks, id).ok_or_else(|| AudioError::DeviceNotFound(id.clone()))?
                }
                None => {
                    let default = pulse.server()?.default_sink;
                    match sinks.iter().find(|d| Some(&d.name) == default.as_ref()) {
                        Some(sink) => sink,
                        // PipeWire without a session manager reports "@DEFAULT_SINK@" rather than
                        // a sink name; the server resolves its own special monitor name.
                        None if !sinks.is_empty() => return Ok(Some("@DEFAULT_MONITOR@".into())),
                        None => return Err(AudioError::NoDefaultDevice),
                    }
                }
            };
            sink.monitor.clone().map(Some).ok_or_else(|| {
                AudioError::DeviceNotFound(format!("monitor of {}", sink.description))
            })
        }
    }
}

fn render_device(pulse: &mut Pulse, target: &RenderTarget) -> Result<Option<String>, AudioError> {
    match target {
        RenderTarget::DefaultOutput => Ok(None),
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

pub fn open_capture(
    source: &CaptureSource,
    channels: u16,
    mut on_audio: CaptureCallback,
    mut on_error: ErrorCallback,
) -> Result<Box<dyn AudioStream>, AudioError> {
    let spec = spec(channels)?;
    let device = capture_device(&mut Pulse::connect()?, source)?;
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
    .map_err(|e| open_error(e, device.as_deref()))?;
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
}
