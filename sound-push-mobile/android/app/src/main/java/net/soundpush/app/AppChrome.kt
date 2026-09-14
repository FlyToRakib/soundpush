package net.soundpush.app

import androidx.annotation.StringRes
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.NavigationBarItemDefaults
import androidx.compose.material3.NavigationRail
import androidx.compose.material3.NavigationRailItem
import androidx.compose.material3.NavigationRailItemDefaults
import androidx.compose.material3.VerticalDivider
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.Density
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import net.soundpush.ui.R
import net.soundpush.ui.icons.SpIcons
import net.soundpush.ui.theme.Tokens

private data class Tab(val route: String, val icon: ImageVector, @param:StringRes val label: Int)

private val TABS = listOf(
    Tab("home", SpIcons.Home, R.string.nav_home),
    Tab("devices", SpIcons.Devices, R.string.nav_devices),
    Tab("settings", SpIcons.Settings, R.string.nav_settings),
)

/** Settings sub-screens: they show a back arrow and keep the Settings tab selected. */
internal val SETTINGS_SUBSCREENS = setOf("audio", "troubleshoot", "troubleshoot/{topic}", "battery", "licenses", "audit")

/** Bottom navigation. Sub-screens (e.g. Audio) keep their parent tab selected. */
@Composable
internal fun BottomBar(route: String, onNavigate: (String) -> Unit) {
    val selectedTab = when (route) {
        in SETTINGS_SUBSCREENS -> "settings"
        else -> route
    }
    Column {
        HorizontalDivider(color = MaterialTheme.colorScheme.outline)
        NavigationBar(containerColor = MaterialTheme.colorScheme.surface, tonalElevation = 0.dp) {
            TABS.forEach { tab ->
                NavigationBarItem(
                    selected = selectedTab == tab.route,
                    onClick = { if (selectedTab != tab.route || route != tab.route) onNavigate(tab.route) },
                    icon = { Icon(tab.icon, contentDescription = null) },
                    // The bar has a fixed height: cap label scaling so 200% text doesn't clip (TalkBack still reads it).
                    label = { CappedFontScale { Text(stringResource(tab.label), maxLines = 1) } },
                    colors = NavigationBarItemDefaults.colors(
                        selectedIconColor = MaterialTheme.colorScheme.primary,
                        selectedTextColor = MaterialTheme.colorScheme.primary,
                        indicatorColor = MaterialTheme.colorScheme.secondaryContainer,
                        unselectedIconColor = MaterialTheme.colorScheme.onSurfaceVariant,
                        unselectedTextColor = MaterialTheme.colorScheme.onSurfaceVariant,
                    ),
                )
            }
        }
    }
}

/** Width of [SideRail] (the Material rail plus its divider); the app shell pads its content by this much. */
internal val RailWidth = 81.dp

/** Navigation rail for medium and expanded windows (tablets, foldables, landscape) in place of the bottom bar. */
@Composable
internal fun SideRail(route: String, onNavigate: (String) -> Unit) {
    val selectedTab = if (route in SETTINGS_SUBSCREENS) "settings" else route
    Row(Modifier.fillMaxHeight().width(RailWidth)) {
        NavigationRail(containerColor = MaterialTheme.colorScheme.surface, modifier = Modifier.weight(1f)) {
            TABS.forEach { tab ->
                NavigationRailItem(
                    selected = selectedTab == tab.route,
                    onClick = { if (selectedTab != tab.route || route != tab.route) onNavigate(tab.route) },
                    icon = { Icon(tab.icon, contentDescription = null) },
                    label = { CappedFontScale { Text(stringResource(tab.label), maxLines = 1) } },
                    colors = NavigationRailItemDefaults.colors(
                        selectedIconColor = MaterialTheme.colorScheme.primary,
                        selectedTextColor = MaterialTheme.colorScheme.primary,
                        indicatorColor = MaterialTheme.colorScheme.secondaryContainer,
                        unselectedIconColor = MaterialTheme.colorScheme.onSurfaceVariant,
                        unselectedTextColor = MaterialTheme.colorScheme.onSurfaceVariant,
                    ),
                )
            }
        }
        VerticalDivider(color = MaterialTheme.colorScheme.outline)
    }
}

@Composable
private fun CappedFontScale(max: Float = 1.3f, content: @Composable () -> Unit) {
    val density = LocalDensity.current
    CompositionLocalProvider(LocalDensity provides Density(density.density, minOf(density.fontScale, max)), content = content)
}

@StringRes
internal fun titleFor(route: String): Int = when (route) {
    "devices" -> R.string.devices_title
    "settings" -> R.string.settings_title
    "audio" -> R.string.audio_title
    "troubleshoot", "troubleshoot/{topic}" -> R.string.trouble_title
    "battery" -> R.string.settings_battery
    "licenses" -> R.string.settings_licenses
    "audit" -> R.string.audit_title
    else -> R.string.app_name
}

/** Shown for the moment between launch and the engine's first state. */
@Composable
internal fun StartupLoading() {
    Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.background) {
        Box(contentAlignment = Alignment.Center) { CircularProgressIndicator() }
    }
}

/** Never leave the user on a blank screen: explain what failed and offer a retry. */
@Composable
internal fun StartupError(message: String, onRetry: () -> Unit) {
    Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.background) {
        Column(
            Modifier.padding(Tokens.Space.lg),
            verticalArrangement = Arrangement.Center,
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Text(stringResource(R.string.startup_error_title), style = MaterialTheme.typography.titleLarge, textAlign = TextAlign.Center)
            Text(
                message,
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                textAlign = TextAlign.Center,
                modifier = Modifier.padding(top = Tokens.Space.sm, bottom = Tokens.Space.lg),
            )
            Button(onClick = onRetry) { Text(stringResource(R.string.common_retry)) }
        }
    }
}
