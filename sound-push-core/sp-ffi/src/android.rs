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
