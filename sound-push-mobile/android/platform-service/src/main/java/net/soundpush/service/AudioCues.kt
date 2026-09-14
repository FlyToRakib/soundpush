package net.soundpush.service

import android.media.AudioManager
import android.media.ToneGenerator
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.filterNotNull
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.launch
import net.soundpush.engine.SoundPush

/**
 * "Sound on connect and disconnect" (settings.audioCues): a short rising tone when a stream
 * starts playing and a falling one when it ends. Useful with a screen reader or the screen off.
 */
object AudioCues {
    fun observe(scope: CoroutineScope) {
        scope.launch {
            var previous: Set<String>? = null
            SoundPush.state.filterNotNull()
                .map { s -> s.settings.audioCues to s.routes.filter { it.status == "active" }.map { it.routeId }.toSet() }
                .distinctUntilChanged()
                .collect { (enabled, active) ->
                    val before = previous
                    previous = active
                    if (!enabled || before == null) return@collect
                    when {
                        (active - before).isNotEmpty() -> play(ToneGenerator.TONE_PROP_ACK, scope)
                        (before - active).isNotEmpty() -> play(ToneGenerator.TONE_PROP_NACK, scope)
                    }
                }
        }
    }

    private fun play(tone: Int, scope: CoroutineScope) {
        val generator = runCatching { ToneGenerator(AudioManager.STREAM_NOTIFICATION, 60) }.getOrNull() ?: return
        generator.startTone(tone, 200)
        scope.launch {
            delay(400)
            generator.release()
        }
    }
}
