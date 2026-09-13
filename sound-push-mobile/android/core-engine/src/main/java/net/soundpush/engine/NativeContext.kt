package net.soundpush.engine

import android.content.Context

/** Gives the native audio stack the JavaVM and application Context (required by AAudio via ndk-context). */
internal object NativeContext {
    @Volatile private var initialized = false

    @Synchronized
    fun init(context: Context) {
        if (initialized) return
        System.loadLibrary("soundpush_ffi")
        nativeInit(context.applicationContext)
        initialized = true
    }

    @JvmStatic
    private external fun nativeInit(context: Context)
}
