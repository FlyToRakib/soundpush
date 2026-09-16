package net.soundpush.ui.components

import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.map
import net.soundpush.engine.EngineState
import net.soundpush.engine.SoundPush

/**
 * One live number from the engine — a stream's timer or statistics, a round-trip time, the
 * microphone level — read by the composable that shows it.
 *
 * Screens are built from [SoundPush.structure], which leaves these numbers out so that a timer
 * tick does not rebuild the screen. Reading the number here instead means the tick recomposes
 * only this small piece, and only when [select]'s result actually changed.
 *
 * [key] identifies what is selected (a route id, say); [fallback] is used while the engine is not
 * running, which is also how previews and tests supply their own values.
 */
@Composable
fun <T> rememberLive(key: Any?, fallback: T, select: (EngineState) -> T?): T {
    val flow = remember(key) {
        SoundPush.state.map { state -> state?.let(select) }.distinctUntilChanged()
    }

    // The current value seeds the first frame, so a number is never drawn as its fallback first;
    // collectAsState observes every change after that.
    @Suppress("StateFlowValueCalledInComposition")
    val initial = SoundPush.state.value?.let(select)
    val live by flow.collectAsState(initial = initial)
    return live ?: fallback
}
