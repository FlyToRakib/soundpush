//! Android process context for native audio.
//!
//! cpal's AAudio backend reads the JavaVM and application Context from
//! `ndk-context`. Without this, enumerating or opening audio devices panics.

use std::sync::Once;

use jni::JNIEnv;
use jni::objects::{JClass, JObject};

static INIT: Once = Once::new();

/// Called once from `net.soundpush.engine.NativeContext.nativeInit(context)` before the engine starts.
#[unsafe(no_mangle)]
pub extern "system" fn Java_net_soundpush_engine_NativeContext_nativeInit(
    env: JNIEnv,
    _class: JClass,
    context: JObject,
) {
    INIT.call_once(|| {
        let Ok(vm) = env.get_java_vm() else { return };
        let Ok(global) = env.new_global_ref(context) else {
            return;
        };
        // SAFETY: the JavaVM lives for the whole process, and the global reference to the
        // application Context is intentionally leaked so it stays valid for the process lifetime.
        unsafe {
            ndk_context::initialize_android_context(
                vm.get_java_vm_pointer().cast(),
                global.as_obj().as_raw().cast(),
            );
        }
        std::mem::forget(global);
    });
}

/// Writes formatted `tracing` events to logcat (through `android_logger`), one record per event,
/// at the event's level.
pub(crate) struct Logcat;

pub(crate) struct LogcatLine {
    level: log::Level,
    buf: Vec<u8>,
}

impl std::io::Write for LogcatLine {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        self.buf.extend_from_slice(data);
        Ok(data.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Drop for LogcatLine {
    fn drop(&mut self) {
        let text = String::from_utf8_lossy(&self.buf);
        let text = text.trim_end();
        if !text.is_empty() {
            log::log!(target: "SoundPush", self.level, "{text}");
        }
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Logcat {
    type Writer = LogcatLine;

    fn make_writer(&'a self) -> Self::Writer {
        LogcatLine {
            level: log::Level::Info,
            buf: Vec::new(),
        }
    }

    fn make_writer_for(&'a self, meta: &tracing::Metadata<'_>) -> Self::Writer {
        let level = match *meta.level() {
            tracing::Level::ERROR => log::Level::Error,
            tracing::Level::WARN => log::Level::Warn,
            tracing::Level::INFO => log::Level::Info,
            tracing::Level::DEBUG => log::Level::Debug,
            tracing::Level::TRACE => log::Level::Trace,
        };
        LogcatLine {
            level,
            buf: Vec::new(),
        }
    }
}
