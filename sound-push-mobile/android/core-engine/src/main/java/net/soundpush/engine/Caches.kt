package net.soundpush.engine

import java.util.concurrent.CopyOnWriteArrayList

/**
 * Things the app can rebuild on demand, dropped when Android reports memory pressure
 * (plan §8.4 "Low memory on phone": `onTrimMemory` drops caches).
 *
 * Only caches belong here. A running stream's buffers, the engine's state and anything the user
 * would notice are never registered, so trimming is never heard and never interrupts a stream.
 */
object Caches {
    private val releases = CopyOnWriteArrayList<() -> Unit>()

    /** Register something to drop. Safe to call from any thread; register once per process. */
    fun onTrim(release: () -> Unit) {
        releases += release
    }

    /** Drop everything registered. A cache that fails to clear is logged, never thrown. */
    fun trim() {
        releases.forEach { release ->
            runCatching(release).onFailure {
                SoundPush.log(SoundPush.LogLevel.Warn, TAG, "could not release a cache", it)
            }
        }
    }

    private const val TAG = "Caches"
}
