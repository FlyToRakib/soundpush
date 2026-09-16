package net.soundpush.ui.theme

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Shapes
import androidx.compose.material3.Typography
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight

/** Extra semantic colors not covered by Material roles. */
data class SpColors(val success: Color, val warning: Color, val micLive: Color, val textSecondary: Color)

val LocalSpColors = staticCompositionLocalOf {
    SpColors(Tokens.Light.success, Tokens.Light.warning, Tokens.Light.micLive, Tokens.Light.textSecondary)
}

private val LightScheme = lightColorScheme(
    primary = Tokens.Light.accent,
    onPrimary = Tokens.Light.onAccent,
    background = Tokens.Light.background,
    onBackground = Tokens.Light.textPrimary,
    surface = Tokens.Light.surface,
    onSurface = Tokens.Light.textPrimary,
    surfaceVariant = Tokens.Light.surfaceMuted,
    onSurfaceVariant = Tokens.Light.textSecondary,
    surfaceContainerLowest = Tokens.Light.surface,
    surfaceContainerLow = Tokens.Light.surface,
    surfaceContainer = Tokens.Light.surface,
    surfaceContainerHigh = Tokens.Light.surface,
    surfaceContainerHighest = Tokens.Light.surfaceMuted,
    // Selected nav item / chips: a soft accent tint instead of Material's default lavender.
    secondary = Tokens.Light.accent,
    onSecondary = Tokens.Light.onAccent,
    secondaryContainer = Tokens.Light.accent.copy(alpha = 0.12f),
    onSecondaryContainer = Tokens.Light.accent,
    primaryContainer = Tokens.Light.accent.copy(alpha = 0.12f),
    onPrimaryContainer = Tokens.Light.accent,
    outline = Tokens.Light.border,
    outlineVariant = Tokens.Light.border,
    error = Tokens.Light.danger,
)

private val DarkScheme = darkColorScheme(
    primary = Tokens.Dark.accent,
    onPrimary = Tokens.Dark.onAccent,
    background = Tokens.Dark.background,
    onBackground = Tokens.Dark.textPrimary,
    surface = Tokens.Dark.surface,
    onSurface = Tokens.Dark.textPrimary,
    surfaceVariant = Tokens.Dark.surfaceMuted,
    onSurfaceVariant = Tokens.Dark.textSecondary,
    surfaceContainerLowest = Tokens.Dark.background,
    surfaceContainerLow = Tokens.Dark.surface,
    surfaceContainer = Tokens.Dark.surface,
    surfaceContainerHigh = Tokens.Dark.surfaceMuted,
    surfaceContainerHighest = Tokens.Dark.surfaceMuted,
    secondary = Tokens.Dark.accent,
    onSecondary = Tokens.Dark.onAccent,
    secondaryContainer = Tokens.Dark.accent.copy(alpha = 0.18f),
    onSecondaryContainer = Tokens.Dark.accent,
    primaryContainer = Tokens.Dark.accent.copy(alpha = 0.18f),
    onPrimaryContainer = Tokens.Dark.accent,
    outline = Tokens.Dark.border,
    outlineVariant = Tokens.Dark.border,
    error = Tokens.Dark.danger,
)

private val SpTypography = Typography(
    titleLarge = TextStyle(fontSize = Tokens.Type.titleSize, lineHeight = Tokens.Type.titleLine, fontWeight = FontWeight.SemiBold),
    titleMedium = TextStyle(fontSize = Tokens.Type.bodySize, lineHeight = Tokens.Type.bodyLine, fontWeight = FontWeight.SemiBold),
    bodyLarge = TextStyle(fontSize = Tokens.Type.bodySize, lineHeight = Tokens.Type.bodyLine),
    bodyMedium = TextStyle(fontSize = Tokens.Type.bodySize, lineHeight = Tokens.Type.bodyLine),
    bodySmall = TextStyle(fontSize = Tokens.Type.captionSize, lineHeight = Tokens.Type.captionLine),
    labelLarge = TextStyle(fontSize = Tokens.Type.bodySize, lineHeight = Tokens.Type.bodyLine, fontWeight = FontWeight.Medium),
)

/** Light / Dark / System (default). System follows the OS live. */
@Composable
fun SoundPushTheme(themeMode: String = "system", content: @Composable () -> Unit) {
    val dark = when (themeMode) {
        "light" -> false
        "dark" -> true
        else -> isSystemInDarkTheme()
    }
    val extra = if (dark) {
        SpColors(Tokens.Dark.success, Tokens.Dark.warning, Tokens.Dark.micLive, Tokens.Dark.textSecondary)
    } else {
        SpColors(Tokens.Light.success, Tokens.Light.warning, Tokens.Light.micLive, Tokens.Light.textSecondary)
    }
    androidx.compose.runtime.CompositionLocalProvider(LocalSpColors provides extra) {
        MaterialTheme(
            colorScheme = if (dark) DarkScheme else LightScheme,
            typography = SpTypography,
            shapes = Shapes(
                small = RoundedCornerShape(Tokens.Radius.control),
                medium = RoundedCornerShape(Tokens.Radius.card),
                large = RoundedCornerShape(16),
            ),
            content = content,
        )
    }
}
