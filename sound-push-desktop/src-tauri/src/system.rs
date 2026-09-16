//! OS state the UI and troubleshooter check (plan §4.6, §8.4, §13.6, §26.2, §28.2):
//! microphone and system-audio permissions, missing Windows media components ("N" editions),
//! launch at login switched off outside SoundPush, Bluetooth outputs, and whether an app is
//! recording from the virtual microphone.

use serde::Serialize;

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemStatus {
    /// "granted", "denied", "notDetermined", "restricted" or "unknown".
    pub microphone: &'static str,
    /// macOS "System Audio Recording" for sending this computer's sound; "granted" elsewhere.
    pub system_audio: &'static str,
    /// Windows media components (Media Foundation) are missing, as on N editions without the
    /// Media Feature Pack.
    pub media_feature_pack_missing: bool,
    /// Windows edition, e.g. "ProfessionalN".
    pub windows_edition: Option<String>,
    /// Launch at login was turned off in Task Manager / Startup apps.
    pub autostart_disabled_by_os: bool,
    /// Playback devices connected over Bluetooth (they add noticeable delay).
    pub bluetooth_outputs: Vec<String>,
    /// Name of the default playback device.
    pub default_output: Option<String>,
    /// Per-app capture of "this computer's audio" works here (Windows 10 2004+, or Linux with a
    /// PipeWire or PulseAudio sound server).
    pub app_capture: bool,
    /// Installed Microsoft Edge WebView2 runtime (Windows); `None` when it is missing, which is
    /// what turns the window into a white screen (plan §10.3). Always `None` on other platforms.
    pub webview2_version: Option<String>,
    /// Audio-enhancement or overlay software that is running and is known to break capture
    /// (plan §8.3). Friendly names, for the troubleshooter. Windows only.
    pub audio_enhancements: Vec<String>,
    /// A VPN carries this computer's whole internet connection, which usually blocks the local
    /// network as well (plan §8.1).
    pub vpn: crate::tethering::VpnStatus,
}

/// What decides whether an automatic update check runs now (plan §24 "Startup performance").
#[derive(Debug, Clone, Copy, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateConditions {
    /// SoundPush was started by the OS at sign-in, so the first check waits half a minute and
    /// leaves the machine to finish signing in.
    pub autostarted: bool,
    /// The connection is metered (a phone hotspot, a mobile dongle, a capped plan). Update checks
    /// and downloads are not worth someone's data.
    pub metered: bool,
}

pub fn update_conditions(autostarted: bool) -> UpdateConditions {
    UpdateConditions {
        autostarted,
        metered: metered_connection(),
    }
}

/// Whether the connection SoundPush would use is metered. Windows knows for certain; elsewhere
/// this is a best effort and false means "no reason to think so".
fn metered_connection() -> bool {
    #[cfg(windows)]
    return windows::metered_connection();
    #[cfg(target_os = "linux")]
    return linux::metered_connection();
    #[allow(unreachable_code)]
    false
}

/// Microphone permission only (cheap; checked before microphone routes start).
pub fn microphone() -> &'static str {
    #[cfg(windows)]
    return if windows::microphone_denied() {
        "denied"
    } else {
        "granted"
    };
    #[cfg(target_os = "macos")]
    return crate::macos::microphone_permission();
    #[allow(unreachable_code)]
    "unknown"
}

pub fn status() -> SystemStatus {
    #[cfg(windows)]
    return SystemStatus {
        microphone: microphone(),
        system_audio: "granted",
        media_feature_pack_missing: windows::media_feature_pack_missing(),
        windows_edition: windows::edition(),
        autostart_disabled_by_os: windows::autostart_disabled_by_os(),
        bluetooth_outputs: windows::bluetooth_outputs(),
        default_output: default_output(),
        app_capture: sp_audio_io::wasapi_process::process_loopback_supported(),
        webview2_version: crate::webview2::version(),
        audio_enhancements: windows::audio_enhancements(),
        vpn: crate::tethering::vpn(),
    };
    #[cfg(target_os = "macos")]
    return SystemStatus {
        microphone: microphone(),
        system_audio: crate::macos::system_audio_permission(),
        bluetooth_outputs: crate::macos::bluetooth_outputs(),
        default_output: default_output(),
        autostart_disabled_by_os: crate::macos::login_item_needs_approval(),
        webview2_version: crate::webview2::version(),
        vpn: crate::tethering::vpn(),
        ..SystemStatus::default()
    };
    #[allow(unreachable_code)]
    SystemStatus {
        microphone: "unknown",
        system_audio: "unknown",
        default_output: default_output(),
        webview2_version: crate::webview2::version(),
        vpn: crate::tethering::vpn(),
        #[cfg(target_os = "linux")]
        app_capture: sp_audio_io::pulse::available(),
        ..SystemStatus::default()
    }
}

/// Outputs only: reading a microphone's format would count as microphone access on macOS.
fn default_output() -> Option<String> {
    sp_audio_io::cpal_backend::CpalBackend::new().default_output_name()
}

/// Whether an app other than SoundPush records from the capture device `input` right now.
/// Linux asks its sound server instead (`virtual_mic::virtual_mic_in_use`).
#[cfg_attr(target_os = "linux", allow(dead_code))]
pub fn capture_device_in_use(input: &str) -> bool {
    #[cfg(windows)]
    return windows::capture_device_in_use(input);
    #[cfg(target_os = "macos")]
    return crate::macos::device_running_somewhere(input);
    #[allow(unreachable_code)]
    {
        let _ = input;
        false
    }
}

/// System Settings page for a topic, as a URL the OS opens.
pub fn settings_url(topic: &str) -> Option<&'static str> {
    #[cfg(windows)]
    return match topic {
        "microphone" => Some("ms-settings:privacy-microphone"),
        "network" => Some("ms-settings:network-status"),
        "optionalFeatures" => Some("ms-settings:optionalfeatures"),
        "sound" => Some("ms-settings:sound"),
        "startup" => Some("ms-settings:startupapps"),
        _ => None,
    };
    #[cfg(target_os = "macos")]
    return match topic {
        "microphone" => {
            Some("x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone")
        }
        "systemAudio" => {
            Some("x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture")
        }
        "sound" => Some("x-apple.systempreferences:com.apple.Sound-Settings.extension"),
        "network" => Some("x-apple.systempreferences:com.apple.Network-Settings.extension"),
        "startup" => Some("x-apple.systempreferences:com.apple.LoginItems-Settings.extension"),
        _ => None,
    };
    #[allow(unreachable_code)]
    {
        let _ = topic;
        None
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use std::time::Duration;

    /// NetworkManager's own judgement about the connection in use: 1 = metered, 3 = probably
    /// metered (2 and 4 are the other way round, 0 is "no idea"). A desktop without
    /// NetworkManager simply says nothing, and the check falls back to "not metered".
    pub fn metered_connection() -> bool {
        use dbus::blocking::stdintf::org_freedesktop_dbus::Properties;

        let Ok(connection) = dbus::blocking::Connection::new_system() else {
            return false;
        };
        let proxy = connection.with_proxy(
            "org.freedesktop.NetworkManager",
            "/org/freedesktop/NetworkManager",
            Duration::from_secs(2),
        );
        proxy
            .get::<u32>("org.freedesktop.NetworkManager", "Metered")
            .is_ok_and(|metered| metered == 1 || metered == 3)
    }
}

#[cfg(windows)]
mod windows {
    use std::path::PathBuf;

    use windows::Win32::Media::Audio::{
        AudioSessionStateActive, DEVICE_STATE_ACTIVE, EDataFlow, IAudioSessionControl2,
        IAudioSessionManager2, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator, eCapture,
        eRender,
    };
    use windows::Win32::System::Com::{
        CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
        STGM_READ,
    };
    use windows::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, REG_ROUTINE_FLAGS, RRF_RT_REG_BINARY,
        RRF_RT_REG_SZ, RegGetValueW,
    };
    use windows::Win32::UI::Shell::PropertiesSystem::PROPERTYKEY;
    use windows::core::{GUID, HSTRING, Interface};

    /// `PKEY_Device_FriendlyName`.
    const FRIENDLY_NAME: PROPERTYKEY = PROPERTYKEY {
        fmtid: GUID::from_u128(0xa45c254e_df1c_4efd_8020_67d146a850e0),
        pid: 14,
    };
    /// `DEVPKEY_Device_EnumeratorName` ("BTHENUM", "BTHHFENUM", "BTHLEDEVICE" for Bluetooth).
    const ENUMERATOR_NAME: PROPERTYKEY = PROPERTYKEY {
        fmtid: GUID::from_u128(0xa45c254e_df1c_4efd_8020_67d146a850e0),
        pid: 24,
    };
    const CONSENT_STORE: &str = r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone";

    fn registry(root: HKEY, path: &str, name: &str, kind: REG_ROUTINE_FLAGS) -> Option<Vec<u8>> {
        let (path, name) = (HSTRING::from(path), HSTRING::from(name));
        let mut buf = vec![0u8; 512];
        let mut size = buf.len() as u32;
        // SAFETY: the buffer and its byte size are valid for the call.
        let status = unsafe {
            RegGetValueW(
                root,
                &path,
                &name,
                kind,
                None,
                Some(buf.as_mut_ptr().cast()),
                Some(&mut size),
            )
        };
        if status.is_err() {
            return None;
        }
        buf.truncate(size as usize);
        Some(buf)
    }

    fn registry_string(root: HKEY, path: &str, name: &str) -> Option<String> {
        let bytes = registry(root, path, name, RRF_RT_REG_SZ)?;
        let wide: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        let len = wide.iter().position(|&c| c == 0).unwrap_or(wide.len());
        Some(String::from_utf16_lossy(&wide[..len]))
    }

    /// Settings → Privacy → Microphone: off for the device, for this user, or for desktop apps.
    pub fn microphone_denied() -> bool {
        let deny = |root, path: &str| {
            registry_string(root, path, "Value").is_some_and(|v| v.eq_ignore_ascii_case("Deny"))
        };
        deny(HKEY_LOCAL_MACHINE, CONSENT_STORE)
            || deny(HKEY_CURRENT_USER, CONSENT_STORE)
            || deny(HKEY_CURRENT_USER, &format!(r"{CONSENT_STORE}\NonPackaged"))
    }

    pub fn edition() -> Option<String> {
        registry_string(
            HKEY_LOCAL_MACHINE,
            r"SOFTWARE\Microsoft\Windows NT\CurrentVersion",
            "EditionID",
        )
    }

    /// Media Foundation ships with every Windows edition except N/KN without the Media Feature Pack.
    pub fn media_feature_pack_missing() -> bool {
        let system = std::env::var_os("SystemRoot")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        !system.join(r"System32\mfplat.dll").exists()
    }

    /// Task Manager's Startup tab records a disabled entry with an odd first byte.
    pub fn autostart_disabled_by_os() -> bool {
        registry(
            HKEY_CURRENT_USER,
            r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run",
            "SoundPush",
            RRF_RT_REG_BINARY,
        )
        .and_then(|b| b.first().copied())
        .is_some_and(|flag| flag & 1 == 1)
    }

    fn with_devices<T: Default>(flow: EDataFlow, f: impl FnOnce(Vec<IMMDevice>) -> T) -> T {
        // SAFETY: COM initialised for this call only; objects are released before CoUninitialize.
        unsafe {
            let init = CoInitializeEx(None, COINIT_MULTITHREADED);
            let devices = (|| -> windows::core::Result<Vec<IMMDevice>> {
                let enumerator: IMMDeviceEnumerator =
                    CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
                let collection = enumerator.EnumAudioEndpoints(flow, DEVICE_STATE_ACTIVE)?;
                (0..collection.GetCount()?)
                    .map(|i| collection.Item(i))
                    .collect()
            })();
            let result = devices.map(f).unwrap_or_default();
            if init.is_ok() {
                CoUninitialize();
            }
            result
        }
    }

    fn property(device: &IMMDevice, key: &PROPERTYKEY) -> Option<String> {
        // SAFETY: reading a property from a live device's store.
        unsafe {
            let store = device.OpenPropertyStore(STGM_READ).ok()?;
            let value = store.GetValue(key).ok()?;
            Some(value.to_string()).filter(|s| !s.is_empty())
        }
    }

    pub fn bluetooth_outputs() -> Vec<String> {
        with_devices(eRender, |devices| {
            devices
                .iter()
                .filter(|d| {
                    property(d, &ENUMERATOR_NAME)
                        .is_some_and(|e| e.to_ascii_uppercase().starts_with("BTH"))
                })
                .filter_map(|d| property(d, &FRIENDLY_NAME))
                .collect()
        })
    }

    /// Windows' own answer to "is this connection metered", the one the Store and Windows Update
    /// use. `NLM_CONNECTION_COST_FIXED` and `_VARIABLE` are the metered plans; the extra flags
    /// mean the plan is already over its limit or roaming, which is metered by any measure.
    pub fn metered_connection() -> bool {
        use windows::Win32::Networking::NetworkListManager::{
            INetworkCostManager, NLM_CONNECTION_COST_FIXED, NLM_CONNECTION_COST_OVERDATALIMIT,
            NLM_CONNECTION_COST_ROAMING, NLM_CONNECTION_COST_VARIABLE, NetworkListManager,
        };

        // SAFETY: COM initialised for this call only; the object is released before CoUninitialize.
        unsafe {
            let init = CoInitializeEx(None, COINIT_MULTITHREADED);
            let cost = (|| -> windows::core::Result<u32> {
                let manager: INetworkCostManager =
                    CoCreateInstance(&NetworkListManager, None, CLSCTX_ALL)?;
                let mut cost = 0u32;
                // A null destination address asks about the connection used by default.
                manager.GetCost(&mut cost, std::ptr::null())?;
                Ok(cost)
            })()
            .unwrap_or(0);
            if init.is_ok() {
                CoUninitialize();
            }
            let metered = NLM_CONNECTION_COST_FIXED.0
                | NLM_CONNECTION_COST_VARIABLE.0
                | NLM_CONNECTION_COST_OVERDATALIMIT.0
                | NLM_CONNECTION_COST_ROAMING.0;
            cost & metered.unsigned_abs() != 0
        }
    }

    /// Audio-enhancement and overlay software known to break WASAPI capture (plan §8.3): they sit
    /// in the audio path as an APO or hook the endpoints, and a loopback or microphone stream then
    /// opens and delivers silence, or fails outright. Matched on the running process, because
    /// what matters is whether it is active now, not whether it was ever installed.
    ///
    /// (process name in lower case without `.exe`, what to call it)
    const AUDIO_ENHANCEMENTS: &[(&str, &str)] = &[
        ("nahimicservice", "Nahimic"),
        ("nahimicsvc32", "Nahimic"),
        ("nahimicsvc64", "Nahimic"),
        ("nahimic3", "Nahimic"),
        ("nahimicmsiosd", "Nahimic"),
        ("a-volute.nahimic", "Nahimic"),
        ("sonicstudio3", "Sonic Studio 3"),
        ("sonicradar3", "Sonic Radar 3"),
        ("asusaudiocenter", "ASUS Audio Center (Sonic Studio)"),
        ("nvidiarttheatreservice", "NVIDIA RTX Voice"),
        ("thxspatialaudio", "THX Spatial Audio"),
        ("razersynapseservice", "Razer Synapse"),
        ("voicemod", "Voicemod"),
        ("voicemeeter", "Voicemeeter"),
        ("clownfish", "Clownfish Voice Changer"),
        ("boomaudio", "Boom 3D"),
        ("dolbyDAX2API", "Dolby Access"),
        ("waves.maxxaudio", "Waves MaxxAudio"),
        ("wavesvc", "Waves MaxxAudio"),
    ];

    /// Which of [`AUDIO_ENHANCEMENTS`] are running, by their friendly name, each named once.
    pub fn audio_enhancements() -> Vec<String> {
        use windows::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
            TH32CS_SNAPPROCESS,
        };

        let mut found: Vec<String> = Vec::new();
        // SAFETY: the snapshot handle is used only while open and closed by its own `Drop`
        // (`HANDLE` from this API is an owned handle in windows-rs).
        unsafe {
            let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
                return found;
            };
            let mut entry = PROCESSENTRY32W {
                dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
                ..Default::default()
            };
            let mut more = Process32FirstW(snapshot, &mut entry).is_ok();
            while more {
                let end = entry
                    .szExeFile
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(entry.szExeFile.len());
                let name = String::from_utf16_lossy(&entry.szExeFile[..end]).to_ascii_lowercase();
                let name = name.strip_suffix(".exe").unwrap_or(&name);
                if let Some((_, friendly)) = AUDIO_ENHANCEMENTS
                    .iter()
                    .find(|(process, _)| name.starts_with(&process.to_ascii_lowercase()))
                    && !found.iter().any(|f| f == friendly)
                {
                    found.push((*friendly).to_string());
                }
                more = Process32NextW(snapshot, &mut entry).is_ok();
            }
            let _ = windows::Win32::Foundation::CloseHandle(snapshot);
        }
        found
    }

    pub fn capture_device_in_use(input: &str) -> bool {
        let own = std::process::id();
        with_devices(eCapture, |devices| {
            devices
                .iter()
                .filter(|d| property(d, &FRIENDLY_NAME).is_some_and(|n| n == input))
                .any(|device| {
                    // SAFETY: session enumeration on a live device.
                    unsafe {
                        let Ok(manager) =
                            device.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None)
                        else {
                            return false;
                        };
                        let Ok(sessions) = manager.GetSessionEnumerator() else {
                            return false;
                        };
                        (0..sessions.GetCount().unwrap_or(0)).any(|i| {
                            sessions
                                .GetSession(i)
                                .and_then(|s| s.cast::<IAudioSessionControl2>())
                                .is_ok_and(|s| {
                                    s.GetState()
                                        .is_ok_and(|state| state == AudioSessionStateActive)
                                        && s.GetProcessId().is_ok_and(|pid| pid != own)
                                })
                        })
                    }
                })
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn running_audio_enhancements_are_listed_once_each() {
            // Whatever is on this machine, the list must be clean: no duplicates and no empties.
            let found = audio_enhancements();
            let mut sorted = found.clone();
            sorted.sort();
            sorted.dedup();
            assert_eq!(sorted.len(), found.len(), "{found:?}");
            assert!(found.iter().all(|f| !f.is_empty()));
            assert!(
                AUDIO_ENHANCEMENTS
                    .iter()
                    .all(|(p, _)| !p.is_empty() && !p.ends_with(".exe")),
                "process names are matched without the extension"
            );
        }

        #[test]
        fn the_connection_cost_is_read_without_admin() {
            // Whatever this machine is on, the call must answer rather than hang or fail.
            let _ = metered_connection();
        }

        #[test]
        fn reads_system_state_without_admin() {
            assert!(edition().is_some());
            assert!(!media_feature_pack_missing() || edition().is_some_and(|e| e.ends_with('N')));
            let _ = (
                microphone_denied(),
                autostart_disabled_by_os(),
                bluetooth_outputs(),
            );
            assert!(!capture_device_in_use(
                "SoundPush test device that does not exist"
            ));
        }
    }
}
