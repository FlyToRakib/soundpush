//! Real-time priority for the threads that carry audio (plan §13.3, §12.3).
//!
//! A capture or render callback runs on a thread the OS audio stack owns, so SoundPush cannot
//! create it with the right priority. Instead the first callback promotes the thread it is
//! already running on. The promotion lives in a thread-local, so it is undone by the thread's own
//! destructor when the audio stack ends the thread — the only place where undoing it is valid on
//! Windows and Linux.
//!
//! Everything here fails softly. A machine, container or policy that does not allow real-time
//! scheduling keeps the normal priority; audio then works exactly as before, only less protected
//! against a game or build using every core (plan §8.3 "CPU starvation").
//!
//! Windows uses MMCSS ("Pro Audio"), macOS the thread time-constraint policy, and Linux
//! `SCHED_RR` with a `nice` fallback. Nothing is promoted while [`set_enabled`] is off (the
//! "Real-time audio priority" advanced override).

use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::{CaptureCallback, RenderCallback};

/// Audio callbacks are expected about this often; macOS needs it to describe the deadline.
const PERIOD_MS: u32 = 10;

static ENABLED: AtomicBool = AtomicBool::new(true);

/// Switch promotion on or off for threads started from now on ("Real-time audio priority").
/// Threads that are already promoted keep their priority until they end.
pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

thread_local! {
    /// Set once this thread has been through [`promote_audio_thread`], successfully or not, so
    /// a callback running every few milliseconds asks the OS exactly once.
    static TRIED: Cell<bool> = const { Cell::new(false) };
    /// Holds the promotion for the life of the thread; dropped by the thread-local destructor.
    static PROMOTION: RefCell<Option<sys::Promotion>> = const { RefCell::new(None) };
}

/// Promote the calling thread to real-time priority, once per thread. Safe to call from an audio
/// callback: after the first call it is a thread-local read and nothing else.
#[inline]
pub fn promote_audio_thread() {
    if TRIED.with(Cell::get) {
        return;
    }
    TRIED.with(|tried| tried.set(true));
    if !enabled() {
        return;
    }
    let promotion = sys::Promotion::acquire(PERIOD_MS);
    if promotion.is_none() {
        // Once per thread, and never from a thread that already carries audio.
        tracing::debug!("the OS did not grant real-time priority to an audio thread");
    }
    PROMOTION.with(|slot| {
        if let Ok(mut slot) = slot.try_borrow_mut() {
            *slot = promotion;
        }
    });
}

/// Wrap a capture callback so the thread delivering it runs at real-time priority.
pub fn realtime_capture(mut on_audio: CaptureCallback) -> CaptureCallback {
    Box::new(move |frames| {
        promote_audio_thread();
        on_audio(frames);
    })
}

/// Wrap a render callback so the thread asking for audio runs at real-time priority.
pub fn realtime_render(mut on_audio: RenderCallback) -> RenderCallback {
    Box::new(move |frames| {
        promote_audio_thread();
        on_audio(frames);
    })
}

#[cfg(windows)]
mod sys {
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::Threading::{
        AvRevertMmThreadCharacteristics, AvSetMmThreadCharacteristicsW,
    };
    use windows::core::w;

    /// An MMCSS registration. The scheduler gives "Pro Audio" threads a guaranteed share of the
    /// CPU even while other processes saturate it.
    pub(super) struct Promotion(HANDLE);

    impl Promotion {
        pub(super) fn acquire(_period_ms: u32) -> Option<Self> {
            let mut task_index = 0u32;
            // SAFETY: the task name is a static wide string and `task_index` is a live local the
            // call only writes to. The returned handle belongs to this thread and is reverted in
            // `Drop`, which runs on the same thread (thread-local destructor).
            let handle = unsafe { AvSetMmThreadCharacteristicsW(w!("Pro Audio"), &mut task_index) };
            handle.ok().filter(|h| !h.is_invalid()).map(Promotion)
        }
    }

    impl Drop for Promotion {
        fn drop(&mut self) {
            // SAFETY: `self.0` came from AvSetMmThreadCharacteristicsW on this same thread and is
            // reverted exactly once.
            let _ = unsafe { AvRevertMmThreadCharacteristics(self.0) };
        }
    }
}

#[cfg(target_os = "macos")]
mod sys {
    /// `THREAD_TIME_CONSTRAINT_POLICY` and the word count of its policy structure.
    const THREAD_TIME_CONSTRAINT_POLICY: i32 = 2;
    const THREAD_TIME_CONSTRAINT_POLICY_COUNT: u32 = 4;
    const THREAD_STANDARD_POLICY: i32 = 1;
    const THREAD_STANDARD_POLICY_COUNT: u32 = 1;
    const KERN_SUCCESS: i32 = 0;

    #[repr(C)]
    #[derive(Default)]
    struct TimeConstraintPolicy {
        period: u32,
        computation: u32,
        constraint: u32,
        preemptible: u32,
    }

    #[repr(C)]
    #[derive(Default)]
    struct TimebaseInfo {
        numer: u32,
        denom: u32,
    }

    // libSystem, always linked on macOS.
    unsafe extern "C" {
        fn mach_thread_self() -> u32;
        fn mach_port_deallocate(task: u32, name: u32) -> i32;
        fn thread_policy_set(thread: u32, flavor: i32, info: *mut u32, count: u32) -> i32;
        fn mach_timebase_info(info: *mut TimebaseInfo) -> i32;
        static mach_task_self_: u32;
    }

    /// Milliseconds as mach absolute-time units (the unit the scheduler policy speaks).
    fn abs_time(ms: u32) -> u32 {
        let mut info = TimebaseInfo::default();
        // SAFETY: `info` is a live local of the layout the call expects.
        if unsafe { mach_timebase_info(&mut info) } != KERN_SUCCESS
            || info.numer == 0
            || info.denom == 0
        {
            return ms * 1_000_000;
        }
        let nanos = u64::from(ms) * 1_000_000;
        u32::try_from(nanos * u64::from(info.denom) / u64::from(info.numer)).unwrap_or(u32::MAX)
    }

    /// A mach thread port whose scheduling policy was set to the real-time (time-constraint) one.
    pub(super) struct Promotion(u32);

    impl Promotion {
        pub(super) fn acquire(period_ms: u32) -> Option<Self> {
            // SAFETY: adds a reference to this thread's mach port; released in `Drop`.
            let thread = unsafe { mach_thread_self() };
            let period = abs_time(period_ms.max(1));
            let mut policy = TimeConstraintPolicy {
                period,
                // Deadline work per period: half the period is what Core Audio clients ask for.
                computation: period / 2,
                constraint: period,
                // Deadline work is not preemptible by other real-time threads.
                preemptible: 0,
            };
            // SAFETY: `policy` is a live local with exactly COUNT 32-bit words, matching the
            // flavour passed alongside it.
            let result = unsafe {
                thread_policy_set(
                    thread,
                    THREAD_TIME_CONSTRAINT_POLICY,
                    std::ptr::from_mut(&mut policy).cast(),
                    THREAD_TIME_CONSTRAINT_POLICY_COUNT,
                )
            };
            if result != KERN_SUCCESS {
                // SAFETY: releases the reference taken above; the port is not used again.
                unsafe { mach_port_deallocate(mach_task_self_, thread) };
                return None;
            }
            Some(Promotion(thread))
        }
    }

    impl Drop for Promotion {
        fn drop(&mut self) {
            let mut standard = 0u32;
            // SAFETY: the standard policy structure is a single 32-bit word, and `self.0` is the
            // port taken in `acquire` on this same thread.
            unsafe {
                thread_policy_set(
                    self.0,
                    THREAD_STANDARD_POLICY,
                    std::ptr::from_mut(&mut standard),
                    THREAD_STANDARD_POLICY_COUNT,
                );
                mach_port_deallocate(mach_task_self_, self.0);
            }
        }
    }
}

#[cfg(target_os = "linux")]
mod sys {
    /// A thread moved to `SCHED_RR`, or given a better `nice` value where real-time scheduling is
    /// not permitted (the usual case without `RLIMIT_RTPRIO` or an RT group).
    pub(super) struct Promotion {
        realtime: bool,
    }

    /// Just above ordinary work and far below a driver's own threads: audio needs to run first,
    /// not to be able to lock up the machine.
    fn realtime_priority() -> i32 {
        // SAFETY: both calls only read scheduler limits for a policy constant.
        let (min, max) = unsafe {
            (
                libc::sched_get_priority_min(libc::SCHED_RR),
                libc::sched_get_priority_max(libc::SCHED_RR),
            )
        };
        if min < 0 || max < min {
            return 1;
        }
        (min + 5).min(max)
    }

    impl Promotion {
        pub(super) fn acquire(_period_ms: u32) -> Option<Self> {
            let param = libc::sched_param {
                sched_priority: realtime_priority(),
            };
            // SAFETY: `param` is a live local of the expected layout, applied to this thread.
            let rt = unsafe {
                libc::pthread_setschedparam(libc::pthread_self(), libc::SCHED_RR, &param)
            };
            if rt == 0 {
                return Some(Promotion { realtime: true });
            }
            // No real-time scheduling here: a better nice value still helps under load.
            // SAFETY: `who = 0` is this thread on Linux, where nice values are per-thread.
            let niced = unsafe { libc::setpriority(libc::PRIO_PROCESS, 0, -10) };
            (niced == 0).then_some(Promotion { realtime: false })
        }
    }

    impl Drop for Promotion {
        fn drop(&mut self) {
            if self.realtime {
                let param = libc::sched_param { sched_priority: 0 };
                // SAFETY: as in `acquire`, on the thread that was promoted.
                unsafe {
                    libc::pthread_setschedparam(libc::pthread_self(), libc::SCHED_OTHER, &param)
                };
            } else {
                // SAFETY: as in `acquire`.
                unsafe { libc::setpriority(libc::PRIO_PROCESS, 0, 0) };
            }
        }
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
mod sys {
    /// Android leaves this to Oboe/AAudio, which creates its own real-time callback thread.
    pub(super) struct Promotion;

    impl Promotion {
        pub(super) fn acquire(_period_ms: u32) -> Option<Self> {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One test, because the switch and the "asked once" flag are process- and thread-wide.
    /// Promotion itself is left off so a test runner is never moved to a real-time policy.
    #[test]
    fn audio_keeps_flowing_whether_or_not_the_os_grants_priority() {
        assert!(enabled(), "on unless an override says otherwise");
        set_enabled(false);
        assert!(!enabled());

        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let seen = calls.clone();
        let mut capture = realtime_capture(Box::new(move |frames: &[f32]| {
            assert_eq!(frames.len(), 2);
            seen.fetch_add(1, Ordering::Relaxed);
        }));
        for _ in 0..3 {
            capture(&[0.0, 0.0]);
        }
        assert_eq!(calls.load(Ordering::Relaxed), 3);
        assert!(TRIED.with(Cell::get), "the OS is asked once per thread");

        let mut render = realtime_render(Box::new(|frames: &mut [f32]| frames.fill(0.5)));
        let mut buf = [0.0f32; 4];
        render(&mut buf);
        assert_eq!(buf, [0.5; 4]);

        set_enabled(true);
    }
}
