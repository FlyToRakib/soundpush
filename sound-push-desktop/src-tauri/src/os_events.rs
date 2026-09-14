//! Network changes, sleep/resume and session end from the OS → engine (plan §8.1 "wakes
//! immediately on OS network-change events", §8.3 "PC sleep/hibernate while streaming", §13.2
//! Power and Network integration, §24 crash-restart).
//!
//! - **Windows:** `NotifyIpInterfaceChange`; a hidden top-level window receives
//!   `WM_POWERBROADCAST` and `WM_QUERYENDSESSION`/`WM_ENDSESSION` (Restart Manager, log off),
//!   and `RegisterApplicationRestart` lets installers and updaters restart SoundPush.
//! - **macOS:** `IORegisterForSystemPower` and SystemConfiguration dynamic-store keys for the
//!   primary network and interface addresses, on one run-loop thread.
//! - **Linux:** rtnetlink address notifications and logind's `PrepareForSleep` signal.
//!
//! OS callbacks only send an [`Event`]; one thread merges bursts (joining a network changes
//! several addresses) and calls the engine.

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager};
use tracing::{info, warn};

use crate::AppState;
use crate::hooks::DesktopHooks;

/// Quiet period that ends a burst of events.
const SETTLE: Duration = Duration::from_millis(1500);
/// A burst that never goes quiet is still acted on after this long.
const MAX_BURST: Duration = Duration::from_secs(10);
/// Event the UI listens to, to check the firewall and USB tethering again.
pub const NETWORK_EVENT: &str = "platform://network-changed";

#[cfg_attr(target_os = "linux", allow(dead_code))]
pub enum Event {
    Network,
    Suspend,
    Resume,
    /// The user logs off, Windows shuts down, or the Restart Manager closes SoundPush for an
    /// installer (`close_app`). Answered on `done` once the engine has stopped.
    SessionEnding {
        close_app: bool,
        done: Sender<()>,
    },
}

pub fn start(app: AppHandle, hooks: Arc<DesktopHooks>) {
    let (tx, rx) = mpsc::channel::<Event>();
    #[cfg(windows)]
    windows::watch(tx);
    #[cfg(target_os = "macos")]
    macos::watch(tx);
    #[cfg(target_os = "linux")]
    linux::watch(tx);
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    drop(tx);

    let spawned = std::thread::Builder::new()
        .name("sp-os-events".into())
        .spawn(move || dispatch(&app, &hooks, &rx));
    if let Err(e) = spawned {
        warn!(error = %e, "could not watch network and power events");
    }
}

fn dispatch(app: &AppHandle, hooks: &DesktopHooks, rx: &Receiver<Event>) {
    while let Ok(first) = rx.recv() {
        let (mut network, mut resumed) = (false, false);
        let started = Instant::now();
        let mut next = Some(first);
        while let Some(event) = next.take() {
            match event {
                Event::Network => network = true,
                Event::Suspend => info!("the computer is going to sleep"),
                Event::Resume => resumed = true,
                Event::SessionEnding { close_app, done } => {
                    end_session(app, close_app);
                    let _ = done.send(());
                }
            }
            if started.elapsed() < MAX_BURST {
                next = rx.recv_timeout(SETTLE).ok();
            }
        }

        let Some(engine) = app
            .try_state::<AppState>()
            .and_then(|s| s.engine.get().cloned())
        else {
            continue;
        };
        if resumed {
            // Audio clients do not survive sleep on every driver: reopen routes on the default
            // devices and refresh the list. Sessions are redialed below.
            info!("the computer woke up");
            let _ = engine.audio_devices_changed(true, true);
        }
        if network || resumed {
            info!(resumed, "network changed");
            let _ = engine.network_changed();
            if cfg!(windows) {
                hooks.set_network_status(crate::network::status());
            }
            let _ = app.emit(NETWORK_EVENT, ());
        }
    }
}

/// Stop the engine before the session ends, so speakers are unmuted and peers are told.
fn end_session(app: &AppHandle, close_app: bool) {
    info!(close_app, "session ending");
    if let Some(engine) = app
        .try_state::<AppState>()
        .and_then(|s| s.engine.get().cloned())
    {
        engine.shutdown(Duration::from_secs(2));
    }
    if close_app {
        // The Restart Manager waits for the process to exit and restarts it afterwards.
        app.exit(0);
    }
}

/// Let installers and updaters (Restart Manager) restart SoundPush, and Windows restart it
/// after a crash or hang. Not after a reboot: launch at login covers that.
pub fn register_restart() {
    #[cfg(windows)]
    windows::register_restart();
}

#[cfg(windows)]
mod windows {
    use std::ffi::c_void;
    use std::sync::mpsc::{Sender, channel};
    use std::sync::{Mutex, OnceLock};
    use std::time::Duration;

    use tracing::{info, warn};
    use windows::Win32::Foundation::{BOOLEAN, HANDLE, HWND, LPARAM, LRESULT, NO_ERROR, WPARAM};
    use windows::Win32::NetworkManagement::IpHelper::{
        MIB_IPINTERFACE_ROW, MIB_NOTIFICATION_TYPE, NotifyIpInterfaceChange,
    };
    use windows::Win32::Networking::WinSock::AF_UNSPEC;
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::System::Recovery::{RESTART_NO_REBOOT, RegisterApplicationRestart};
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, ENDSESSION_CLOSEAPP, GetMessageW, HMENU,
        MSG, PBT_APMRESUMEAUTOMATIC, PBT_APMRESUMESUSPEND, PBT_APMSUSPEND, RegisterClassW,
        TranslateMessage, WINDOW_EX_STYLE, WM_ENDSESSION, WM_POWERBROADCAST, WM_QUERYENDSESSION,
        WNDCLASSW, WS_OVERLAPPED,
    };
    use windows::core::w;

    use super::Event;

    /// For the window procedure and the IP helper callback, which get no Rust context.
    static EVENTS: OnceLock<Mutex<Sender<Event>>> = OnceLock::new();

    fn send(event: Event) {
        if let Some(tx) = EVENTS.get().and_then(|t| t.lock().ok()) {
            let _ = tx.send(event);
        }
    }

    pub fn register_restart() {
        // SAFETY: a static command line and a valid flag.
        if let Err(e) = unsafe { RegisterApplicationRestart(w!("--autostart"), RESTART_NO_REBOOT) }
        {
            warn!(error = %e, "could not register for application restart");
        }
    }

    unsafe extern "system" fn on_ip_change(
        _context: *const c_void,
        _row: *const MIB_IPINTERFACE_ROW,
        _kind: MIB_NOTIFICATION_TYPE,
    ) {
        send(Event::Network);
    }

    unsafe extern "system" fn window_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match msg {
            WM_POWERBROADCAST => {
                match wparam.0 as u32 {
                    PBT_APMSUSPEND => send(Event::Suspend),
                    PBT_APMRESUMEAUTOMATIC | PBT_APMRESUMESUSPEND => send(Event::Resume),
                    _ => {}
                }
                LRESULT(1)
            }
            // Never veto a shutdown or an installer.
            WM_QUERYENDSESSION => LRESULT(1),
            WM_ENDSESSION => {
                if wparam.0 != 0 {
                    let (done, stopped) = channel();
                    send(Event::SessionEnding {
                        close_app: (lparam.0 as u32) & ENDSESSION_CLOSEAPP != 0,
                        done,
                    });
                    // Windows allows about five seconds before it ends the process.
                    let _ = stopped.recv_timeout(Duration::from_secs(4));
                }
                LRESULT(0)
            }
            // SAFETY: default handling with the arguments Windows passed in.
            _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
        }
    }

    pub fn watch(tx: Sender<Event>) {
        if EVENTS.set(Mutex::new(tx)).is_err() {
            return;
        }
        let mut handle = HANDLE::default();
        // SAFETY: a callback that only sends on a channel; the registration lives as long as
        // the process, so the handle is never cancelled.
        let status = unsafe {
            NotifyIpInterfaceChange(AF_UNSPEC, Some(on_ip_change), None, BOOLEAN(0), &mut handle)
        };
        if status != NO_ERROR {
            warn!(?status, "could not watch network changes");
        }

        // A hidden top-level window: message-only windows do not get power and session broadcasts.
        let spawned = std::thread::Builder::new()
            .name("sp-os-window".into())
            .spawn(|| {
                // SAFETY: standard window class registration and a message loop on this thread,
                // which owns the window for the life of the process.
                unsafe {
                    let Ok(module) = GetModuleHandleW(None) else {
                        return;
                    };
                    let class = w!("SoundPushSystemEvents");
                    let wc = WNDCLASSW {
                        lpfnWndProc: Some(window_proc),
                        hInstance: module.into(),
                        lpszClassName: class,
                        ..Default::default()
                    };
                    if RegisterClassW(&wc) == 0 {
                        warn!("could not register the system events window class");
                        return;
                    }
                    let created = CreateWindowExW(
                        WINDOW_EX_STYLE::default(),
                        class,
                        w!("SoundPush"),
                        WS_OVERLAPPED,
                        0,
                        0,
                        0,
                        0,
                        HWND::default(),
                        HMENU::default(),
                        module,
                        None,
                    );
                    if let Err(e) = created {
                        warn!(error = %e, "could not create the system events window");
                        return;
                    }
                    info!("watching power and session events");
                    let mut msg = MSG::default();
                    // 0 = WM_QUIT, -1 = error.
                    while GetMessageW(&mut msg, HWND::default(), 0, 0).0 > 0 {
                        let _ = TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                }
            });
        if let Err(e) = spawned {
            warn!(error = %e, "could not start the system events thread");
        }
    }
}

#[cfg(target_os = "macos")]
mod macos {
    //! Not compiled on the development machine yet (no macOS host): plain C APIs declared by hand,
    //! matching the SDK headers.
    use std::ffi::{CStr, c_char, c_void};
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::mpsc::Sender;
    use std::sync::{Mutex, OnceLock};

    use tracing::{info, warn};

    use super::Event;

    type CFTypeRef = *const c_void;

    #[repr(C)]
    struct CFArrayCallBacks {
        version: isize,
        retain: *const c_void,
        release: *const c_void,
        copy_description: *const c_void,
        equal: *const c_void,
    }

    #[repr(C)]
    struct SCDynamicStoreContext {
        version: isize,
        info: *mut c_void,
        retain: *const c_void,
        release: *const c_void,
        copy_description: *const c_void,
    }

    type StoreCallback =
        unsafe extern "C" fn(store: CFTypeRef, changed: CFTypeRef, info: *mut c_void);
    type PowerCallback = unsafe extern "C" fn(
        refcon: *mut c_void,
        service: u32,
        message: u32,
        argument: *mut c_void,
    );

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        static kCFRunLoopDefaultMode: CFTypeRef;
        static kCFTypeArrayCallBacks: CFArrayCallBacks;
        fn CFRunLoopGetCurrent() -> CFTypeRef;
        fn CFRunLoopAddSource(run_loop: CFTypeRef, source: CFTypeRef, mode: CFTypeRef);
        fn CFRunLoopRun();
        fn CFStringCreateWithCString(
            alloc: CFTypeRef,
            string: *const c_char,
            encoding: u32,
        ) -> CFTypeRef;
        fn CFArrayCreate(
            alloc: CFTypeRef,
            values: *const CFTypeRef,
            count: isize,
            callbacks: *const CFArrayCallBacks,
        ) -> CFTypeRef;
        fn CFRelease(cf: CFTypeRef);
    }

    #[link(name = "SystemConfiguration", kind = "framework")]
    unsafe extern "C" {
        fn SCDynamicStoreCreate(
            alloc: CFTypeRef,
            name: CFTypeRef,
            callout: Option<StoreCallback>,
            context: *mut SCDynamicStoreContext,
        ) -> CFTypeRef;
        fn SCDynamicStoreSetNotificationKeys(
            store: CFTypeRef,
            keys: CFTypeRef,
            patterns: CFTypeRef,
        ) -> u8;
        fn SCDynamicStoreCreateRunLoopSource(
            alloc: CFTypeRef,
            store: CFTypeRef,
            order: isize,
        ) -> CFTypeRef;
    }

    #[link(name = "IOKit", kind = "framework")]
    unsafe extern "C" {
        fn IORegisterForSystemPower(
            refcon: *mut c_void,
            port: *mut *mut c_void,
            callback: PowerCallback,
            notifier: *mut u32,
        ) -> u32;
        fn IONotificationPortGetRunLoopSource(port: *mut c_void) -> CFTypeRef;
        fn IOAllowPowerChange(kernel_port: u32, notification: isize) -> i32;
    }

    const UTF8: u32 = 0x0800_0100;
    // iokit_common_msg(…) from IOMessage.h.
    const CAN_SYSTEM_SLEEP: u32 = 0xE000_0270;
    const SYSTEM_WILL_SLEEP: u32 = 0xE000_0280;
    const SYSTEM_HAS_POWERED_ON: u32 = 0xE000_0300;

    static EVENTS: OnceLock<Mutex<Sender<Event>>> = OnceLock::new();
    static ROOT_PORT: AtomicU32 = AtomicU32::new(0);

    fn send(event: Event) {
        if let Some(tx) = EVENTS.get().and_then(|t| t.lock().ok()) {
            let _ = tx.send(event);
        }
    }

    unsafe extern "C" fn on_power(
        _refcon: *mut c_void,
        _service: u32,
        message: u32,
        argument: *mut c_void,
    ) {
        match message {
            // Sleep must be acknowledged, or macOS waits 30 seconds before sleeping.
            CAN_SYSTEM_SLEEP => {
                // SAFETY: the notification id macOS passed, on the port it was registered with.
                unsafe { IOAllowPowerChange(ROOT_PORT.load(Ordering::Relaxed), argument as isize) };
            }
            SYSTEM_WILL_SLEEP => {
                send(Event::Suspend);
                // SAFETY: as above.
                unsafe { IOAllowPowerChange(ROOT_PORT.load(Ordering::Relaxed), argument as isize) };
            }
            SYSTEM_HAS_POWERED_ON => send(Event::Resume),
            _ => {}
        }
    }

    unsafe extern "C" fn on_network(_store: CFTypeRef, _changed: CFTypeRef, _info: *mut c_void) {
        send(Event::Network);
    }

    /// A CFString the caller releases.
    unsafe fn cf_string(s: &CStr) -> CFTypeRef {
        // SAFETY: a NUL-terminated UTF-8 string.
        unsafe { CFStringCreateWithCString(std::ptr::null(), s.as_ptr(), UTF8) }
    }

    /// A CFArray of new CFStrings; the array retains them, so they are released here.
    unsafe fn cf_string_array(strings: &[&CStr]) -> CFTypeRef {
        // SAFETY: the values are valid CFStrings for the duration of CFArrayCreate.
        unsafe {
            let values: Vec<CFTypeRef> = strings.iter().map(|s| cf_string(s)).collect();
            let array = CFArrayCreate(
                std::ptr::null(),
                values.as_ptr(),
                values.len() as isize,
                &kCFTypeArrayCallBacks,
            );
            for v in values {
                if !v.is_null() {
                    CFRelease(v);
                }
            }
            array
        }
    }

    pub fn watch(tx: Sender<Event>) {
        if EVENTS.set(Mutex::new(tx)).is_err() {
            return;
        }
        let spawned = std::thread::Builder::new()
            .name("sp-os-runloop".into())
            // SAFETY: Core Foundation, IOKit and SystemConfiguration calls on this thread's run
            // loop; registrations are kept for the life of the process.
            .spawn(|| unsafe {
                let run_loop = CFRunLoopGetCurrent();

                let mut port: *mut c_void = std::ptr::null_mut();
                let mut notifier = 0u32;
                let root = IORegisterForSystemPower(
                    std::ptr::null_mut(),
                    &mut port,
                    on_power,
                    &mut notifier,
                );
                if root == 0 || port.is_null() {
                    warn!("could not watch sleep and wake");
                } else {
                    ROOT_PORT.store(root, Ordering::Relaxed);
                    CFRunLoopAddSource(
                        run_loop,
                        IONotificationPortGetRunLoopSource(port),
                        kCFRunLoopDefaultMode,
                    );
                }

                let name = cf_string(c"SoundPush");
                let mut context = SCDynamicStoreContext {
                    version: 0,
                    info: std::ptr::null_mut(),
                    retain: std::ptr::null(),
                    release: std::ptr::null(),
                    copy_description: std::ptr::null(),
                };
                let store =
                    SCDynamicStoreCreate(std::ptr::null(), name, Some(on_network), &mut context);
                CFRelease(name);
                if store.is_null() {
                    warn!("could not watch network changes");
                } else {
                    let keys = cf_string_array(&[
                        c"State:/Network/Global/IPv4",
                        c"State:/Network/Global/IPv6",
                    ]);
                    let patterns = cf_string_array(&[c"State:/Network/Interface/[^/]+/IPv[46]"]);
                    let watching = SCDynamicStoreSetNotificationKeys(store, keys, patterns) != 0;
                    CFRelease(keys);
                    CFRelease(patterns);
                    let source = SCDynamicStoreCreateRunLoopSource(std::ptr::null(), store, 0);
                    if watching && !source.is_null() {
                        CFRunLoopAddSource(run_loop, source, kCFRunLoopDefaultMode);
                        // The run loop retains the source; the store is kept on purpose.
                        CFRelease(source);
                    } else {
                        warn!("could not watch network changes");
                    }
                }

                info!("watching network, sleep and wake");
                CFRunLoopRun();
            });
        if let Err(e) = spawned {
            warn!(error = %e, "could not start the system events thread");
        }
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use std::sync::mpsc::Sender;
    use std::time::Duration;

    use tracing::{info, warn};

    use super::Event;

    pub fn watch(tx: Sender<Event>) {
        let network = tx.clone();
        let spawned = std::thread::Builder::new()
            .name("sp-netlink".into())
            .spawn(move || netlink(&network));
        if let Err(e) = spawned {
            warn!(error = %e, "could not watch network changes");
        }
        let spawned = std::thread::Builder::new()
            .name("sp-logind".into())
            .spawn(move || logind(tx));
        if let Err(e) = spawned {
            warn!(error = %e, "could not watch sleep and wake");
        }
    }

    /// Address added or removed on any interface (joining or leaving a network). Link events
    /// are left out: Wi-Fi drivers send them for every scan.
    fn netlink(tx: &Sender<Event>) {
        // SAFETY: a netlink socket owned by this thread; the buffer outlives every recv.
        unsafe {
            let fd = libc::socket(
                libc::AF_NETLINK,
                libc::SOCK_RAW | libc::SOCK_CLOEXEC,
                libc::NETLINK_ROUTE,
            );
            if fd < 0 {
                warn!("could not open a netlink socket");
                return;
            }
            let mut addr: libc::sockaddr_nl = std::mem::zeroed();
            addr.nl_family = libc::AF_NETLINK as libc::sa_family_t;
            addr.nl_groups = (libc::RTMGRP_IPV4_IFADDR | libc::RTMGRP_IPV6_IFADDR) as u32;
            if libc::bind(
                fd,
                std::ptr::from_ref(&addr).cast(),
                std::mem::size_of::<libc::sockaddr_nl>() as libc::socklen_t,
            ) < 0
            {
                warn!("could not watch network changes (netlink)");
                libc::close(fd);
                return;
            }
            info!("watching network changes");
            let mut buf = [0u8; 16 * 1024];
            loop {
                let n = libc::recv(fd, buf.as_mut_ptr().cast(), buf.len(), 0);
                if n < 0 {
                    // ENOBUFS: messages were dropped, which still means something changed.
                    match std::io::Error::last_os_error().raw_os_error() {
                        Some(libc::EINTR) => continue,
                        Some(libc::ENOBUFS) => {}
                        _ => break,
                    }
                }
                if tx.send(Event::Network).is_err() {
                    break;
                }
            }
            libc::close(fd);
        }
    }

    /// logind announces sleep with `PrepareForSleep(true)` and wake with `PrepareForSleep(false)`.
    fn logind(tx: Sender<Event>) {
        let connection = match dbus::blocking::Connection::new_system() {
            Ok(c) => c,
            Err(e) => {
                warn!(error = %e, "no system bus; sleep and wake are not watched");
                return;
            }
        };
        let rule = dbus::message::MatchRule::new_signal(
            "org.freedesktop.login1.Manager",
            "PrepareForSleep",
        );
        let added = connection.add_match(rule, move |(sleeping,): (bool,), _, _| {
            tx.send(if sleeping {
                Event::Suspend
            } else {
                Event::Resume
            })
            .is_ok()
        });
        if let Err(e) = added {
            warn!(error = %e, "could not watch sleep and wake");
            return;
        }
        loop {
            if let Err(e) = connection.process(Duration::from_secs(300)) {
                warn!(error = %e, "stopped watching sleep and wake");
                return;
            }
        }
    }
}
