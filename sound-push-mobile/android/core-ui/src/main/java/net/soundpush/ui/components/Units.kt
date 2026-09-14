package net.soundpush.ui.components

import androidx.annotation.StringRes
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.res.stringResource

/**
 * A formatter for a string resource with one placeholder, such as `%1$d ms`, for labels formatted
 * outside composition (slider values while dragging). Units stay translatable and reorderable.
 */
@Composable
fun rememberFormat(@StringRes id: Int): (Any) -> String {
    val pattern = stringResource(id)
    return remember(pattern) { { value -> String.format(pattern, value) } }
}
