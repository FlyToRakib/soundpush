package net.soundpush.app

import android.app.Application
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onRoot
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.github.takahirom.roborazzi.captureRoboImage
import net.soundpush.audio.AudioScreen
import net.soundpush.devices.DevicesScreen
import net.soundpush.engine.Capabilities
import net.soundpush.engine.EngineState
import net.soundpush.engine.LocalDevice
import net.soundpush.engine.PeerView
import net.soundpush.engine.Permissions
import net.soundpush.engine.RouteStats
import net.soundpush.engine.RouteView
import net.soundpush.engine.Settings
import net.soundpush.home.HomeScreen
import net.soundpush.settings.BatteryGuideScreen
import net.soundpush.settings.SettingsScreen
import net.soundpush.settings.TroubleshootTopicScreen
import net.soundpush.settings.TroubleshooterScreen
import net.soundpush.ui.components.BannerModel
import net.soundpush.ui.components.ScreenHeader
import net.soundpush.ui.icons.SpIcons
import net.soundpush.ui.theme.SoundPushTheme
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

/**
 * Renders each screen inside the real app chrome (header + bottom bar) in Light and Dark.
 * Images: app/build/outputs/roborazzi/
 */
@RunWith(AndroidJUnit4::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [35], qualifiers = "w393dp-h851dp-xxhdpi", application = Application::class)
class ScreenshotTest {
    @get:Rule
    val compose = createComposeRule()

    private val mac = PeerView(
        deviceId = "aa11",
        name = "Rakibs-MacBook-Air",
        platform = "macos",
        trusted = true,
        online = true,
        connection = "connected",
        quality = "excellent",
        rttMs = 4.0,
        permissions = Permissions(),
        autoConnect = true,
        canSendSystemAudio = true,
        canSendMic = true,
        canPlay = true,
        hasVirtualMic = false,
    )

    private val connected = EngineState(
        local = LocalDevice(name = "Redmi Note 9 Pro", displayCode = "SP-7K3Q-2M9P-X4TR", addresses = listOf("192.168.68.101:47650"), appVersion = "0.1.0"),
        peers = listOf(mac, PeerView(deviceId = "bb22", name = "Office-PC", platform = "windows", online = true)),
        routes = listOf(
            RouteView(
                routeId = "r1",
                peerId = "aa11",
                peerName = "Rakibs-MacBook-Air",
                kind = "receiveSystemAudio",
                status = "active",
                elapsedSecs = 754,
                stats = RouteStats(codec = "Opus", bitrateKbps = 128, latencyMs = 48.0, bufferMs = 30.0),
            ),
        ),
        capabilities = Capabilities(appAudio = true),
        settings = Settings(deviceName = "Redmi Note 9 Pro"),
    )

    private val offline = connected.copy(
        peers = listOf(mac.copy(connection = "reconnecting", quality = "unknown", online = false)),
        routes = emptyList(),
    )

    private val empty = EngineState(local = connected.local, capabilities = Capabilities(appAudio = true))

    private fun capture(name: String, theme: String, route: String, content: @Composable () -> Unit) {
        compose.setContent {
            SoundPushTheme(theme) {
                Surface(color = MaterialTheme.colorScheme.background, modifier = Modifier.fillMaxSize()) {
                    Column {
                        ScreenHeader(
                            title = androidx.compose.ui.res.stringResource(titleFor(route)),
                            onBack = if (route == "audio") ({}) else null,
                        )
                        Box(Modifier.weight(1f).fillMaxWidth()) { content() }
                        BottomBar(route) {}
                    }
                }
            }
        }
        compose.onRoot().captureRoboImage("build/outputs/roborazzi/$name-$theme.png")
    }

    private fun home(state: EngineState): @Composable () -> Unit = {
        HomeScreen(state, onStartRoutes = { _, _ -> }, onPair = {}, onOpenDevices = {}, onShowMessage = {})
    }

    @Test fun homeConnectedLight() = capture("home-connected", "light", "home", home(connected))

    @Test fun homeConnectedDark() = capture("home-connected", "dark", "home", home(connected))

    @Test fun homeOfflineLight() = capture("home-offline", "light", "home", home(offline))

    @Test fun homeEmptyLight() = capture("home-empty", "light", "home", home(empty))

    @Test fun devicesLight() = capture("devices", "light", "devices") { DevicesScreen(connected, onScan = {}, onShowMessage = {}) }

    @Test fun devicesDark() = capture("devices", "dark", "devices") { DevicesScreen(connected, onScan = {}, onShowMessage = {}) }

    @Test fun settingsLight() = capture("settings", "light", "settings") { SettingsScreen(connected, onOpenAudio = {}) }

    @Test fun settingsDark() = capture("settings", "dark", "settings") { SettingsScreen(connected, onOpenAudio = {}) }

    @Test fun audioLight() = capture("audio", "light", "audio") { AudioScreen(connected) }

    @Test fun audioDark() = capture("audio", "dark", "audio") { AudioScreen(connected) }

    private val bluetoothBanner = BannerModel(
        key = "bluetoothLatency",
        title = "Bluetooth adds delay",
        message = "Bluetooth headphones and speakers add about 100–300 ms.",
        actionLabel = "Use Stable",
        onAction = {},
        icon = SpIcons.Bluetooth,
        dismissLabel = "Dismiss",
        onDismiss = {},
    )

    @Test fun homeBannersLight() = capture("home-banners", "light", "home") {
        HomeScreen(connected, onStartRoutes = { _, _ -> }, onPair = {}, onOpenDevices = {}, onShowMessage = {}, banners = listOf(bluetoothBanner))
    }

    @Test fun troubleshooterLight() = capture("troubleshoot", "light", "troubleshoot") {
        TroubleshooterScreen(onOpenTopic = {}, onExportDiagnostics = {})
    }

    @Test fun troubleshootAudioDark() = capture("troubleshoot-audio", "dark", "troubleshoot/{topic}") {
        TroubleshootTopicScreen("audio", connected, onOpenDevices = {}, onOpenHome = {}, onOpenBatteryGuide = {}, onExportDiagnostics = {})
    }

    @Test fun batteryGuideLight() = capture("battery", "light", "battery") { BatteryGuideScreen() }

    @Test fun onboardingLight() = captureBare("onboarding", "light") {
        OnboardingScreen(empty, notificationsEnabled = true, onRequestNotifications = {}, onOpenBatteryGuide = {}, onScan = {}, onEnterAddress = {}, onFinish = {})
    }

    @Test fun onboardingDark() = captureBare("onboarding", "dark") {
        OnboardingScreen(empty, notificationsEnabled = true, onRequestNotifications = {}, onOpenBatteryGuide = {}, onScan = {}, onEnterAddress = {}, onFinish = {})
    }

    /** Largest system font size: nothing may clip. */
    @Test @Config(qualifiers = "w393dp-h851dp-xxhdpi", fontScale = 2.0f)
    fun homeConnectedFontScale200() = capture("home-connected-font200", "light", "home", home(connected))

    @Test @Config(qualifiers = "w393dp-h851dp-xxhdpi", fontScale = 2.0f)
    fun settingsFontScale200() = capture("settings-font200", "light", "settings") { SettingsScreen(connected, onOpenAudio = {}) }

    /** Right-to-left layout (Arabic): mirrored chevrons and back arrow. */
    @Test @Config(qualifiers = "ar-w393dp-h851dp-xxhdpi")
    fun homeConnectedRtl() = capture("home-connected-rtl", "light", "home", home(connected))

    private fun captureBare(name: String, theme: String, content: @Composable () -> Unit) {
        compose.setContent { SoundPushTheme(theme) { content() } }
        compose.onRoot().captureRoboImage("build/outputs/roborazzi/$name-$theme.png")
    }
}
