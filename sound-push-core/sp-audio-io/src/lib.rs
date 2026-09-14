//! Audio device I/O.
//!
//! Every backend delivers and accepts **48 kHz interleaved `f32`** with the
//! channel count requested by the caller. Format conversion (resampling,
//! down/up-mixing, sample format) happens inside the backend so the rest of the
//! engine never sees device formats.
//!
//! Callbacks run on real-time audio threads: they must not block or allocate.

#[cfg(feature = "cpal-backend")]
pub mod cpal_backend;
mod convert;
#[cfg(all(target_os = "macos", feature = "cpal-backend"))]
pub mod macos_tap;
pub mod null;
#[cfg(windows)]
pub mod wasapi_process;
#[cfg(all(target_os = "linux", feature = "pulse"))]
pub mod pulse;

pub use convert::{CaptureConverter, RenderConverter};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeviceKind {
    Input,
    Output,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    /// Stable identifier (the device name for cpal backends).
    pub id: String,
    pub name: String,
    pub kind: DeviceKind,
    pub is_default: bool,
    pub channels: u16,
    pub sample_rate: u32,
}

/// What to capture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureSource {
    /// The system default microphone.
    DefaultInput,
    /// A specific input device.
    Input(String),
    /// Everything played on an output device (default output when `None`).
    SystemLoopback(Option<String>),
    /// What one app plays (by executable name, e.g. "spotify.exe"), or everything except that
    /// app when `exclude` is set. Windows 10 version 2004+ (WASAPI process loopback).
    Application { process: String, exclude: bool },
}

/// Where to play.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderTarget {
    DefaultOutput,
    Output(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamInfo {
    /// Device-side sample rate.
    pub device_sample_rate: u32,
    /// Device-side channel count.
    pub device_channels: u16,
    /// Channels delivered to / expected from the callback.
    pub channels: u16,
    /// Estimated device buffer latency in milliseconds.
    pub latency_ms: u32,
}

/// A running stream. Dropping it stops the stream.
pub trait AudioStream: Send {
    fn info(&self) -> StreamInfo;
}

/// Capture callback: receives 48 kHz interleaved frames.
pub type CaptureCallback = Box<dyn FnMut(&[f32]) + Send + 'static>;
/// Render callback: fills 48 kHz interleaved frames.
pub type RenderCallback = Box<dyn FnMut(&mut [f32]) + Send + 'static>;
/// Called from the audio thread when the device fails (e.g. unplugged).
pub type ErrorCallback = Box<dyn FnMut(AudioError) + Send + 'static>;

pub trait AudioBackend: Send + Sync {
    /// Short backend name for diagnostics.
    fn name(&self) -> &'static str;
    fn list_devices(&self) -> Result<Vec<DeviceInfo>, AudioError>;
    /// Whether this backend can capture system output on this platform.
    fn supports_loopback(&self) -> bool;
    fn open_capture(
        &self,
        source: &CaptureSource,
        channels: u16,
        on_audio: CaptureCallback,
        on_error: ErrorCallback,
    ) -> Result<Box<dyn AudioStream>, AudioError>;
    fn open_render(
        &self,
        target: &RenderTarget,
        channels: u16,
        on_audio: RenderCallback,
        on_error: ErrorCallback,
    ) -> Result<Box<dyn AudioStream>, AudioError>;
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum AudioError {
    #[error("audio device not found: {0}")]
    DeviceNotFound(String),
    #[error("no default audio device")]
    NoDefaultDevice,
    #[error("audio device is busy or used exclusively by another app")]
    DeviceBusy,
    #[error("unsupported audio format: {0}")]
    FormatUnsupported(String),
    #[error("system audio capture is not supported on this platform")]
    LoopbackUnsupported,
    #[error("microphone permission denied")]
    PermissionDenied,
    #[error("audio device disconnected")]
    DeviceLost,
    #[error("audio backend error: {0}")]
    Backend(String),
}
