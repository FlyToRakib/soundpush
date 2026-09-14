//! Audio device notifications from the OS → engine (plan §8.3, §13.3 "Device changes").
//!
//! - **Windows:** `IMMNotificationClient` (default device changed, device added/removed/state).
//! - **macOS:** Core Audio property listeners on the default input/output and the device list.
//! - **Linux:** no notification source here yet; the PipeWire integration feeds [`Change`]s.
//!
//! Bursts (unplugging a headset changes the default and the list at once) are merged into one
//! engine call, which refreshes the device list and moves routes that follow the default device.

use std::sync::mpsc;
use std::time::Duration;

use tauri::{AppHandle, Manager};
use tracing::warn;

use crate::AppState;

/// Quiet period that ends a burst of notifications.
const SETTLE: Duration = Duration::from_millis(400);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Change {
    pub default_input: bool,
    pub default_output: bool,
}

impl Change {
    pub const LIST: Self = Self { default_input: false, default_output: false };

    fn merge(&mut self, other: Self) {
        self.default_input |= other.default_input;
        self.default_output |= other.default_output;
    }
}

pub fn start(app: AppHandle) {
    let (tx, rx) = mpsc::channel::<Change>();
    #[cfg(windows)]
    windows::watch(tx);
    #[cfg(target_os = "macos")]
    crate::macos::watch_devices(tx);
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        // Linux integration point: send a `Change` on PipeWire/PulseAudio device events.
        drop(tx);
    }

    let spawned = std::thread::Builder::new().name("sp-device-events".into()).spawn(move || {
        while let Ok(first) = rx.recv() {
            let mut change = first;
            while let Ok(more) = rx.recv_timeout(SETTLE) {
                change.merge(more);
            }
            if let Some(engine) = app.try_state::<AppState>().and_then(|s| s.engine.get().cloned()) {
                let _ = engine.audio_devices_changed(change.default_input, change.default_output);
            }
        }
    });
    if let Err(e) = spawned {
        warn!(error = %e, "could not watch audio devices");
    }
}

#[cfg(windows)]
mod windows {
    use std::sync::mpsc::Sender;

    use tracing::{info, warn};
    use windows::Win32::Media::Audio::{
        DEVICE_STATE, EDataFlow, ERole, IMMDeviceEnumerator, IMMNotificationClient, IMMNotificationClient_Impl,
        MMDeviceEnumerator, eCapture, eConsole, eRender,
    };
    use windows::Win32::System::Com::{CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx};
    use windows::Win32::UI::Shell::PropertiesSystem::PROPERTYKEY;
    use windows::core::{PCWSTR, implement};

    use super::Change;

    #[implement(IMMNotificationClient)]
    struct DeviceEvents(Sender<Change>);

    impl IMMNotificationClient_Impl for DeviceEvents_Impl {
        fn OnDeviceStateChanged(&self, _id: &PCWSTR, _state: DEVICE_STATE) -> windows::core::Result<()> {
            let _ = self.0.send(Change::LIST);
            Ok(())
        }

        fn OnDeviceAdded(&self, _id: &PCWSTR) -> windows::core::Result<()> {
            let _ = self.0.send(Change::LIST);
            Ok(())
        }

        fn OnDeviceRemoved(&self, _id: &PCWSTR) -> windows::core::Result<()> {
            let _ = self.0.send(Change::LIST);
            Ok(())
        }

        fn OnDefaultDeviceChanged(&self, flow: EDataFlow, role: ERole, _id: &PCWSTR) -> windows::core::Result<()> {
            // Windows reports each role; SoundPush's "system default" is the console role.
            if role == eConsole {
                let _ = self.0.send(Change { default_input: flow == eCapture, default_output: flow == eRender });
            }
            Ok(())
        }

        fn OnPropertyValueChanged(&self, _id: &PCWSTR, _key: &PROPERTYKEY) -> windows::core::Result<()> {
            Ok(())
        }
    }

    /// Registers for notifications on a thread that keeps the COM objects alive for the life
    /// of the process. Callbacks only send on a channel, as Windows requires them to be quick.
    pub fn watch(tx: Sender<Change>) {
        let spawned = std::thread::Builder::new().name("sp-device-watch".into()).spawn(move || {
            // SAFETY: COM stays initialised on this thread, which never exits while the process runs.
            let registered = unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
                CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL).and_then(
                    |enumerator| {
                        let client: IMMNotificationClient = DeviceEvents(tx).into();
                        enumerator.RegisterEndpointNotificationCallback(&client)?;
                        Ok((enumerator, client))
                    },
                )
            };
            match registered {
                Ok(_keep_alive) => {
                    info!("watching audio device changes");
                    loop {
                        std::thread::park();
                    }
                }
                Err(e) => warn!(error = %e, "could not register for audio device notifications"),
            }
        });
        if let Err(e) = spawned {
            warn!(error = %e, "could not start the audio device watcher");
        }
    }
}
