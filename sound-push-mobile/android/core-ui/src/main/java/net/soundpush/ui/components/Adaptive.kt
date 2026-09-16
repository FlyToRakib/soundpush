package net.soundpush.ui.components

import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.wrapContentWidth
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp

/**
 * Material window width classes (plan §14.2, adaptive layouts): compact phones, medium (small
 * tablets, unfolded foldables in portrait, large phones in landscape), expanded (tablets,
 * foldables in landscape). Computed from the window, not the screen, so split screen works.
 */
enum class WidthClass {
    Compact,
    Medium,
    Expanded,
    ;

    companion object {
        const val MEDIUM_DP = 600
        const val EXPANDED_DP = 840

        fun of(widthDp: Int): WidthClass = when {
            widthDp >= EXPANDED_DP -> Expanded
            widthDp >= MEDIUM_DP -> Medium
            else -> Compact
        }
    }
}

/** The current window's width class, provided once by the app shell. */
val LocalWidthClass = staticCompositionLocalOf { WidthClass.Compact }

/** Widest a single column of settings or text gets, so rows don't stretch across a tablet. */
val ReadableWidth = 720.dp

/**
 * Fills the space but lays the content out in a centred column no wider than [ReadableWidth].
 * On phones the window is narrower than that, so nothing changes.
 */
fun Modifier.readableWidth(): Modifier = fillMaxSize().wrapContentWidth(Alignment.CenterHorizontally).widthIn(max = ReadableWidth)
