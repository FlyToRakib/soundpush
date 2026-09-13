//! OS integration: sleep prevention and muting the default output device.

use std::process::Child;
#[cfg(unix)]
use std::process::Command;

/// Keeps the system awake while alive.
pub enum SleepInhibitor {
    #[cfg(windows)]
    Windows,
    #[allow(dead_code)]
    Process(Child),
}

impl SleepInhibitor {
    pub fn acquire() -> Option<Self> {
        #[cfg(windows)]
        {
            use windows::Win32::System::Power::{ES_CONTINUOUS, ES_SYSTEM_REQUIRED, SetThreadExecutionState};
            // SAFETY: plain Win32 call with valid flags.
            let prev = unsafe { SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED) };
            return (prev.0 != 0).then_some(Self::Windows);
        }
        #[cfg(target_os = "macos")]
        {
            return Command::new("caffeinate").arg("-i").spawn().ok().map(Self::Process);
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            return Command::new("systemd-inhibit")
                .args(["--what=idle:sleep", "--who=SoundPush", "--why=Streaming audio", "sleep", "infinity"])
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
            Self::Windows => {
                use windows::Win32::System::Power::{ES_CONTINUOUS, SetThreadExecutionState};
                // SAFETY: plain Win32 call restoring the default state.
                unsafe {
                    SetThreadExecutionState(ES_CONTINUOUS);
                }
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
        return Command::new("osascript").args(["-e", &script]).status().is_ok_and(|s| s.success());
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
    use windows::Win32::Media::Audio::{IMMDeviceEnumerator, MMDeviceEnumerator, eConsole, eRender};
    use windows::Win32::System::Com::{CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx};

    // SAFETY: standard COM initialization and calls on objects we own for this scope.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole)?;
        let volume: IAudioEndpointVolume = device.Activate(CLSCTX_ALL, None)?;
        volume.SetMute(windows::Win32::Foundation::BOOL::from(muted), std::ptr::null())
    }
}
