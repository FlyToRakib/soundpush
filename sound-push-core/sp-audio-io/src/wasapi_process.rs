//! Windows per-app audio capture with WASAPI process loopback (Windows 10 version 2004+).
//!
//! `AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK` records what one process tree plays, or
//! everything except one process tree. Windows delivers 48 kHz float stereo in the requested
//! format, so only the channel count is adapted here. A dedicated thread owns the COM objects;
//! the returned handle stops it on drop.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::thread::JoinHandle;
use std::time::Duration;

use tracing::{info, warn};
use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows::Win32::Media::Audio::{
    AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
    AUDCLNT_STREAMFLAGS_EVENTCALLBACK, AUDCLNT_STREAMFLAGS_LOOPBACK, AUDIOCLIENT_ACTIVATION_PARAMS,
    AUDIOCLIENT_ACTIVATION_PARAMS_0, AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK, AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS,
    ActivateAudioInterfaceAsync, DEVICE_STATE_ACTIVE, IActivateAudioInterfaceAsyncOperation,
    IActivateAudioInterfaceCompletionHandler, IActivateAudioInterfaceCompletionHandler_Impl, IAudioCaptureClient,
    IAudioClient, IAudioSessionControl2, IAudioSessionManager2, IMMDeviceEnumerator, MMDeviceEnumerator,
    PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE, PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE,
    VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK, WAVEFORMATEX, eRender,
};
use windows::Win32::System::Com::{CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
use windows::core::{GUID, HRESULT, IUnknown, Interface, PROPVARIANT, implement, w};

use crate::convert::CaptureConverter;
use crate::{AudioError, AudioStream, CaptureCallback, ErrorCallback, StreamInfo};

/// First Windows build with process loopback (Windows 10 version 2004).
const MIN_BUILD: u32 = 19041;
const RATE: u32 = 48_000;
/// 20 ms, in 100-nanosecond units.
const BUFFER_DURATION: i64 = 200_000;
/// `VT_BLOB`.
const VT_BLOB: u16 = 65;
/// `WAVE_FORMAT_IEEE_FLOAT`.
const WAVE_FORMAT_IEEE_FLOAT: u16 = 3;

/// An app that has an audio session on a playback device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioApp {
    /// Executable name, e.g. "spotify.exe" (what capture settings store).
    pub process: String,
    /// The app is playing sound right now.
    pub active: bool,
}

/// Whether this Windows version supports per-app capture.
pub fn process_loopback_supported() -> bool {
    windows_build().is_some_and(|b| b >= MIN_BUILD)
}

fn windows_build() -> Option<u32> {
    let mut buf = [0u16; 32];
    let mut size = (buf.len() * 2) as u32;
    // SAFETY: the buffer and its size in bytes are valid for the call.
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            w!(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion"),
            w!("CurrentBuildNumber"),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    if status.is_err() {
        return None;
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len]).trim().parse().ok()
}

/// Runs `f` with COM initialised on this thread (multithreaded apartment).
fn with_com<T>(f: impl FnOnce() -> T) -> T {
    // SAFETY: balanced with CoUninitialize below when initialisation succeeded.
    let init = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    let result = f();
    if init.is_ok() {
        // SAFETY: matches the successful CoInitializeEx above.
        unsafe { CoUninitialize() };
    }
    result
}

struct ProcessEntry {
    pid: u32,
    parent: u32,
    exe: String,
}

fn processes() -> Vec<ProcessEntry> {
    let mut out = Vec::new();
    // SAFETY: the snapshot handle is closed below; the entry has its size set as required.
    unsafe {
        let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return out;
        };
        let mut entry = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut ok = Process32FirstW(snapshot, &mut entry).is_ok();
        while ok {
            let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
            out.push(ProcessEntry {
                pid: entry.th32ProcessID,
                parent: entry.th32ParentProcessID,
                exe: String::from_utf16_lossy(&entry.szExeFile[..len]),
            });
            ok = Process32NextW(snapshot, &mut entry).is_ok();
        }
        let _ = CloseHandle(snapshot);
    }
    out
}

/// The root of the process tree running `exe` (the browser, not one of its tab processes).
fn find_process(exe: &str) -> Option<u32> {
    let all = processes();
    let named = |p: &&ProcessEntry| p.exe.eq_ignore_ascii_case(exe);
    all.iter()
        .filter(named)
        .find(|p| !all.iter().filter(named).any(|q| q.pid == p.parent))
        .or_else(|| all.iter().find(named))
        .map(|p| p.pid)
}

/// Apps with an audio session on any active playback device, playing ones first.
/// SoundPush itself and the system sounds session are left out.
pub fn audio_apps() -> Vec<AudioApp> {
    let own = std::process::id();
    let names: std::collections::HashMap<u32, String> = processes().into_iter().map(|p| (p.pid, p.exe)).collect();
    let mut apps: Vec<AudioApp> = Vec::new();
    with_com(|| {
        // SAFETY: COM calls on objects created and released within this scope.
        let result: windows::core::Result<()> = unsafe {
            (|| {
                let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
                let devices = enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)?;
                for i in 0..devices.GetCount()? {
                    let Ok(manager) =
                        devices.Item(i).and_then(|d| d.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None))
                    else {
                        continue;
                    };
                    let sessions = manager.GetSessionEnumerator()?;
                    for j in 0..sessions.GetCount()? {
                        let Ok(session) = sessions.GetSession(j).and_then(|s| s.cast::<IAudioSessionControl2>()) else {
                            continue;
                        };
                        let pid = session.GetProcessId().unwrap_or(0);
                        if pid == 0 || pid == own || session.IsSystemSoundsSession() == windows::Win32::Foundation::S_OK
                        {
                            continue;
                        }
                        let Some(exe) = names.get(&pid) else { continue };
                        let active = session
                            .GetState()
                            .is_ok_and(|s| s == windows::Win32::Media::Audio::AudioSessionStateActive);
                        match apps.iter_mut().find(|a| a.process.eq_ignore_ascii_case(exe)) {
                            Some(app) => app.active |= active,
                            None => apps.push(AudioApp { process: exe.clone(), active }),
                        }
                    }
                }
                Ok(())
            })()
        };
        if let Err(e) = result {
            warn!(error = %e, "could not list apps playing audio");
        }
    });
    apps.sort_by(|a, b| b.active.cmp(&a.active).then_with(|| a.process.to_lowercase().cmp(&b.process.to_lowercase())));
    apps
}

/// Signals the waiting thread when Windows finishes activating the audio client.
#[implement(IActivateAudioInterfaceCompletionHandler)]
struct Completion(Arc<(Mutex<bool>, Condvar)>);

impl IActivateAudioInterfaceCompletionHandler_Impl for Completion_Impl {
    fn ActivateCompleted(
        &self,
        _operation: Option<&IActivateAudioInterfaceAsyncOperation>,
    ) -> windows::core::Result<()> {
        let (done, signal) = &*self.0;
        if let Ok(mut done) = done.lock() {
            *done = true;
        }
        signal.notify_all();
        Ok(())
    }
}

/// `PROPVARIANT` holding a `VT_BLOB` (same layout as the Windows struct on every architecture).
#[repr(C)]
struct BlobVariant {
    vt: u16,
    reserved: [u16; 3],
    size: u32,
    data: *const u8,
}

fn backend(e: windows::core::Error) -> AudioError {
    AudioError::Backend(e.message())
}

fn activate(pid: u32, exclude: bool) -> Result<IAudioClient, AudioError> {
    let params = AUDIOCLIENT_ACTIVATION_PARAMS {
        ActivationType: AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
        Anonymous: AUDIOCLIENT_ACTIVATION_PARAMS_0 {
            ProcessLoopbackParams: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS {
                TargetProcessId: pid,
                ProcessLoopbackMode: if exclude {
                    PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE
                } else {
                    PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE
                },
            },
        },
    };
    let variant = BlobVariant {
        vt: VT_BLOB,
        reserved: [0; 3],
        size: std::mem::size_of::<AUDIOCLIENT_ACTIVATION_PARAMS>() as u32,
        data: std::ptr::from_ref(&params).cast(),
    };
    let state = Arc::new((Mutex::new(false), Condvar::new()));
    let handler: IActivateAudioInterfaceCompletionHandler = Completion(state.clone()).into();
    // SAFETY: `variant` and `params` outlive the activation, which is awaited below.
    let operation = unsafe {
        ActivateAudioInterfaceAsync(
            VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,
            &IAudioClient::IID,
            Some(std::ptr::from_ref(&variant).cast::<PROPVARIANT>()),
            &handler,
        )
    }
    .map_err(backend)?;

    let (done, signal) = &*state;
    let guard = done.lock().map_err(|_| AudioError::Backend("activation lock poisoned".into()))?;
    let (guard, _) = signal
        .wait_timeout_while(guard, Duration::from_secs(5), |done| !*done)
        .map_err(|_| AudioError::Backend("activation lock poisoned".into()))?;
    if !*guard {
        return Err(AudioError::Backend("per-app capture did not start in time".into()));
    }
    drop(guard);

    let mut result = HRESULT(0);
    let mut unknown: Option<IUnknown> = None;
    // SAFETY: both out pointers are valid; activation has completed.
    unsafe { operation.GetActivateResult(&mut result, &mut unknown) }.map_err(backend)?;
    result.ok().map_err(backend)?;
    unknown.ok_or_else(|| AudioError::Backend("no audio client".into()))?.cast::<IAudioClient>().map_err(backend)
}

struct ProcessStream {
    running: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    info: StreamInfo,
}

impl AudioStream for ProcessStream {
    fn info(&self) -> StreamInfo {
        self.info
    }
}

impl Drop for ProcessStream {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Start capturing `process` (or everything but it). Fails with `LoopbackUnsupported` on older
/// Windows and `DeviceNotFound` when the app is not running.
pub fn open(
    process: &str,
    exclude: bool,
    channels: u16,
    on_audio: CaptureCallback,
    on_error: ErrorCallback,
) -> Result<Box<dyn AudioStream>, AudioError> {
    if !process_loopback_supported() {
        return Err(AudioError::LoopbackUnsupported);
    }
    let pid = find_process(process).ok_or_else(|| AudioError::DeviceNotFound(process.to_string()))?;
    let running = Arc::new(AtomicBool::new(true));
    let (ready_tx, ready_rx) = mpsc::channel();
    let thread = {
        let running = running.clone();
        let process = process.to_string();
        std::thread::Builder::new()
            .name("sp-app-capture".into())
            .spawn(move || {
                with_com(|| capture_thread(&process, pid, exclude, channels, &running, &ready_tx, on_audio, on_error));
            })
            .map_err(|e| AudioError::Backend(e.to_string()))?
    };
    match ready_rx.recv() {
        Ok(Ok(())) => Ok(Box::new(ProcessStream {
            running,
            thread: Some(thread),
            info: StreamInfo {
                device_sample_rate: RATE,
                device_channels: 2,
                channels,
                latency_ms: (BUFFER_DURATION / 10_000) as u32,
            },
        })),
        Ok(Err(e)) => {
            let _ = thread.join();
            Err(e)
        }
        Err(_) => Err(AudioError::Backend("capture thread exited".into())),
    }
}

struct EventHandle(HANDLE);

impl Drop for EventHandle {
    fn drop(&mut self) {
        // SAFETY: the handle came from CreateEventW and is closed once.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

#[allow(clippy::too_many_arguments)]
fn capture_thread(
    process: &str,
    pid: u32,
    exclude: bool,
    channels: u16,
    running: &AtomicBool,
    ready: &mpsc::Sender<Result<(), AudioError>>,
    mut on_audio: CaptureCallback,
    mut on_error: ErrorCallback,
) {
    let setup = || -> Result<(IAudioClient, IAudioCaptureClient, EventHandle), AudioError> {
        let client = activate(pid, exclude)?;
        let format = WAVEFORMATEX {
            wFormatTag: WAVE_FORMAT_IEEE_FLOAT,
            nChannels: 2,
            nSamplesPerSec: RATE,
            nAvgBytesPerSec: RATE * 8,
            nBlockAlign: 8,
            wBitsPerSample: 32,
            cbSize: 0,
        };
        // SAFETY: plain COM calls on a live client; `format` outlives Initialize.
        unsafe {
            client
                .Initialize(
                    AUDCLNT_SHAREMODE_SHARED,
                    AUDCLNT_STREAMFLAGS_LOOPBACK
                        | AUDCLNT_STREAMFLAGS_EVENTCALLBACK
                        | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
                    BUFFER_DURATION,
                    0,
                    &format,
                    None::<*const GUID>,
                )
                .map_err(backend)?;
            let event = EventHandle(CreateEventW(None, false, false, None).map_err(backend)?);
            client.SetEventHandle(event.0).map_err(backend)?;
            let capture: IAudioCaptureClient = client.GetService().map_err(backend)?;
            client.Start().map_err(backend)?;
            Ok((client, capture, event))
        }
    };
    let (client, capture, event) = match setup() {
        Ok(parts) => parts,
        Err(e) => {
            let _ = ready.send(Err(e));
            return;
        }
    };
    info!(process, exclude, "per-app capture started");
    let _ = ready.send(Ok(()));

    let mut converter = CaptureConverter::new(RATE, 2, channels);
    let mut scratch: Vec<f32> = Vec::with_capacity(RATE as usize);
    while running.load(Ordering::Relaxed) {
        // SAFETY: the event handle is valid for the life of this loop.
        if unsafe { WaitForSingleObject(event.0, 100) } != WAIT_OBJECT_0 {
            continue;
        }
        // SAFETY: GetBuffer/ReleaseBuffer are paired; the buffer is read only in between.
        let drained: windows::core::Result<()> = unsafe {
            (|| {
                while capture.GetNextPacketSize()? > 0 {
                    let mut data = std::ptr::null_mut();
                    let mut frames = 0u32;
                    let mut flags = 0u32;
                    capture.GetBuffer(&mut data, &mut frames, &mut flags, None, None)?;
                    let samples = frames as usize * 2;
                    scratch.clear();
                    if data.is_null() || flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 {
                        scratch.resize(samples, 0.0);
                    } else {
                        let bytes = std::slice::from_raw_parts(data, samples * 4);
                        scratch.extend(bytes.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])));
                    }
                    capture.ReleaseBuffer(frames)?;
                    on_audio(converter.process(&scratch));
                }
                Ok(())
            })()
        };
        if let Err(e) = drained {
            warn!(error = %e, "per-app capture failed");
            on_error(AudioError::DeviceLost);
            break;
        }
    }
    // SAFETY: stopping a started client.
    let _ = unsafe { client.Stop() };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blob_variant_matches_propvariant_layout() {
        assert_eq!(std::mem::size_of::<BlobVariant>(), std::mem::size_of::<PROPVARIANT>());
    }

    #[test]
    fn finds_this_process_and_reads_the_build() {
        let exe = std::env::current_exe().ok().and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()));
        if let Some(exe) = exe {
            assert!(find_process(&exe).is_some());
        }
        assert!(windows_build().is_some());
        assert_eq!(find_process("definitely-not-running-soundpush.exe"), None);
    }
}
