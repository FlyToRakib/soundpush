//! OS integration: sleep prevention and muting the default output device.

use std::process::Child;
#[cfg(unix)]
use std::process::{Command, Stdio};

/// Keeps the system awake while alive.
pub enum SleepInhibitor {
    /// `SetThreadExecutionState` is per thread, so a dedicated thread holds the flag and clears
    /// it when told to stop. The sender is that signal; dropping it also stops the thread.
    #[cfg(windows)]
    Windows(std::sync::mpsc::Sender<()>),
    #[allow(dead_code)]
    Process(Child),
}

impl SleepInhibitor {
    pub fn acquire() -> Option<Self> {
        #[cfg(windows)]
        {
            use windows::Win32::System::Power::{
                ES_CONTINUOUS, ES_SYSTEM_REQUIRED, SetThreadExecutionState,
            };
            let (stop, stopped) = std::sync::mpsc::channel::<()>();
            let (ready, is_ready) = std::sync::mpsc::channel::<bool>();
            let thread = std::thread::Builder::new()
                .name("sp-keep-awake".into())
                .spawn(move || {
                    // SAFETY: plain Win32 calls with valid flags, on the thread that owns the state.
                    let prev =
                        unsafe { SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED) };
                    let _ = ready.send(prev.0 != 0);
                    // Blocks until the inhibitor is dropped.
                    let _ = stopped.recv();
                    // SAFETY: restores the default state on the same thread.
                    unsafe {
                        SetThreadExecutionState(ES_CONTINUOUS);
                    }
                });
            return match thread {
                Ok(_) if is_ready.recv().unwrap_or(false) => Some(Self::Windows(stop)),
                _ => None,
            };
        }
        #[cfg(target_os = "macos")]
        {
            // `-w` ends the assertion together with this process, even after a crash.
            return Command::new("caffeinate")
                .args(["-i", "-w", &std::process::id().to_string()])
                .spawn()
                .ok()
                .map(Self::Process);
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            // The inhibited child is `cat` on a pipe this process owns: it exits when the pipe
            // closes, so the inhibit ends with SoundPush even after a crash.
            return Command::new("systemd-inhibit")
                .args([
                    "--what=idle:sleep",
                    "--who=SoundPush",
                    "--why=Streaming audio",
                    "cat",
                ])
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .spawn()
                .ok()
                .map(Self::Process);
        }
        #[allow(unreachable_code)]
        None
    }
}

impl Drop for SleepInhibitor {
    fn drop(&mut self) {
        match self {
            #[cfg(windows)]
            Self::Windows(stop) => {
                let _ = stop.send(());
            }
            Self::Process(child) => {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
}

/// Mute or unmute the default playback device. Returns false if unsupported or failed.
pub fn set_default_output_muted(muted: bool) -> bool {
    #[cfg(windows)]
    {
        return windows_mute(muted).is_ok();
    }
    #[cfg(target_os = "macos")]
    {
        let script = format!("set volume output muted {muted}");
        return Command::new("osascript")
            .args(["-e", &script])
            .status()
            .is_ok_and(|s| s.success());
    }
    // PulseAudio mutes a sink's monitor together with the sink, which would also silence the
    // system audio being sent: playback moves to a "SoundPush Speakers" null sink instead (see
    // `pulse::mute_speakers`). PipeWire takes monitors before volume and mute
    // (`monitor.channel-volumes` is off by default), so the default sink itself is muted there.
    #[cfg(target_os = "linux")]
    {
        use sp_audio_io::pulse;
        let pipewire = pulse::Pulse::connect()
            .and_then(|mut p| p.server())
            .is_ok_and(|s| s.is_pipewire());
        if !pipewire {
            let result = if muted {
                pulse::mute_speakers()
            } else {
                pulse::unmute_speakers()
            };
            if let Err(e) = &result {
                tracing::warn!(error = %e, muted, "could not change the speakers");
            }
            return result.is_ok();
        }
        if !muted {
            // Speakers muted while the server was still PulseAudio.
            let _ = pulse::unmute_speakers();
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let flag = if muted { "1" } else { "0" };
        return Command::new("wpctl")
            .args(["set-mute", "@DEFAULT_AUDIO_SINK@", flag])
            .status()
            .is_ok_and(|s| s.success())
            || Command::new("pactl")
                .args(["set-sink-mute", "@DEFAULT_SINK@", flag])
                .status()
                .is_ok_and(|s| s.success());
    }
    #[allow(unreachable_code)]
    false
}

#[cfg(windows)]
fn windows_mute(muted: bool) -> windows::core::Result<()> {
    use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
    use windows::Win32::Media::Audio::{
        IMMDeviceEnumerator, MMDeviceEnumerator, eConsole, eRender,
    };
    use windows::Win32::System::Com::{
        CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
    };

    // SAFETY: standard COM initialization and calls on objects we own for this scope.
    unsafe {
        let init = CoInitializeEx(None, COINIT_MULTITHREADED);
        let result = (|| {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
            let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole)?;
            let volume: IAudioEndpointVolume = device.Activate(CLSCTX_ALL, None)?;
            volume.SetMute(
                windows::Win32::Foundation::BOOL::from(muted),
                std::ptr::null(),
            )
        })();
        // Balance a successful initialisation (S_OK or S_FALSE) once the objects above are released.
        if init.is_ok() {
            CoUninitialize();
        }
        result
    }
}
