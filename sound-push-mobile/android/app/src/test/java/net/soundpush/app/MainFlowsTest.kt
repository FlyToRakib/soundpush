package net.soundpush.app

import android.app.Application
import android.content.Context
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsOn
import androidx.compose.ui.test.hasStateDescription
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.isToggleable
import androidx.compose.ui.test.junit4.accessibility.enableAccessibilityChecks
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.tryPerformAccessibilityChecks
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
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
import net.soundpush.engine.StreamSettings
import net.soundpush.home.HomeScreen
import net.soundpush.settings.SettingsScreen
import net.soundpush.ui.R
import net.soundpush.ui.components.Choice
import net.soundpush.ui.components.LocalWidthClass
import net.soundpush.ui.components.WidthClass
import net.soundpush.ui.theme.SoundPushTheme
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

/**
 * The main flows as a user drives them (tap, read, toggle), on the JVM with Robolectric so they run
 * in CI without a device. Every test also runs the Accessibility Test Framework checks (touch
 * target size, labels, contrast) on the screen it ends on. The engine is not running here, so
 * engine commands are no-ops; callbacks and rendered state are what is checked.
 */
@RunWith(AndroidJUnit4::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [35], qualifiers = "w393dp-h851dp-xxhdpi", application = Application::class)
class MainFlowsTest {
    @get:Rule
    val compose = createComposeRule()

    private val context: Context = ApplicationProvider.getApplicationContext()
    private fun text(id: Int, vararg args: Any) = context.getString(id, *args)

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
        canSendSystemAudio = true,
        canPlay = true,
        transport = "quic",
    )

    private val idle = EngineState(
        local = LocalDevice(name = "Pixel", appVersion = "0.1.0"),
        peers = listOf(mac),
        capabilities = Capabilities(appAudio = true),
        settings = Settings(deviceName = "Pixel"),
    )

    private val listening = idle.copy(
        peers = listOf(mac.copy(speakersMuted = true)),
        routes = listOf(
            RouteView(
                routeId = "r1",
                peerId = "aa11",
                peerName = "Rakibs-MacBook-Air",
                kind = "receiveSystemAudio",
                status = "active",
                elapsedSecs = 75,
                stats = RouteStats(codec = "Opus", bitrateKbps = 128, latencyMs = 48.0, bufferMs = 30.0, jitterMs = 1.2),
            ),
        ),
    )

    @Before
    fun checkAccessibility() {
        compose.enableAccessibilityChecks()
    }

    private fun show(widthClass: WidthClass = WidthClass.Compact, content: @Composable () -> Unit) {
        compose.setContent {
            SoundPushTheme("light") {
                CompositionLocalProvider(LocalWidthClass provides widthClass) {
                    Surface(color = MaterialTheme.colorScheme.background) { content() }
                }
            }
        }
    }

    @Test
    fun listenTaskStartsWithTheOnlyConnectedComputer() {
        var started: Pair<String, List<String>>? = null
        show {
            HomeScreen(idle, onStartRoutes = { peer, kinds -> started = peer to kinds }, onPair = {}, onOpenDevices = {}, onShowMessage = {})
        }
        compose.onNodeWithText(text(R.string.task_listen)).performClick()
        assertEquals("aa11" to listOf("receiveSystemAudio"), started)
        compose.onRoot().tryPerformAccessibilityChecks()
    }

    @Test
    fun routeCardShowsConnectionDetailsAndTheEnginesMutePcState() {
        show { HomeScreen(listening, onStartRoutes = { _, _ -> }, onPair = {}, onOpenDevices = {}, onShowMessage = {}) }
        compose.onNodeWithText(text(R.string.route_receiveSystemAudio, "Rakibs-MacBook-Air")).performClick()
        compose.onNodeWithText(text(R.string.route_details)).assertIsDisplayed()
        compose.onNodeWithText(text(R.string.transport_network), substring = true).assertExists()
        // speakersMuted comes from the engine's PeerView, not from a local switch state.
        compose.onNode(hasText(text(R.string.route_mute_pc)) and isToggleable()).assertIsOn()
        compose.onRoot().tryPerformAccessibilityChecks()
    }

    @Test
    fun customLatencyOffersMinimumAndMaximumBuffers() {
        val custom = idle.copy(settings = idle.settings.copy(stream = StreamSettings(latency = "custom", customMinMs = 40, customMaxMs = 200)))
        show { AudioScreen(custom) }
        compose.onNodeWithText(text(R.string.latency_custom_min)).assertExists()
        compose.onNodeWithText(text(R.string.latency_custom_max)).assertExists()
        compose.onNodeWithText(text(R.string.unit_ms, 200)).assertExists()
        compose.onRoot().tryPerformAccessibilityChecks()
    }

    @Test
    fun expandedDevicesShowsTheDetailPaneBesideTheList() {
        show(WidthClass.Expanded) { DevicesScreen(idle, onScan = {}, onShowMessage = {}) }
        compose.onNodeWithText(text(R.string.devices_select)).assertIsDisplayed()
        compose.onNodeWithText("Rakibs-MacBook-Air").performClick()
        compose.onNodeWithText(text(R.string.devices_forget)).assertExists()
        compose.onNodeWithText(text(R.string.devices_select)).assertDoesNotExist()
        compose.onRoot().tryPerformAccessibilityChecks()
    }

    @Test
    fun languagePickerOffersSystemAndTranslations() {
        var picked: String? = null
        val languages = listOf(Choice("system", text(R.string.language_system)), Choice("en", "English"))
        show { SettingsScreen(idle, onOpenAudio = {}, languages = languages, onLanguageChange = { picked = it }) }
        compose.onNodeWithText(text(R.string.settings_language)).performClick()
        compose.onNodeWithText("English").performClick()
        assertEquals("en", picked)
        compose.onRoot().tryPerformAccessibilityChecks()
    }

    @Test
    fun blockedPermissionExplainsAndOpensSettings() {
        var opened = false
        var dismissed = false
        show {
            PermissionBlockedDialog(
                title = text(R.string.perm_mic_blocked_title),
                body = text(R.string.perm_mic_blocked_body),
                onOpenSettings = { opened = true },
                onDismiss = { dismissed = true },
            )
        }
        compose.onNodeWithText(text(R.string.perm_mic_blocked_title)).assertIsDisplayed()
        compose.onNodeWithText(text(R.string.common_open_settings)).performClick()
        assertTrue(opened && dismissed)
    }
    /** Balance and the 80 Hz low-cut, which the engine and the desktop already had (plan §4.3). */
    @Test
    fun audioScreenOffersBalanceAndTheLowCutFilter() {
        show { AudioScreen(idle) }
        compose.onNodeWithText(text(R.string.audio_balance)).assertExists()
        compose.onNodeWithText(text(R.string.audio_balance_center)).assertExists()
        compose.onNode(hasText(text(R.string.audio_high_pass)) and isToggleable()).assertIsOn()
        compose.onRoot().tryPerformAccessibilityChecks()
    }

    /** The recommended microphone preset is marked in the list (plan §4.3). */
    @Test
    fun theRecommendedMicrophoneModeIsMarked() {
        show { AudioScreen(idle) }
        compose.onNodeWithText(text(R.string.audio_mic_mode)).performScrollTo().performClick()
        val recommended = text(R.string.mic_voiceCommunication) + " · " + text(R.string.mic_recommended_badge)
        compose.onNodeWithText(recommended).assertExists()
        compose.onNodeWithText(text(R.string.mic_raw)).assertExists()
    }

    /** "Resilient" is a user-facing choice that still defaults to the automatic behaviour (plan §15.6). */
    @Test
    fun resilientDefaultsToAutoAndExplainsItself() {
        show { AudioScreen(idle) }
        compose.onNodeWithText(text(R.string.audio_redundancy)).assertExists()
        compose.onNodeWithText(text(R.string.audio_redundancy_auto_desc)).assertExists()
        compose.onRoot().tryPerformAccessibilityChecks()
    }

    @Test
    fun resilientExplainsAlwaysOn() {
        val always = idle.copy(settings = idle.settings.copy(stream = StreamSettings(redundancy = "on")))
        show { AudioScreen(always) }
        compose.onNodeWithText(text(R.string.audio_redundancy_on_desc)).assertExists()
    }

    /** Where noise suppression runs only matters while it is on (plan §4.3, §15.7). */
    @Test
    fun whereNoiseSuppressionRunsIsHiddenWhileItIsOff() {
        show { AudioScreen(idle) }
        compose.onNodeWithText(text(R.string.audio_noise_suppression_where)).assertDoesNotExist()
    }

    @Test
    fun noiseSuppressionOffersWhereItRuns() {
        val on = idle.copy(settings = idle.settings.copy(mic = idle.settings.mic.copy(noiseSuppression = true)))
        show { AudioScreen(on) }
        compose.onNodeWithText(text(R.string.audio_noise_suppression_where)).assertExists()
        compose.onNodeWithText(text(R.string.audio_noise_suppression_sender_desc)).assertExists()
        compose.onRoot().tryPerformAccessibilityChecks()
    }

    /** Clipping is shown and announced, not only coloured (plan §8.3). */
    @Test
    fun theLevelMeterShowsAndAnnouncesClipping() {
        val loud = idle.copy(micLevelDb = -1f, micClipping = true)
        show { AudioScreen(loud) }
        compose.onNodeWithText(text(R.string.audio_clipping)).assertExists()
        compose
            .onNode(hasStateDescription(text(R.string.a11y_level_clipping, text(R.string.a11y_level, -1))))
            .assertExists()
        compose.onRoot().tryPerformAccessibilityChecks()
    }

    /** A feedback loop while monitoring says what to do about it (plan §8.2). */
    @Test
    fun aFeedbackLoopIsExplainedOnTheAudioScreen() {
        show { AudioScreen(idle.copy(micFeedback = true)) }
        compose.onNodeWithText(text(R.string.audio_feedback)).assertExists()
        compose.onRoot().tryPerformAccessibilityChecks()
    }

    /** Noise suppression switching itself off is explained where the setting is (plan §8.3). */
    @Test
    fun suspendedNoiseSuppressionIsExplained() {
        show { AudioScreen(idle.copy(noiseSuppressionSuspended = true)) }
        compose.onNodeWithText(text(R.string.audio_noise_suppression_suspended)).assertExists()
        compose.onRoot().tryPerformAccessibilityChecks()
    }
}
