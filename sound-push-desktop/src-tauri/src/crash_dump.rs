//! Minidumps for crashes the panic hook never sees (plan §13.2, §8.4 "Desktop app crash").
//!
//! A Rust panic leaves a text report through `sp_engine::crash`. An access violation, a
//! `SIGSEGV` in a driver or a stack overflow unwinds nothing, so the process disappears without a
//! trace. `crash-handler` catches those and this module writes a minidump plus a short text note
//! into the same `crash-reports` folder, which means the existing "SoundPush closed unexpectedly"
//! notice on the next start and the diagnostics export find it with no further work.
//!
//! The privacy stance is unchanged: everything is written locally and nothing is ever uploaded.
//! A minidump holds process memory, so diagnostics exports keep including only the text note; the
//! user can attach the `.dmp` by hand from the crash-reports folder if they want to.
//!
//! The handler runs in a compromised process, so it does as little as possible: format a short
//! string, write two files, and let the default handler finish the crash.

use std::path::Path;
use std::sync::OnceLock;

use crash_handler::{CrashContext, CrashEventResult, CrashHandler};
use tracing::{info, warn};

/// Kept for the life of the process; dropping it would remove the handler.
static HANDLER: OnceLock<CrashHandler> = OnceLock::new();

/// Install the crash handler once per process. Later calls are ignored, and a platform that
/// refuses the handler simply keeps the panic hook.
pub fn install(data_dir: &Path, app_version: &str) {
    if HANDLER.get().is_some() {
        return;
    }
    let dir = sp_engine::crash::report_dir(data_dir);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        warn!(error = %e, "no crash-report folder; minidumps are off");
        return;
    }
    let data_dir = data_dir.to_path_buf();
    let version = app_version.to_string();
    // SAFETY: the closure runs in a crashed process. It only formats a short string and writes
    // files, allocating no more than the writer itself does, and never touches engine state,
    // locks or the audio threads. `make_crash_event` is unsafe for exactly that reason.
    let event = unsafe {
        crash_handler::make_crash_event(move |context: &CrashContext| {
            on_crash(&data_dir, &dir, &version, context);
            // Let the OS carry on with the crash: the process must still die, and a debugger or
            // the Windows Restart Manager gets its turn.
            CrashEventResult::Handled(false)
        })
    };
    match CrashHandler::attach(event) {
        Ok(handler) => {
            let _ = HANDLER.set(handler);
            info!("crash handler installed (local minidumps, nothing uploaded)");
        }
        Err(e) => warn!(error = %e, "could not install the crash handler"),
    }
}

fn on_crash(data_dir: &Path, dir: &Path, version: &str, context: &CrashContext) {
    let stem = sp_engine::crash::crash_stem(sp_engine::crash::now_secs());
    let dump = dir.join(format!("{stem}.dmp"));
    let written = write_minidump(&dump, context);
    if !written {
        let _ = std::fs::remove_file(&dump);
    }
    sp_engine::crash::write_note(data_dir, version, &stem, &describe(context), written);
}

/// One line naming what the OS reported, so the text note is useful without a debugger.
fn describe(context: &CrashContext) -> String {
    #[cfg(windows)]
    {
        format!(
            "SoundPush stopped on a Windows exception.\nException code: {:#010x}\nThread: {}",
            context.exception_code, context.thread_id
        )
    }
    #[cfg(target_os = "linux")]
    {
        format!(
            "SoundPush stopped on signal {} (code {}).\nThread: {}",
            context.siginfo.ssi_signo, context.siginfo.ssi_code, context.tid
        )
    }
    #[cfg(target_os = "macos")]
    {
        match &context.exception {
            Some(e) => format!(
                "SoundPush stopped on a Mach exception.\nKind: {}\nCode: {:#x}\nThread: {}",
                e.kind, e.code, context.thread
            ),
            None => format!(
                "SoundPush stopped unexpectedly.\nThread: {}",
                context.thread
            ),
        }
    }
}

/// Write the minidump, returning whether one ended up on disk at a sensible size.
fn write_minidump(path: &Path, context: &CrashContext) -> bool {
    let Ok(mut file) = std::fs::File::create(path) else {
        return false;
    };
    let ok = write_platform_minidump(&mut file, context);
    drop(file);
    ok && std::fs::metadata(path)
        .is_ok_and(|m| m.len() > 0 && m.len() <= sp_engine::crash::MAX_MINIDUMP_BYTES)
}

#[cfg(windows)]
fn write_platform_minidump(file: &mut std::fs::File, context: &CrashContext) -> bool {
    // SAFETY (the writer's own requirement): the exception pointers come from the exception
    // filter that is still on this thread's stack, so they stay valid for this call.
    minidump_writer::minidump_writer::MinidumpWriter::dump_crash_context(context, None, file)
        .is_ok()
}

#[cfg(target_os = "linux")]
fn write_platform_minidump(file: &mut std::fs::File, context: &CrashContext) -> bool {
    minidump_writer::minidump_writer::MinidumpWriter::new(context.pid, context.tid)
        .write(file)
        .is_ok()
}

#[cfg(target_os = "macos")]
fn write_platform_minidump(file: &mut std::fs::File, context: &CrashContext) -> bool {
    // The handler thread is excluded from the dump; the crashing thread is in it.
    minidump_writer::minidump_writer::MinidumpWriter::new(
        Some(context.task),
        Some(context.handler_thread),
    )
    .dump(file)
    .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Installing must never fail the start, and a second call is ignored. No crash is simulated:
    /// the handler is process-wide and would take the test runner down with it.
    #[test]
    fn installing_twice_is_harmless_and_leaves_the_report_folder_ready() {
        let dir = tempfile::tempdir().unwrap();
        install(dir.path(), "0.0.0-test");
        install(dir.path(), "0.0.0-test");
        assert!(sp_engine::crash::report_dir(dir.path()).is_dir());
    }
}
