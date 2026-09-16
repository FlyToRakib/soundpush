//! "Make SoundPush the default input and output while active" (plan §23.2, headset flow).
//!
//! Off by default. While it is on and a route is running, the computer's default recording device
//! becomes the virtual microphone the phone feeds, so every app picks the phone up without being
//! set up one by one, and the default playback device becomes the virtual cable SoundPush
//! captures, so everything the computer plays reaches the phone. A real speaker is never made the
//! default: that would change nothing and would only move the user's own choice around.
//!
//! What was default before is put back when the last route stops and when SoundPush exits. A run
//! that never gets to exit (a crash, a power cut) leaves the note behind, and the next start puts
//! the defaults back from it — the same trick `hooks.rs` uses for the speaker mute.
//!
//! Windows has no documented way to set the default endpoint, so it uses `IPolicyConfig`, the
//! interface the Sound control panel itself calls. macOS sets the Core Audio default device, and
//! Linux asks the sound server (`sp_audio_io::pulse`).

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Input,
    Output,
}

/// Defaults SoundPush replaced, in whatever form the platform identifies a device by (a Windows
/// endpoint id, a Core Audio device UID, a PulseAudio sink or source name). Written to disk while
/// they are replaced so a crashed run can still be undone.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Previous {
    input: Option<String>,
    output: Option<String>,
}

impl Previous {
    fn slot(&mut self, kind: Kind) -> &mut Option<String> {
        match kind {
            Kind::Input => &mut self.input,
            Kind::Output => &mut self.output,
        }
    }

    fn is_empty(&self) -> bool {
        self.input.is_none() && self.output.is_none()
    }
}

/// Exists while SoundPush has changed a default device.
const MARKER: &str = "default-devices.json";

pub struct DefaultDevices {
    previous: Mutex<Previous>,
    marker: PathBuf,
}

impl DefaultDevices {
    /// Put back anything a previous run left replaced, then start clean.
    pub fn new(data_dir: &Path) -> Self {
        let devices = Self {
            previous: Mutex::new(Previous::default()),
            marker: data_dir.join(MARKER),
        };
        if let Some(left) = std::fs::read(&devices.marker)
            .ok()
            .and_then(|b| serde_json::from_slice::<Previous>(&b).ok())
            .filter(|p| !p.is_empty())
        {
            warn!("the previous run left the default audio devices changed; putting them back");
            if let Ok(mut previous) = devices.previous.lock() {
                *previous = left;
            }
            devices.restore();
        }
        devices
    }

    /// Ask for these devices to be the defaults. `None` for a direction means "leave it alone",
    /// and puts back what SoundPush replaced there. Names are the ones the audio backend lists.
    pub fn apply(&self, input: Option<&str>, output: Option<&str>) {
        let Ok(mut previous) = self.previous.lock() else {
            return;
        };
        for (kind, wanted) in [(Kind::Input, input), (Kind::Output, output)] {
            match wanted {
                Some(name) => take_over(&mut previous, kind, name),
                None => give_back(&mut previous, kind),
            }
        }
        self.write_marker(&previous);
    }

    /// Put every replaced default back (the last route stopped, or SoundPush is exiting).
    pub fn restore(&self) {
        let Ok(mut previous) = self.previous.lock() else {
            return;
        };
        give_back(&mut previous, Kind::Input);
        give_back(&mut previous, Kind::Output);
        self.write_marker(&previous);
    }

    fn write_marker(&self, previous: &Previous) {
        if previous.is_empty() {
            let _ = std::fs::remove_file(&self.marker);
        } else if let Ok(json) = serde_json::to_vec(previous) {
            let _ = std::fs::write(&self.marker, json);
        }
    }
}

/// Make `name` the default for `kind`, remembering what was there the first time.
fn take_over(previous: &mut Previous, kind: Kind, name: &str) {
    let Some(wanted) = sys::find(kind, name) else {
        debug!(
            device = name,
            "not making a device that is gone the default"
        );
        return;
    };
    let current = sys::current(kind);
    if current.as_deref() == Some(wanted.as_str()) {
        return;
    }
    if !sys::set(kind, &wanted) {
        return;
    }
    // Only the first change is remembered: later ones would record SoundPush's own device.
    previous.slot(kind).get_or_insert_with(|| {
        debug!(device = name, "made SoundPush the default device");
        current.unwrap_or_default()
    });
}

/// Put back what `kind` was before, if SoundPush replaced it.
fn give_back(previous: &mut Previous, kind: Kind) {
    let Some(before) = previous.slot(kind).take() else {
        return;
    };
    // An empty entry means there was no default at all; nothing to put back.
    if !before.is_empty() && !sys::set(kind, &before) {
        warn!("could not put the previous default audio device back");
    }
}

#[cfg(windows)]
mod sys {
    // The `IPolicyConfig` methods below keep Windows' own spelling so the vtable order stays
    // easy to check against the interface definition.
    #![allow(non_snake_case)]

    use windows::Win32::Media::Audio::{
        DEVICE_STATE_ACTIVE, EDataFlow, ERole, IMMDeviceEnumerator, MMDeviceEnumerator, eCapture,
        eCommunications, eConsole, eMultimedia, eRender,
    };
    use windows::Win32::System::Com::{
        CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
        CoUninitialize, STGM_READ,
    };
    use windows::Win32::UI::Shell::PropertiesSystem::PROPERTYKEY;
    use windows::core::{GUID, HSTRING, PCWSTR, Result as WinResult};

    use super::Kind;

    /// `PKEY_Device_FriendlyName`, the name the audio backend also lists.
    const FRIENDLY_NAME: PROPERTYKEY = PROPERTYKEY {
        fmtid: GUID::from_u128(0xa45c254e_df1c_4efd_8020_67d146a850e0),
        pid: 14,
    };

    /// The two objects that switch the default endpoint, the way the Sound control panel does.
    /// Windows exposes no documented way to do this, and which of the two answers depends on the
    /// Windows version: Windows 11 (26100) offers only `CPolicyConfigVistaClient`, older builds
    /// offer `CPolicyConfigClient`. Both are tried.
    const POLICY_CONFIG_CLIENT: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);
    const POLICY_CONFIG_VISTA_CLIENT: GUID =
        GUID::from_u128(0x294935ce_f637_4e7c_a41b_ab255460b862);

    /// `IPolicyConfig`. Every method must be declared so the slots line up; only
    /// `SetDefaultEndpoint` is called, so the rest take opaque pointers. The names are Windows'
    /// own, which is why this module keeps them as they are spelled there.
    #[windows_core::interface("f8679f50-850a-410c-bd91-695c509cfa4a")]
    unsafe trait IPolicyConfig: windows_core::IUnknown {
        unsafe fn GetMixFormat(
            &self,
            id: PCWSTR,
            format: *mut *mut core::ffi::c_void,
        ) -> windows_core::HRESULT;
        unsafe fn GetDeviceFormat(
            &self,
            id: PCWSTR,
            default: i32,
            format: *mut *mut core::ffi::c_void,
        ) -> windows_core::HRESULT;
        unsafe fn ResetDeviceFormat(&self, id: PCWSTR) -> windows_core::HRESULT;
        unsafe fn SetDeviceFormat(
            &self,
            id: PCWSTR,
            endpoint: *mut core::ffi::c_void,
            mix: *mut core::ffi::c_void,
        ) -> windows_core::HRESULT;
        unsafe fn GetProcessingPeriod(
            &self,
            id: PCWSTR,
            default: i32,
            period: *mut i64,
            min: *mut i64,
        ) -> windows_core::HRESULT;
        unsafe fn SetProcessingPeriod(&self, id: PCWSTR, period: *mut i64)
        -> windows_core::HRESULT;
        unsafe fn GetShareMode(
            &self,
            id: PCWSTR,
            mode: *mut core::ffi::c_void,
        ) -> windows_core::HRESULT;
        unsafe fn SetShareMode(
            &self,
            id: PCWSTR,
            mode: *mut core::ffi::c_void,
        ) -> windows_core::HRESULT;
        unsafe fn GetPropertyValue(
            &self,
            id: PCWSTR,
            key: *const PROPERTYKEY,
            value: *mut core::ffi::c_void,
        ) -> windows_core::HRESULT;
        unsafe fn SetPropertyValue(
            &self,
            id: PCWSTR,
            key: *const PROPERTYKEY,
            value: *mut core::ffi::c_void,
        ) -> windows_core::HRESULT;
        unsafe fn SetDefaultEndpoint(&self, id: PCWSTR, role: ERole) -> windows_core::HRESULT;
        unsafe fn SetEndpointVisibility(&self, id: PCWSTR, visible: i32) -> windows_core::HRESULT;
    }

    /// `IPolicyConfigVista`, the same thing without `ResetDeviceFormat`. Keeping the two apart
    /// matters: one missing slot would shift every later method, and `SetDefaultEndpoint` would
    /// call something else entirely.
    #[windows_core::interface("568b9108-44bf-40b4-9006-86afe5b5a620")]
    unsafe trait IPolicyConfigVista: windows_core::IUnknown {
        unsafe fn GetMixFormat(
            &self,
            id: PCWSTR,
            format: *mut *mut core::ffi::c_void,
        ) -> windows_core::HRESULT;
        unsafe fn GetDeviceFormat(
            &self,
            id: PCWSTR,
            default: i32,
            format: *mut *mut core::ffi::c_void,
        ) -> windows_core::HRESULT;
        unsafe fn SetDeviceFormat(
            &self,
            id: PCWSTR,
            endpoint: *mut core::ffi::c_void,
            mix: *mut core::ffi::c_void,
        ) -> windows_core::HRESULT;
        unsafe fn GetProcessingPeriod(
            &self,
            id: PCWSTR,
            default: i32,
            period: *mut i64,
            min: *mut i64,
        ) -> windows_core::HRESULT;
        unsafe fn SetProcessingPeriod(&self, id: PCWSTR, period: *mut i64)
        -> windows_core::HRESULT;
        unsafe fn GetShareMode(
            &self,
            id: PCWSTR,
            mode: *mut core::ffi::c_void,
        ) -> windows_core::HRESULT;
        unsafe fn SetShareMode(
            &self,
            id: PCWSTR,
            mode: *mut core::ffi::c_void,
        ) -> windows_core::HRESULT;
        unsafe fn GetPropertyValue(
            &self,
            id: PCWSTR,
            key: *const PROPERTYKEY,
            value: *mut core::ffi::c_void,
        ) -> windows_core::HRESULT;
        unsafe fn SetPropertyValue(
            &self,
            id: PCWSTR,
            key: *const PROPERTYKEY,
            value: *mut core::ffi::c_void,
        ) -> windows_core::HRESULT;
        unsafe fn SetDefaultEndpoint(&self, id: PCWSTR, role: ERole) -> windows_core::HRESULT;
        unsafe fn SetEndpointVisibility(&self, id: PCWSTR, visible: i32) -> windows_core::HRESULT;
    }

    /// What the Sound control panel sets: the device becomes the default for ordinary playback,
    /// for multimedia and for communications.
    const ROLES: [ERole; 3] = [eConsole, eMultimedia, eCommunications];

    /// A device's endpoint id as an owned `String`, releasing the buffer Windows allocated.
    ///
    /// # Safety
    /// `device` must be a live `IMMDevice`.
    unsafe fn endpoint_id(device: &windows::Win32::Media::Audio::IMMDevice) -> Option<String> {
        // SAFETY: the caller guarantees a live device; `GetId` hands over a COM-allocated string
        // that the caller owns, so it is copied and then freed.
        unsafe {
            let id = device.GetId().ok()?;
            let owned = id.to_string().ok();
            CoTaskMemFree(Some(id.as_ptr().cast()));
            owned
        }
    }

    fn flow(kind: Kind) -> EDataFlow {
        match kind {
            Kind::Input => eCapture,
            Kind::Output => eRender,
        }
    }

    fn with_com<T: Default>(f: impl FnOnce() -> WinResult<T>) -> T {
        // SAFETY: balanced with CoUninitialize when initialisation succeeded; every object the
        // closure creates is released before that.
        let init = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        let result = f().unwrap_or_else(|e| {
            tracing::debug!(error = %e, "default audio device call failed");
            T::default()
        });
        if init.is_ok() {
            // SAFETY: matches the successful CoInitializeEx.
            unsafe { CoUninitialize() };
        }
        result
    }

    /// Endpoint id of the active device named `name`, as `IMMDevice::GetId` spells it.
    pub(super) fn find(kind: Kind, name: &str) -> Option<String> {
        with_com(|| {
            // SAFETY: COM calls on objects created and released in this scope.
            unsafe {
                let enumerator: IMMDeviceEnumerator =
                    CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
                let devices = enumerator.EnumAudioEndpoints(flow(kind), DEVICE_STATE_ACTIVE)?;
                for i in 0..devices.GetCount()? {
                    let Ok(device) = devices.Item(i) else {
                        continue;
                    };
                    let friendly = device
                        .OpenPropertyStore(STGM_READ)
                        .and_then(|store| store.GetValue(&FRIENDLY_NAME))
                        .map(|v| v.to_string())
                        .unwrap_or_default();
                    if friendly == name {
                        return Ok(endpoint_id(&device));
                    }
                }
                Ok(None)
            }
        })
    }

    pub(super) fn current(kind: Kind) -> Option<String> {
        with_com(|| {
            // SAFETY: COM calls on objects created and released in this scope.
            unsafe {
                let enumerator: IMMDeviceEnumerator =
                    CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
                // No default at all is a normal state (a PC with no sound card), not an error.
                let Ok(device) = enumerator.GetDefaultAudioEndpoint(flow(kind), eConsole) else {
                    return Ok(None);
                };
                Ok(endpoint_id(&device))
            }
        })
    }

    pub(super) fn set(kind: Kind, endpoint_id: &str) -> bool {
        let _ = kind;
        with_com(|| {
            let id = HSTRING::from(endpoint_id);
            let id = PCWSTR(id.as_ptr());
            let mut ok = false;
            // SAFETY: each client object is created and released in this scope, and the id string
            // outlives every call that reads it. A build that does not offer one of the two
            // objects simply fails to create it.
            unsafe {
                if let Ok(policy) =
                    CoCreateInstance::<_, IPolicyConfig>(&POLICY_CONFIG_CLIENT, None, CLSCTX_ALL)
                {
                    for role in ROLES {
                        ok |= policy.SetDefaultEndpoint(id, role).is_ok();
                    }
                }
                if !ok
                    && let Ok(policy) = CoCreateInstance::<_, IPolicyConfigVista>(
                        &POLICY_CONFIG_VISTA_CLIENT,
                        None,
                        CLSCTX_ALL,
                    )
                {
                    for role in ROLES {
                        ok |= policy.SetDefaultEndpoint(id, role).is_ok();
                    }
                }
            }
            Ok(ok)
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn the_current_defaults_can_be_read_and_set_to_themselves() {
            // Setting the default to what it already is proves IPolicyConfig answers without
            // changing anything on the machine running the tests.
            for kind in [Kind::Input, Kind::Output] {
                let Some(current) = current(kind) else {
                    continue;
                };
                assert!(current.starts_with("{0.0."), "unexpected id {current}");
                assert!(set(kind, &current), "could not set {kind:?} to itself");
            }
            assert_eq!(find(Kind::Output, "a device that does not exist"), None);
        }
    }
}

#[cfg(target_os = "macos")]
mod sys {
    use super::Kind;

    // CoreAudio, weakly available through the framework the app already links.
    #[link(name = "CoreAudio", kind = "framework")]
    unsafe extern "C" {
        fn AudioObjectGetPropertyDataSize(
            object: u32,
            address: *const AudioObjectPropertyAddress,
            qualifier_size: u32,
            qualifier: *const core::ffi::c_void,
            size: *mut u32,
        ) -> i32;
        fn AudioObjectGetPropertyData(
            object: u32,
            address: *const AudioObjectPropertyAddress,
            qualifier_size: u32,
            qualifier: *const core::ffi::c_void,
            size: *mut u32,
            data: *mut core::ffi::c_void,
        ) -> i32;
        fn AudioObjectSetPropertyData(
            object: u32,
            address: *const AudioObjectPropertyAddress,
            qualifier_size: u32,
            qualifier: *const core::ffi::c_void,
            size: u32,
            data: *const core::ffi::c_void,
        ) -> i32;
    }

    #[repr(C)]
    struct AudioObjectPropertyAddress {
        selector: u32,
        scope: u32,
        element: u32,
    }

    const SYSTEM_OBJECT: u32 = 1;
    const NO_ERROR: i32 = 0;
    /// Four-character codes, as Core Audio spells its selectors.
    const fn fourcc(s: &[u8; 4]) -> u32 {
        u32::from_be_bytes(*s)
    }
    const GLOBAL_SCOPE: u32 = fourcc(b"glob");
    const DEVICES: u32 = fourcc(b"dev#");
    const DEFAULT_INPUT: u32 = fourcc(b"dIn ");
    const DEFAULT_OUTPUT: u32 = fourcc(b"dOut");
    const DEVICE_NAME: u32 = fourcc(b"lnam");
    const MAIN_ELEMENT: u32 = 0;

    fn address(selector: u32) -> AudioObjectPropertyAddress {
        AudioObjectPropertyAddress {
            selector,
            scope: GLOBAL_SCOPE,
            element: MAIN_ELEMENT,
        }
    }

    fn default_selector(kind: Kind) -> u32 {
        match kind {
            Kind::Input => DEFAULT_INPUT,
            Kind::Output => DEFAULT_OUTPUT,
        }
    }

    /// Every audio device the system knows about.
    fn devices() -> Vec<u32> {
        let address = address(DEVICES);
        let mut size = 0u32;
        // SAFETY: the address outlives the call, which only writes the byte size it needs.
        let status = unsafe {
            AudioObjectGetPropertyDataSize(SYSTEM_OBJECT, &address, 0, std::ptr::null(), &mut size)
        };
        if status != NO_ERROR || size == 0 {
            return Vec::new();
        }
        let mut ids = vec![0u32; size as usize / std::mem::size_of::<u32>()];
        // SAFETY: `ids` is sized from the byte count the call just reported.
        let status = unsafe {
            AudioObjectGetPropertyData(
                SYSTEM_OBJECT,
                &address,
                0,
                std::ptr::null(),
                &mut size,
                ids.as_mut_ptr().cast(),
            )
        };
        if status == NO_ERROR { ids } else { Vec::new() }
    }

    /// A device's name, as `CFStringRef`, converted through Core Foundation.
    fn name_of(device: u32) -> Option<String> {
        use objc2_core_foundation::{CFRetained, CFString};

        let address = address(DEVICE_NAME);
        let mut string: *const CFString = std::ptr::null();
        let mut size = std::mem::size_of::<*const CFString>() as u32;
        // SAFETY: the property is a CFStringRef, so the out parameter is one pointer wide.
        let status = unsafe {
            AudioObjectGetPropertyData(
                device,
                &address,
                0,
                std::ptr::null(),
                &mut size,
                std::ptr::from_mut(&mut string).cast(),
            )
        };
        if status != NO_ERROR || string.is_null() {
            return None;
        }
        // SAFETY: Core Audio hands out a +1 reference for a "copy" property; taking ownership
        // releases it when the retained value is dropped.
        let string = unsafe { CFRetained::from_raw(std::ptr::NonNull::new(string.cast_mut())?) };
        Some(string.to_string())
    }

    pub(super) fn find(kind: Kind, name: &str) -> Option<String> {
        // A device is identified by its id; the id is stable while the device exists, which is
        // exactly as long as SoundPush needs it.
        let _ = kind;
        devices()
            .into_iter()
            .find(|d| name_of(*d).as_deref() == Some(name))
            .map(|d| d.to_string())
    }

    pub(super) fn current(kind: Kind) -> Option<String> {
        let address = address(default_selector(kind));
        let mut device = 0u32;
        let mut size = std::mem::size_of::<u32>() as u32;
        // SAFETY: the default device property is one `AudioDeviceID` wide.
        let status = unsafe {
            AudioObjectGetPropertyData(
                SYSTEM_OBJECT,
                &address,
                0,
                std::ptr::null(),
                &mut size,
                std::ptr::from_mut(&mut device).cast(),
            )
        };
        (status == NO_ERROR && device != 0).then(|| device.to_string())
    }

    pub(super) fn set(kind: Kind, device: &str) -> bool {
        let Ok(device) = device.parse::<u32>() else {
            return false;
        };
        let address = address(default_selector(kind));
        // SAFETY: the default device property is one `AudioDeviceID` wide, which is what is
        // passed together with its size.
        let status = unsafe {
            AudioObjectSetPropertyData(
                SYSTEM_OBJECT,
                &address,
                0,
                std::ptr::null(),
                std::mem::size_of::<u32>() as u32,
                std::ptr::from_ref(&device).cast(),
            )
        };
        status == NO_ERROR
    }
}

#[cfg(target_os = "linux")]
mod sys {
    use super::Kind;

    /// Sink and source names are what the sound server identifies a device by, and what
    /// `sp_audio_io::pulse` lists as a device id.
    pub(super) fn find(kind: Kind, name: &str) -> Option<String> {
        let mut pulse = sp_audio_io::pulse::Pulse::connect().ok()?;
        let devices = match kind {
            Kind::Input => pulse.sources().ok()?,
            Kind::Output => pulse.sinks().ok()?,
        };
        devices
            .into_iter()
            .find(|d| d.name == name || d.description == name)
            .map(|d| d.name)
    }

    pub(super) fn current(kind: Kind) -> Option<String> {
        let mut pulse = sp_audio_io::pulse::Pulse::connect().ok()?;
        let server = pulse.server().ok()?;
        match kind {
            Kind::Input => server.default_source,
            Kind::Output => server.default_sink,
        }
    }

    pub(super) fn set(kind: Kind, name: &str) -> bool {
        let Ok(mut pulse) = sp_audio_io::pulse::Pulse::connect() else {
            return false;
        };
        match kind {
            Kind::Input => pulse.set_default_source(name),
            Kind::Output => pulse.set_default_sink(name),
        }
        .is_ok()
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
mod sys {
    use super::Kind;

    pub(super) fn find(_kind: Kind, _name: &str) -> Option<String> {
        None
    }
    pub(super) fn current(_kind: Kind) -> Option<String> {
        None
    }
    pub(super) fn set(_kind: Kind, _device: &str) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_run_that_never_finished_is_undone_on_the_next_start() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join(MARKER);
        std::fs::write(&marker, br#"{"input":"an-old-device"}"#).unwrap();
        // The device is gone, so nothing can be put back — but the note must not survive, or
        // every later start would try again.
        let devices = DefaultDevices::new(dir.path());
        assert!(!marker.exists());
        assert!(
            devices.previous.lock().unwrap().is_empty(),
            "nothing is left remembered"
        );
    }

    #[test]
    fn asking_for_nothing_changes_nothing_and_leaves_no_note() {
        let dir = tempfile::tempdir().unwrap();
        let devices = DefaultDevices::new(dir.path());
        devices.apply(None, None);
        devices.restore();
        assert!(!dir.path().join(MARKER).exists());
    }

    #[test]
    fn only_the_first_change_is_remembered() {
        // Otherwise stopping a route would "put back" SoundPush's own device.
        let mut previous = Previous::default();
        *previous.slot(Kind::Input) = Some("the user's headset".into());
        previous
            .slot(Kind::Input)
            .get_or_insert_with(|| "SoundPush Microphone".into());
        assert_eq!(previous.input.as_deref(), Some("the user's headset"));
        give_back(&mut previous, Kind::Input);
        assert!(previous.is_empty());
    }
}
