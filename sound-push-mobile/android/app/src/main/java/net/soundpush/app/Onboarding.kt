package net.soundpush.app

import android.content.Intent
import android.os.Build
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.systemBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import net.soundpush.engine.EngineState
import net.soundpush.engine.SoundPush
import net.soundpush.settings.BatteryGuides
import net.soundpush.settings.DESKTOP_DOWNLOAD_URL
import net.soundpush.settings.PhoneMaker
import net.soundpush.ui.R
import net.soundpush.ui.components.IconTile
import net.soundpush.ui.components.SectionTitle
import net.soundpush.ui.icons.SpIcons
import net.soundpush.ui.theme.Tokens

private const val STEP_WELCOME = "welcome"
private const val STEP_NOTIFICATIONS = "notifications"
private const val STEP_BATTERY = "battery"
private const val STEP_PAIR = "pair"

/**
 * First run: what SoundPush does, notifications (Android 13+, explained before the system asks),
 * the phone maker's battery setting where one is needed, then pairing. Every step can be skipped;
 * the microphone, camera and screen capture are asked for only when first used.
 * Pairing a computer during the flow finishes it.
 */
@Composable
internal fun OnboardingScreen(
    state: EngineState,
    notificationsEnabled: Boolean,
    onRequestNotifications: () -> Unit,
    onOpenBatteryGuide: () -> Unit,
    onScan: () -> Unit,
    onEnterAddress: () -> Unit,
    onFinish: () -> Unit,
) {
    val context = LocalContext.current
    // Fixed when the flow starts, so granting a permission doesn't shift the step numbers.
    val steps = rememberSaveable {
        arrayListOf<String>().apply {
            add(STEP_WELCOME)
            if (Build.VERSION.SDK_INT >= 33 && !notificationsEnabled) add(STEP_NOTIFICATIONS)
            if (BatteryGuides.detect() != PhoneMaker.Generic) add(STEP_BATTERY)
            add(STEP_PAIR)
        }
    }
    var index by rememberSaveable { mutableIntStateOf(0) }
    val pairedAtStart = rememberSaveable { state.trustedPeers.size }
    LaunchedEffect(state.trustedPeers.size) {
        if (state.trustedPeers.size > pairedAtStart) onFinish()
    }
    val step = steps[index.coerceIn(0, steps.lastIndex)]
    val next: () -> Unit = { if (index < steps.lastIndex) index += 1 else onFinish() }

    Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.background) {
        Column(
            Modifier
                .fillMaxSize()
                .systemBarsPadding()
                .verticalScroll(rememberScrollState())
                .padding(horizontal = Tokens.Space.lg, vertical = Tokens.Space.md),
        ) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(
                    stringResource(R.string.onboarding_step, index + 1, steps.size),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.weight(1f),
                )
                TextButton(onClick = onFinish) { Text(stringResource(R.string.onboarding_skip)) }
            }
            Spacer(Modifier.heightIn(min = Tokens.Space.xl))

            val icon = when (step) {
                STEP_NOTIFICATIONS -> SpIcons.Alert
                STEP_BATTERY -> SpIcons.Settings
                STEP_PAIR -> SpIcons.Qr
                else -> SpIcons.Speaker
            }
            IconTile(icon)
            Spacer(Modifier.heightIn(min = Tokens.Space.md))
            val title = when (step) {
                STEP_NOTIFICATIONS -> stringResource(R.string.onboarding_notif_title)
                STEP_BATTERY -> stringResource(R.string.onboarding_battery_title)
                STEP_PAIR -> stringResource(R.string.onboarding_pair_title)
                else -> stringResource(R.string.onboarding_welcome_title)
            }
            Text(title, style = MaterialTheme.typography.headlineSmall, modifier = Modifier.semantics { heading() })
            Spacer(Modifier.heightIn(min = Tokens.Space.sm))
            val body = when (step) {
                STEP_NOTIFICATIONS -> stringResource(R.string.onboarding_notif_body)
                STEP_BATTERY -> stringResource(R.string.onboarding_battery_body, BatteryGuides.makerName())
                STEP_PAIR -> stringResource(R.string.onboarding_pair_body)
                else -> stringResource(R.string.onboarding_welcome_body)
            }
            Text(body, style = MaterialTheme.typography.bodyLarge, color = MaterialTheme.colorScheme.onSurfaceVariant)

            if (step == STEP_WELCOME) {
                Spacer(Modifier.heightIn(min = Tokens.Space.md))
                Text(
                    stringResource(R.string.onboarding_permissions_note),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            if (step == STEP_PAIR) NearbyComputers(state)

            Spacer(Modifier.heightIn(min = Tokens.Space.xl))
            Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(Tokens.Space.sm)) {
                when (step) {
                    STEP_NOTIFICATIONS -> {
                        Button(onClick = {
                            onRequestNotifications()
                            next()
                        }, Modifier.fillMaxWidth()) {
                            Text(stringResource(R.string.onboarding_notif_allow))
                        }
                        TextButton(onClick = next, Modifier.fillMaxWidth()) { Text(stringResource(R.string.common_not_now)) }
                    }
                    STEP_BATTERY -> {
                        Button(onClick = {
                            onOpenBatteryGuide()
                            next()
                        }, Modifier.fillMaxWidth()) {
                            Text(stringResource(R.string.onboarding_battery_open))
                        }
                        TextButton(onClick = next, Modifier.fillMaxWidth()) { Text(stringResource(R.string.common_not_now)) }
                    }
                    STEP_PAIR -> {
                        Button(onClick = onScan, Modifier.fillMaxWidth()) { Text(stringResource(R.string.home_pair)) }
                        OutlinedButton(onClick = onEnterAddress, Modifier.fillMaxWidth()) { Text(stringResource(R.string.home_pair_address)) }
                        TextButton(
                            onClick = {
                                val share = Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, DESKTOP_DOWNLOAD_URL)
                                runCatching { context.startActivity(Intent.createChooser(share, null)) }
                            },
                            modifier = Modifier.fillMaxWidth(),
                        ) { Text(stringResource(R.string.settings_get_desktop)) }
                    }
                    else -> Button(onClick = next, Modifier.fillMaxWidth()) { Text(stringResource(R.string.onboarding_next)) }
                }
            }
        }
    }
}

/** Computers already visible on this network can be paired with one tap, without the camera. */
@Composable
private fun NearbyComputers(state: EngineState) {
    val computers = state.nearbyUntrusted.filter { it.platform != "android" && it.platform != "ios" }
    if (computers.isEmpty()) return
    SectionTitle(stringResource(R.string.onboarding_pair_nearby))
    computers.forEach { peer ->
        Row(Modifier.fillMaxWidth().heightIn(min = 56.dp), verticalAlignment = Alignment.CenterVertically) {
            IconTile(SpIcons.forPlatform(peer.platform), active = false)
            Spacer(Modifier.width(Tokens.Space.md))
            Text(peer.name, style = MaterialTheme.typography.bodyLarge, maxLines = 2, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f))
            OutlinedButton(onClick = { SoundPush.command { pairWithDevice(peer.deviceId) } }) {
                Text(stringResource(R.string.devices_pair))
            }
        }
    }
}
