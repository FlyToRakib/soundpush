//! Cross-platform backend on top of `cpal` (CoreAudio, WASAPI, ALSA, AAudio/OpenSL ES).
//!
//! cpal streams are not `Send` on every platform, so each stream is created and
//! owned by a dedicated thread; the returned handle stops it on drop.
//! System-audio loopback is available on Windows (WASAPI loopback on an output
//! device). Other platforms report [`AudioBackend::supports_loopback`] = false
//! until their native capture backends (process taps, PipeWire monitors) land.

use std::sync::mpsc;
use std::thread::JoinHandle;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, Host, SampleFormat, StreamConfig};
use tracing::{info, warn};

use crate::convert::{CaptureConverter, RenderConverter};
use crate::{
    AudioBackend, AudioError, AudioStream, CaptureCallback, CaptureSource, DeviceInfo, DeviceKind, ErrorCallback,
    RenderCallback, RenderTarget, StreamInfo,
};

pub struct CpalBackend;

impl CpalBackend {
    pub fn new() -> Self {
        Self
    }
}

impl Default for CpalBackend {
    fn default() -> Self {
        Self::new()
    }
}

fn host() -> Host {
    cpal::default_host()
}

fn find_device(host: &Host, kind: DeviceKind, id: Option<&str>) -> Result<Device, AudioError> {
    match id {
        None => match kind {
            DeviceKind::Input => host.default_input_device(),
            DeviceKind::Output => host.default_output_device(),
        }
        .ok_or(AudioError::NoDefaultDevice),
        Some(id) => {
            let mut devices = match kind {
                DeviceKind::Input => host.input_devices(),
                DeviceKind::Output => host.output_devices(),
            }
            .map_err(|e| AudioError::Backend(e.to_string()))?;
            devices
                .find(|d| d.name().map(|n| n == id).unwrap_or(false))
                .ok_or_else(|| AudioError::DeviceNotFound(id.to_string()))
        }
    }
}

/// A just-created aggregate device takes a moment to appear in the device list.
#[cfg(target_os = "macos")]
fn find_input_when_ready(host: &Host, name: &str) -> Result<Device, AudioError> {
    for _ in 0..40 {
        if let Ok(d) = find_device(host, DeviceKind::Input, Some(name)) {
            return Ok(d);
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    Err(AudioError::DeviceNotFound(name.to_string()))
}

fn map_build_error(e: cpal::BuildStreamError) -> AudioError {
    match e {
        cpal::BuildStreamError::DeviceNotAvailable => AudioError::DeviceLost,
        cpal::BuildStreamError::StreamConfigNotSupported => AudioError::FormatUnsupported("stream config".into()),
        other => {
            let text = other.to_string();
            if text.to_lowercase().contains("permission") || text.contains("denied") {
                AudioError::PermissionDenied
            } else if text.to_lowercase().contains("in use") || text.contains("exclusive") {
                AudioError::DeviceBusy
            } else {
                AudioError::Backend(text)
            }
        }
    }
}

struct ThreadStream {
    stop: Option<mpsc::Sender<()>>,
    thread: Option<JoinHandle<()>>,
    info: StreamInfo,
}

impl AudioStream for ThreadStream {
    fn info(&self) -> StreamInfo {
        self.info
    }
}

impl Drop for ThreadStream {
    fn drop(&mut self) {
        drop(self.stop.take());
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Run `build` on a dedicated thread that owns the resulting cpal stream.
/// Resources that must outlive a stream (e.g. the macOS tap device it reads from).
type StreamGuard = Option<Box<dyn std::any::Any>>;

fn spawn_stream<F>(build: F) -> Result<Box<dyn AudioStream>, AudioError>
where
    F: FnOnce() -> Result<(cpal::Stream, StreamInfo, StreamGuard), AudioError> + Send + 'static,
{
    let (ready_tx, ready_rx) = mpsc::channel();
    let (stop_tx, stop_rx) = mpsc::channel::<()>();
    let thread = std::thread::Builder::new()
        .name("sp-audio".into())
        .spawn(move || match build() {
            Ok((stream, info, guard)) => {
                if let Err(e) = stream.play() {
                    let _ = ready_tx.send(Err(AudioError::Backend(e.to_string())));
                    return;
                }
                let _ = ready_tx.send(Ok(info));
                // Block until the handle is dropped.
                let _ = stop_rx.recv();
                // Stop the stream before releasing what it reads from.
                drop(stream);
                drop(guard);
            }
            Err(e) => {
                let _ = ready_tx.send(Err(e));
            }
        })
        .map_err(|e| AudioError::Backend(e.to_string()))?;
    let info = ready_rx
        .recv()
        .map_err(|_| AudioError::Backend("audio thread exited".into()))??;
    Ok(Box::new(ThreadStream {
        stop: Some(stop_tx),
        thread: Some(thread),
        info,
    }))
}

impl CpalBackend {
    /// Names of the playback devices only. Unlike [`AudioBackend::list_devices`] this never
    /// inspects an input device: on macOS reading a microphone's stream format counts as
    /// microphone access, so a periodic check must not touch inputs or the permission prompt
    /// appears again and again.
    pub fn output_device_names(&self) -> Vec<String> {
        host()
            .output_devices()
            .map(|devices| devices.filter_map(|d| d.name().ok()).collect())
            .unwrap_or_default()
    }

    /// Name of the default playback device, without touching any input (see above).
    pub fn default_output_name(&self) -> Option<String> {
        host().default_output_device().and_then(|d| d.name().ok())
    }
}

impl AudioBackend for CpalBackend {
    fn name(&self) -> &'static str {
        "cpal"
    }

    fn list_devices(&self) -> Result<Vec<DeviceInfo>, AudioError> {
        let host = host();
        let default_in = host.default_input_device().and_then(|d| d.name().ok());
        let default_out = host.default_output_device().and_then(|d| d.name().ok());
        // SoundPush's own system-audio capture device (macOS) is internal, never a user choice.
        let internal = |name: &str| name == "SoundPush System Audio";
        let mut out = Vec::new();
        if let Ok(devices) = host.input_devices() {
            for d in devices {
                let Ok(name) = d.name() else { continue };
                if internal(&name) {
                    continue;
                }
                let Ok(cfg) = d.default_input_config() else { continue };
                out.push(DeviceInfo {
                    id: name.clone(),
                    is_default: default_in.as_deref() == Some(&name),
                    name,
                    kind: DeviceKind::Input,
                    channels: cfg.channels(),
                    sample_rate: cfg.sample_rate().0,
                });
            }
        }
        if let Ok(devices) = host.output_devices() {
            for d in devices {
                let Ok(name) = d.name() else { continue };
                if internal(&name) {
                    continue;
                }
                let Ok(cfg) = d.default_output_config() else { continue };
                out.push(DeviceInfo {
                    id: name.clone(),
                    is_default: default_out.as_deref() == Some(&name),
                    name,
                    kind: DeviceKind::Output,
                    channels: cfg.channels(),
                    sample_rate: cfg.sample_rate().0,
                });
            }
        }
        Ok(out)
    }

    fn supports_loopback(&self) -> bool {
        // Windows: WASAPI loopback. macOS: Core Audio process taps (14.2+).
        cfg!(any(target_os = "windows", target_os = "macos"))
    }

    fn open_capture(
        &self,
        source: &CaptureSource,
        channels: u16,
        mut on_audio: CaptureCallback,
        mut on_error: ErrorCallback,
    ) -> Result<Box<dyn AudioStream>, AudioError> {
        if matches!(source, CaptureSource::SystemLoopback(_)) && !self.supports_loopback() {
            return Err(AudioError::LoopbackUnsupported);
        }
        if let CaptureSource::Application { process, exclude } = source {
            // Per-app capture bypasses cpal: it needs WASAPI process loopback.
            #[cfg(windows)]
            return crate::wasapi_process::open(process, *exclude, channels, on_audio, on_error);
            #[cfg(not(windows))]
            {
                let _ = (process, exclude);
                return Err(AudioError::LoopbackUnsupported);
            }
        }
        let source = source.clone();
        spawn_stream(move || {
            let host = host();
            #[allow(unused_mut)]
            let mut guard: StreamGuard = None;
            let (device, supported) = match &source {
                CaptureSource::DefaultInput => {
                    let d = find_device(&host, DeviceKind::Input, None)?;
                    let c = d.default_input_config().map_err(|e| AudioError::Backend(e.to_string()))?;
                    (d, c)
                }
                CaptureSource::Input(id) => {
                    let d = find_device(&host, DeviceKind::Input, Some(id))?;
                    let c = d.default_input_config().map_err(|e| AudioError::Backend(e.to_string()))?;
                    (d, c)
                }
                #[cfg(target_os = "macos")]
                CaptureSource::SystemLoopback(_) => {
                    // Process tap wrapped in a temporary aggregate input device.
                    let tap = crate::macos_tap::SystemTap::create().map_err(AudioError::Backend)?;
                    let d = find_input_when_ready(&host, crate::macos_tap::TAP_DEVICE_NAME)?;
                    let c = d.default_input_config().map_err(|e| AudioError::Backend(e.to_string()))?;
                    guard = Some(Box::new(tap));
                    (d, c)
                }
                #[cfg(not(target_os = "macos"))]
                CaptureSource::SystemLoopback(id) => {
                    // WASAPI: building an input stream on an output device captures its mix.
                    let d = find_device(&host, DeviceKind::Output, id.as_deref())?;
                    let c = d.default_output_config().map_err(|e| AudioError::Backend(e.to_string()))?;
                    (d, c)
                }
                // Handled before the stream thread starts.
                CaptureSource::Application { .. } => return Err(AudioError::LoopbackUnsupported),
            };
            let device_rate = supported.sample_rate().0;
            let device_channels = supported.channels();
            let format = supported.sample_format();
            let config: StreamConfig = supported.into();
            info!(device = %device.name().unwrap_or_default(), device_rate, device_channels, ?format, "opening capture");

            let mut converter = CaptureConverter::new(device_rate, device_channels, channels);
            let mut scratch: Vec<f32> = Vec::with_capacity(16_384);
            let err_fn = move |e: cpal::StreamError| {
                warn!(error = %e, "capture stream error");
                on_error(match e {
                    cpal::StreamError::DeviceNotAvailable => AudioError::DeviceLost,
                    other => AudioError::Backend(other.to_string()),
                });
            };

            let stream = match format {
                SampleFormat::F32 => device.build_input_stream(
                    &config,
                    move |data: &[f32], _| on_audio(converter.process(data)),
                    err_fn,
                    None,
                ),
                SampleFormat::I16 => device.build_input_stream(
                    &config,
                    move |data: &[i16], _| {
                        scratch.clear();
                        scratch.extend(data.iter().map(|&s| s as f32 / 32768.0));
                        on_audio(converter.process(&scratch));
                    },
                    err_fn,
                    None,
                ),
                SampleFormat::I32 => device.build_input_stream(
                    &config,
                    move |data: &[i32], _| {
                        scratch.clear();
                        scratch.extend(data.iter().map(|&s| s as f32 / 2_147_483_648.0));
                        on_audio(converter.process(&scratch));
                    },
                    err_fn,
                    None,
                ),
                other => return Err(AudioError::FormatUnsupported(format!("{other:?}"))),
            }
            .map_err(map_build_error)?;

            let latency_ms = match config.buffer_size {
                cpal::BufferSize::Fixed(frames) => frames * 1000 / device_rate,
                cpal::BufferSize::Default => 10,
            };
            Ok((
                stream,
                StreamInfo {
                    device_sample_rate: device_rate,
                    device_channels,
                    channels,
                    latency_ms,
                },
                guard,
            ))
        })
    }

    fn open_render(
        &self,
        target: &RenderTarget,
        channels: u16,
        mut on_audio: RenderCallback,
        mut on_error: ErrorCallback,
    ) -> Result<Box<dyn AudioStream>, AudioError> {
        let target = target.clone();
        spawn_stream(move || {
            let host = host();
            let guard: StreamGuard = None;
            let device = match &target {
                RenderTarget::DefaultOutput => find_device(&host, DeviceKind::Output, None)?,
                RenderTarget::Output(id) => find_device(&host, DeviceKind::Output, Some(id))?,
            };
            let supported = device
                .default_output_config()
                .map_err(|e| AudioError::Backend(e.to_string()))?;
            let device_rate = supported.sample_rate().0;
            let device_channels = supported.channels();
            let format = supported.sample_format();
            let config: StreamConfig = supported.into();
            info!(device = %device.name().unwrap_or_default(), device_rate, device_channels, ?format, "opening render");

            let mut converter = RenderConverter::new(device_rate, device_channels, channels);
            let mut scratch: Vec<f32> = Vec::with_capacity(16_384);
            let err_fn = move |e: cpal::StreamError| {
                warn!(error = %e, "render stream error");
                on_error(match e {
                    cpal::StreamError::DeviceNotAvailable => AudioError::DeviceLost,
                    other => AudioError::Backend(other.to_string()),
                });
            };

            let stream = match format {
                SampleFormat::F32 => device.build_output_stream(
                    &config,
                    move |data: &mut [f32], _| converter.fill(data, &mut on_audio),
                    err_fn,
                    None,
                ),
                SampleFormat::I16 => device.build_output_stream(
                    &config,
                    move |data: &mut [i16], _| {
                        scratch.clear();
                        scratch.resize(data.len(), 0.0);
                        converter.fill(&mut scratch, &mut on_audio);
                        for (d, s) in data.iter_mut().zip(scratch.iter()) {
                            *d = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
                        }
                    },
                    err_fn,
                    None,
                ),
                SampleFormat::I32 => device.build_output_stream(
                    &config,
                    move |data: &mut [i32], _| {
                        scratch.clear();
                        scratch.resize(data.len(), 0.0);
                        converter.fill(&mut scratch, &mut on_audio);
                        for (d, s) in data.iter_mut().zip(scratch.iter()) {
                            *d = (s.clamp(-1.0, 1.0) as f64 * 2_147_483_647.0) as i32;
                        }
                    },
                    err_fn,
                    None,
                ),
                other => return Err(AudioError::FormatUnsupported(format!("{other:?}"))),
            }
            .map_err(map_build_error)?;

            let latency_ms = match config.buffer_size {
                cpal::BufferSize::Fixed(frames) => frames * 1000 / device_rate,
                cpal::BufferSize::Default => 10,
            };
            Ok((
                stream,
                StreamInfo {
                    device_sample_rate: device_rate,
                    device_channels,
                    channels,
                    latency_ms,
                },
                guard,
            ))
        })
    }
}
