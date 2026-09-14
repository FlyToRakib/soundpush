//! macOS integration: privacy permissions (TCC), Core Audio device notifications, device
//! properties cpal does not expose (Bluetooth transport, "device is running somewhere"), and the
//! login item (`SMAppService`, macOS 13+).
//!
//! Permissions (plan §26.2): checking never shows a prompt. The microphone prompt appears only
//! when SoundPush first opens a microphone (or the user presses "Allow" in the app); when access
//! was denied the UI opens the right System Settings pane instead of asking again.
//!
//! Core Audio and TCC are called through small hand-written declarations (stable C APIs).
//! TCC's preflight is private API, looked up at runtime; when it is missing the status is
//! "unknown" and no warning is shown.

use std::ffi::{c_char, c_void};
use std::sync::Mutex;
use std::sync::mpsc::Sender;
use std::time::{SystemTime, UNIX_EPOCH};

use objc2::msg_send;
use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::{AnyClass, AnyObject};
use objc2_foundation::NSString;
use tracing::warn;

use crate::device_watch::Change;

#[repr(C)]
#[derive(Clone, Copy)]
struct PropertyAddress {
    selector: u32,
    scope: u32,
    element: u32,
}

type ListenerProc = unsafe extern "C" fn(
    object: u32,
    count: u32,
    addresses: *const PropertyAddress,
    client: *mut c_void,
) -> i32;

#[link(name = "CoreAudio", kind = "framework")]
unsafe extern "C" {
    fn AudioObjectAddPropertyListener(
        object: u32,
        address: *const PropertyAddress,
        listener: ListenerProc,
        client: *mut c_void,
    ) -> i32;
    fn AudioObjectGetPropertyDataSize(
        object: u32,
        address: *const PropertyAddress,
        qualifier_size: u32,
        qualifier: *const c_void,
        size: *mut u32,
    ) -> i32;
    fn AudioObjectGetPropertyData(
        object: u32,
        address: *const PropertyAddress,
        qualifier_size: u32,
        qualifier: *const c_void,
        size: *mut u32,
        data: *mut c_void,
    ) -> i32;
    fn AudioObjectSetPropertyData(
        object: u32,
        address: *const PropertyAddress,
        qualifier_size: u32,
        qualifier: *const c_void,
        size: u32,
        data: *const c_void,
    ) -> i32;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFStringGetCString(
        string: *const c_void,
        buffer: *mut c_char,
        size: isize,
        encoding: u32,
    ) -> u8;
    fn CFRelease(cf: *const c_void);
}

// AVCaptureDevice lives in AVFoundation; linking it makes the class available at runtime.
#[link(name = "AVFoundation", kind = "framework")]
unsafe extern "C" {}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
}

// SMAppService lives in ServiceManagement (macOS 13+). On older systems the class is missing.
#[link(name = "ServiceManagement", kind = "framework")]
unsafe extern "C" {}

unsafe extern "C" {
    fn dlopen(path: *const c_char, mode: i32) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}

const fn fourcc(code: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*code)
}

const SYSTEM_OBJECT: u32 = 1;
const SCOPE_GLOBAL: u32 = fourcc(b"glob");
const SCOPE_OUTPUT: u32 = fourcc(b"outp");
const ELEMENT_MAIN: u32 = 0;
const DEVICES: u32 = fourcc(b"dev#");
const DEFAULT_INPUT: u32 = fourcc(b"dIn ");
const DEFAULT_OUTPUT: u32 = fourcc(b"dOut");
const RUN_LOOP: u32 = fourcc(b"rnlp");
const NAME: u32 = fourcc(b"lnam");
const STREAMS: u32 = fourcc(b"stm#");
const TRANSPORT: u32 = fourcc(b"tran");
const RUNNING_SOMEWHERE: u32 = fourcc(b"gone");
const TRANSPORT_BLUETOOTH: u32 = fourcc(b"blue");
const TRANSPORT_BLUETOOTH_LE: u32 = fourcc(b"blea");
const UTF8: u32 = 0x0800_0100;
const RTLD_LAZY: i32 = 1;

fn address(selector: u32, scope: u32) -> PropertyAddress {
    PropertyAddress {
        selector,
        scope,
        element: ELEMENT_MAIN,
    }
}

// ------------------------------------------------------------------ permissions

/// `AVCaptureDevice` authorization for audio. Never prompts.
pub fn microphone_permission() -> &'static str {
    let Some(class) = AnyClass::get(c"AVCaptureDevice") else {
        return "unknown";
    };
    // AVMediaTypeAudio is the string "soun".
    let media = NSString::from_str("soun");
    // SAFETY: +[AVCaptureDevice authorizationStatusForMediaType:] takes an NSString and returns NSInteger.
    let status: isize = unsafe { msg_send![class, authorizationStatusForMediaType: &*media] };
    match status {
        0 => "notDetermined",
        1 => "restricted",
        2 => "denied",
        3 => "granted",
        _ => "unknown",
    }
}

/// "System Audio Recording" (Core Audio process taps, TCC service `kTCCServiceAudioCapture`).
/// Before macOS 14.2 system audio comes from ScreenCaptureKit, which needs Screen Recording.
pub fn system_audio_permission() -> &'static str {
    if !sp_audio_io::macos_sck::process_taps_supported() {
        // CGPreflightScreenCaptureAccess never prompts; "false" also covers "not asked yet".
        // https://developer.apple.com/documentation/coregraphics/cgpreflightscreencaptureaccess()
        // SAFETY: a plain status query without arguments.
        return if unsafe { CGPreflightScreenCaptureAccess() } {
            "granted"
        } else {
            "unknown"
        };
    }
    // SAFETY: dlopen/dlsym with valid C strings; the symbol has the documented preflight
    // signature `int TCCAccessPreflight(CFStringRef service, CFDictionaryRef options)`.
    unsafe {
        let handle = dlopen(
            c"/System/Library/PrivateFrameworks/TCC.framework/Versions/A/TCC".as_ptr(),
            RTLD_LAZY,
        );
        if handle.is_null() {
            return "unknown";
        }
        let symbol = dlsym(handle, c"TCCAccessPreflight".as_ptr());
        if symbol.is_null() {
            return "unknown";
        }
        let preflight = std::mem::transmute::<
            *mut c_void,
            unsafe extern "C" fn(*const c_void, *const c_void) -> i32,
        >(symbol);
        let service: Retained<NSString> = NSString::from_str("kTCCServiceAudioCapture");
        match preflight(Retained::as_ptr(&service).cast(), std::ptr::null()) {
            0 => "granted",
            1 => "denied",
            2 => "notDetermined",
            _ => "unknown",
        }
    }
}

// ------------------------------------------------------------------ login item

/// `SMAppServiceStatus` values.
/// https://developer.apple.com/documentation/servicemanagement/smappservice/status-swift.enum
const LOGIN_ITEM_ENABLED: isize = 1;
const LOGIN_ITEM_REQUIRES_APPROVAL: isize = 2;
/// A launch this soon after the user logged in counts as the login item starting.
const LOGIN_WINDOW_SECS: i64 = 120;

/// `SMAppService.mainAppService`: SoundPush itself as a login item, listed in System Settings →
/// General → Login Items. `None` before macOS 13.
/// https://developer.apple.com/documentation/servicemanagement/smappservice/mainapp
fn main_app_service() -> Option<Retained<AnyObject>> {
    let class = AnyClass::get(c"SMAppService")?;
    // SAFETY: +[SMAppService mainAppService] takes no arguments and returns an SMAppService.
    unsafe { msg_send![class, mainAppService] }
}

/// Login items through `SMAppService` are available (macOS 13+).
pub fn login_items_supported() -> bool {
    AnyClass::get(c"SMAppService").is_some()
}

fn login_item_status() -> Option<isize> {
    autoreleasepool(|_| {
        let service = main_app_service()?;
        // SAFETY: `status` is a read-only NSInteger property.
        Some(unsafe { msg_send![&*service, status] })
    })
}

/// SoundPush is registered to open at login, but the user turned it off in Login Items.
pub fn login_item_needs_approval() -> bool {
    login_item_status() == Some(LOGIN_ITEM_REQUIRES_APPROVAL)
}

/// Register or unregister the login item. Registering again while the user has it switched off
/// in System Settings would not turn it on, so that state is left alone.
/// https://developer.apple.com/documentation/servicemanagement/smappservice/register()
pub fn set_login_item(enabled: bool) -> Result<(), String> {
    autoreleasepool(|_| {
        let service = main_app_service().ok_or("login items need macOS 13 or later")?;
        // SAFETY: `status` is a read-only NSInteger property.
        let status: isize = unsafe { msg_send![&*service, status] };
        let registered = matches!(status, LOGIN_ITEM_ENABLED | LOGIN_ITEM_REQUIRES_APPROVAL);
        if enabled == registered {
            return Ok(());
        }
        let mut error: *mut AnyObject = std::ptr::null_mut();
        let out = std::ptr::from_mut(&mut error);
        // SAFETY: both methods take an `NSError **` out parameter and return BOOL.
        let ok: bool = unsafe {
            if enabled {
                msg_send![&*service, registerAndReturnError: out]
            } else {
                msg_send![&*service, unregisterAndReturnError: out]
            }
        };
        if ok {
            return Ok(());
        }
        // SAFETY: on failure the out parameter is nil or an autoreleased NSError.
        let message = unsafe { error.as_ref() }
            .and_then(|e| unsafe { msg_send![e, localizedDescription] })
            .map(|d: Retained<NSString>| d.to_string());
        Err(message.unwrap_or_else(|| "the login item could not be changed".into()))
    })
}

/// Whether this launch is the login item starting at login. `SMAppService` opens the app without
/// arguments (unlike `--autostart` elsewhere), so a launch counts when the login item is enabled
/// and the user's console login (utmpx) happened moments ago.
pub fn launched_at_login() -> bool {
    if login_item_status() != Some(LOGIN_ITEM_ENABLED) {
        return false;
    }
    let text = |chars: &[c_char]| -> String {
        let bytes: Vec<u8> = chars
            .iter()
            .take_while(|&&c| c != 0)
            .map(|&c| c as u8)
            .collect();
        String::from_utf8_lossy(&bytes).into_owned()
    };
    let user = std::env::var("USER").unwrap_or_default();
    let mut login: Option<i64> = None;
    // SAFETY: getutxent returns a record valid until the next call; the fields are copied out.
    unsafe {
        libc::setutxent();
        while let Some(e) = libc::getutxent().as_ref() {
            if e.ut_type == libc::USER_PROCESS
                && text(&e.ut_line) == "console"
                && text(&e.ut_user) == user
            {
                login = login.max(Some(e.ut_tv.tv_sec));
            }
        }
        libc::endutxent();
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    login.is_some_and(|at| (0..LOGIN_WINDOW_SECS).contains(&(now - at)))
}

// ------------------------------------------------------------------ devices

unsafe extern "C" fn on_property(
    _object: u32,
    count: u32,
    addresses: *const PropertyAddress,
    client: *mut c_void,
) -> i32 {
    if client.is_null() || addresses.is_null() {
        return 0;
    }
    // SAFETY: `client` is the leaked Mutex<Sender> from `watch_devices`; Core Audio passes
    // `count` valid addresses.
    let (tx, addresses) = unsafe {
        (
            &*client.cast::<Mutex<Sender<Change>>>(),
            std::slice::from_raw_parts(addresses, count as usize),
        )
    };
    let mut change = Change::LIST;
    for a in addresses {
        match a.selector {
            DEFAULT_INPUT => change.default_input = true,
            DEFAULT_OUTPUT => change.default_output = true,
            _ => {}
        }
    }
    if let Ok(tx) = tx.lock() {
        let _ = tx.send(change);
    }
    0
}

/// Listen for default input/output and device list changes for the life of the process.
pub fn watch_devices(tx: Sender<Change>) {
    // Deliver notifications on Core Audio's own thread rather than the main run loop.
    let no_run_loop: *const c_void = std::ptr::null();
    // SAFETY: setting the run loop property to NULL with a pointer-sized value.
    unsafe {
        AudioObjectSetPropertyData(
            SYSTEM_OBJECT,
            &address(RUN_LOOP, SCOPE_GLOBAL),
            0,
            std::ptr::null(),
            std::mem::size_of::<*const c_void>() as u32,
            std::ptr::from_ref(&no_run_loop).cast(),
        );
    }
    // Leaked on purpose: listeners stay registered until the process exits.
    let client = Box::into_raw(Box::new(Mutex::new(tx))).cast::<c_void>();
    for selector in [DEVICES, DEFAULT_INPUT, DEFAULT_OUTPUT] {
        // SAFETY: valid address, a listener with the Core Audio signature, and a client pointer
        // that is never freed.
        let status = unsafe {
            AudioObjectAddPropertyListener(
                SYSTEM_OBJECT,
                &address(selector, SCOPE_GLOBAL),
                on_property,
                client,
            )
        };
        if status != 0 {
            warn!(status, "could not listen for audio device changes");
        }
    }
}

fn device_ids() -> Vec<u32> {
    let addr = address(DEVICES, SCOPE_GLOBAL);
    let mut size = 0u32;
    // SAFETY: size query, then a buffer of exactly that many bytes.
    unsafe {
        if AudioObjectGetPropertyDataSize(SYSTEM_OBJECT, &addr, 0, std::ptr::null(), &mut size) != 0
        {
            return Vec::new();
        }
        let mut ids = vec![0u32; size as usize / std::mem::size_of::<u32>()];
        let mut size = (ids.len() * std::mem::size_of::<u32>()) as u32;
        if AudioObjectGetPropertyData(
            SYSTEM_OBJECT,
            &addr,
            0,
            std::ptr::null(),
            &mut size,
            ids.as_mut_ptr().cast(),
        ) != 0
        {
            return Vec::new();
        }
        ids.truncate(size as usize / std::mem::size_of::<u32>());
        ids
    }
}

fn u32_property(device: u32, selector: u32) -> Option<u32> {
    let mut value = 0u32;
    let mut size = std::mem::size_of::<u32>() as u32;
    // SAFETY: a u32 out value with its size.
    let status = unsafe {
        AudioObjectGetPropertyData(
            device,
            &address(selector, SCOPE_GLOBAL),
            0,
            std::ptr::null(),
            &mut size,
            std::ptr::from_mut(&mut value).cast(),
        )
    };
    (status == 0).then_some(value)
}

fn device_name(device: u32) -> Option<String> {
    let mut string: *const c_void = std::ptr::null();
    let mut size = std::mem::size_of::<*const c_void>() as u32;
    // SAFETY: the property returns a retained CFString, released below.
    unsafe {
        let status = AudioObjectGetPropertyData(
            device,
            &address(NAME, SCOPE_GLOBAL),
            0,
            std::ptr::null(),
            &mut size,
            std::ptr::from_mut(&mut string).cast(),
        );
        if status != 0 || string.is_null() {
            return None;
        }
        let mut buf = [0 as c_char; 256];
        let ok = CFStringGetCString(string, buf.as_mut_ptr(), buf.len() as isize, UTF8) != 0;
        CFRelease(string);
        ok.then(|| {
            std::ffi::CStr::from_ptr(buf.as_ptr())
                .to_string_lossy()
                .to_string()
        })
    }
}

fn has_output(device: u32) -> bool {
    let mut size = 0u32;
    // SAFETY: size query only.
    let status = unsafe {
        AudioObjectGetPropertyDataSize(
            device,
            &address(STREAMS, SCOPE_OUTPUT),
            0,
            std::ptr::null(),
            &mut size,
        )
    };
    status == 0 && size > 0
}

pub fn bluetooth_outputs() -> Vec<String> {
    device_ids()
        .into_iter()
        .filter(|&d| {
            has_output(d)
                && matches!(
                    u32_property(d, TRANSPORT),
                    Some(TRANSPORT_BLUETOOTH | TRANSPORT_BLUETOOTH_LE)
                )
        })
        .filter_map(device_name)
        .collect()
}

/// Whether any process does I/O on the device named `name`. SoundPush's own feed counts too,
/// so this answers "did an app open the virtual microphone" only while no route feeds it.
pub fn device_running_somewhere(name: &str) -> bool {
    device_ids()
        .into_iter()
        .find(|&d| device_name(d).as_deref() == Some(name))
        .and_then(|d| u32_property(d, RUNNING_SOMEWHERE))
        .is_some_and(|running| running != 0)
}
