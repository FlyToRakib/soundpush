package net.soundpush.settings

import android.Manifest
import android.app.ActivityManager
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.media.AudioManager
import android.os.Build
import android.provider.Settings as AndroidSettings
import androidx.annotation.StringRes
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.unit.dp
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat
import androidx.lifecycle.compose.LifecycleResumeEffect
import net.soundpush.engine.DeviceStatus
import net.soundpush.engine.EngineState
import net.soundpush.engine.SoundPush
import net.soundpush.ui.R
import net.soundpush.ui.components.NavRow
import net.soundpush.ui.components.SectionTitle
import net.soundpush.ui.components.SpCard
import net.soundpush.ui.icons.SpIcons
import net.soundpush.ui.theme.LocalSpColors
import net.soundpush.ui.theme.Tokens

/** The problems the guided troubleshooter covers; [key] is the navigation argument. */
enum class TroubleTopic(val key: String, @param:StringRes val title: Int) {
    FindComputer("find", R.string.trouble_find),
    NoAudio("audio", R.string.trouble_no_audio),
    Microphone("mic", R.string.trouble_mic),
    Disconnects("background", R.string.trouble_disconnects);

    companion object {
        fun from(key: String?) = entries.firstOrNull { it.key == key }
    }
}

enum class CheckStatus { Problem, Ok, Info }

/** One automatic check: what was found, and the one action that fixes it. */
data class Check(val status: CheckStatus, val text: String, val actionLabel: String? = null, val onAction: (() -> Unit)? = null)

@Composable
fun TroubleshooterScreen(onOpenTopic: (String) -> Unit, onExportDiagnostics: () -> Unit) {
    Column(
        Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(start = Tokens.Space.md, end = Tokens.Space.md, bottom = Tokens.Space.lg),
        verticalArrangement = Arrangement.spacedBy(Tokens.Space.xs),
    ) {
        SectionTitle(stringResource(R.string.trouble_pick))
        SpCard {
            TroubleTopic.entries.forEachIndexed { i, topic ->
                if (i > 0) HorizontalDivider(color = MaterialTheme.colorScheme.outline)
                NavRow(stringResource(topic.title)) { onOpenTopic(topic.key) }
            }
        }
        SpCard(Modifier.padding(top = Tokens.Space.md)) {
            NavRow(stringResource(R.string.settings_diagnostics), stringResource(R.string.settings_diagnostics_desc), onExportDiagnostics)
        }
    }
}

/**
 * Runs the checks for one topic before asking the user anything: problems first, then what is
 * fine, then tips. Re-runs whenever the user returns from a settings screen.
 */
@Composable
fun TroubleshootTopicScreen(
    topicKey: String,
    state: EngineState,
    onOpenDevices: () -> Unit,
    onOpenHome: () -> Unit,
    onOpenBatteryGuide: () -> Unit,
    onExportDiagnostics: () -> Unit,
) {
    val topic = TroubleTopic.from(topicKey) ?: TroubleTopic.FindComputer
    val context = LocalContext.current
    val network by DeviceStatus.network.collectAsState()
    val output by DeviceStatus.output.collectAsState()
    var resumes by remember { mutableIntStateOf(0) }
    LifecycleResumeEffect(Unit) {
        resumes++
        onPauseOrDispose { }
    }
    val strings = TroubleStrings(context)
    val checks = remember(topic, state, network, output, resumes) {
        when (topic) {
            TroubleTopic.FindComputer -> findChecks(strings, state, network, onOpenDevices)
            TroubleTopic.NoAudio -> audioChecks(context, strings, state, output, onOpenHome)
            TroubleTopic.Microphone -> micChecks(context, strings, state)
            TroubleTopic.Disconnects -> backgroundChecks(context, strings, state, onOpenBatteryGuide)
        }.sortedBy { it.status.ordinal }
    }

    Column(
        Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(start = Tokens.Space.md, end = Tokens.Space.md, bottom = Tokens.Space.lg),
        verticalArrangement = Arrangement.spacedBy(Tokens.Space.xs),
    ) {
        SectionTitle(stringResource(R.string.trouble_checks))
        SpCard {
            checks.forEachIndexed { i, check ->
                if (i > 0) HorizontalDivider(color = MaterialTheme.colorScheme.outline)
                CheckRow(check)
            }
        }
        Text(
            stringResource(R.string.trouble_still_stuck),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(top = Tokens.Space.md, start = 4.dp, end = 4.dp),
        )
        SpCard {
            NavRow(stringResource(R.string.settings_diagnostics), stringResource(R.string.settings_diagnostics_desc), onExportDiagnostics)
        }
    }
}

/** Status icon and text read as one item ("Needs attention. Media volume is at 0."); the fix button stays separate. */
@Composable
internal fun CheckRow(check: Check) {
    val colors = LocalSpColors.current
    val (icon, tint, stateRes) = when (check.status) {
        CheckStatus.Ok -> Triple(SpIcons.Check, colors.success, R.string.trouble_state_ok)
        CheckStatus.Problem -> Triple(SpIcons.Alert, colors.warning, R.string.trouble_state_problem)
        CheckStatus.Info -> Triple(SpIcons.Info, MaterialTheme.colorScheme.onSurfaceVariant, R.string.trouble_state_info)
    }
    val stateText = stringResource(stateRes)
    Column(Modifier.fillMaxWidth().heightIn(min = 48.dp).padding(vertical = Tokens.Space.sm)) {
        Row(
            Modifier.semantics(mergeDescendants = true) { stateDescription = stateText },
            verticalAlignment = Alignment.Top,
        ) {
            Icon(icon, null, tint = tint, modifier = Modifier.size(20.dp))
            Spacer(Modifier.width(12.dp))
            Text(check.text, style = MaterialTheme.typography.bodyLarge, modifier = Modifier.weight(1f))
        }
        if (check.actionLabel != null && check.onAction != null) {
            TextButton(onClick = check.onAction, modifier = Modifier.align(Alignment.End)) { Text(check.actionLabel) }
        }
    }
}

/** String lookups for the checks, which are built outside composition. */
private class TroubleStrings(private val context: Context) {
    fun get(@StringRes id: Int, vararg args: Any): String = context.getString(id, *args)
}

private fun findChecks(s: TroubleStrings, state: EngineState, network: DeviceStatus.NetworkInfo, onOpenDevices: () -> Unit) = buildList {
    add(
        when {
            network.usbTethering -> Check(CheckStatus.Ok, s.get(R.string.trouble_wifi_usb))
            network.wifi || network.ethernet -> Check(CheckStatus.Ok, s.get(R.string.trouble_wifi_ok))
            else -> Check(CheckStatus.Problem, s.get(R.string.trouble_wifi_off))
        },
    )
    if (network.vpn) add(Check(CheckStatus.Problem, s.get(R.string.trouble_vpn)))
    if (state.settings.visibility == "hidden") add(Check(CheckStatus.Info, s.get(R.string.trouble_hidden)))
    if (state.peers.any { it.online }) {
        add(Check(CheckStatus.Ok, s.get(R.string.trouble_nearby_found)))
    } else {
        add(Check(CheckStatus.Problem, s.get(R.string.trouble_nearby_none), s.get(R.string.trouble_add_address), onOpenDevices))
    }
    add(Check(CheckStatus.Info, s.get(R.string.trouble_firewall)))
}

private fun audioChecks(context: Context, s: TroubleStrings, state: EngineState, output: DeviceStatus.Output, onOpenHome: () -> Unit) = buildList {
    val receiving = state.routes.filter { !it.isSending && it.status != "stopped" }
    if (receiving.isEmpty()) {
        add(Check(CheckStatus.Problem, s.get(R.string.trouble_not_streaming), s.get(R.string.trouble_open_home), onOpenHome))
    } else {
        add(Check(CheckStatus.Ok, s.get(R.string.trouble_streaming_ok, receiving.first().peerName)))
    }
    receiving.filter { it.muted }.takeIf { it.isNotEmpty() }?.let { muted ->
        add(
            Check(CheckStatus.Problem, s.get(R.string.trouble_route_muted), s.get(R.string.trouble_unmute)) {
                muted.forEach { r -> SoundPush.command { setRouteMuted(r.routeId, false) } }
            },
        )
    }
    val am = context.getSystemService(AudioManager::class.java)
    if (am != null && am.getStreamVolume(AudioManager.STREAM_MUSIC) == 0) {
        add(
            Check(CheckStatus.Problem, s.get(R.string.trouble_volume_zero), s.get(R.string.trouble_volume_up)) {
                am.adjustStreamVolume(AudioManager.STREAM_MUSIC, AudioManager.ADJUST_RAISE, AudioManager.FLAG_SHOW_UI)
            },
        )
    } else {
        add(Check(CheckStatus.Ok, s.get(R.string.trouble_volume_ok)))
    }
    if (output == DeviceStatus.Output.Bluetooth) add(Check(CheckStatus.Info, s.get(R.string.trouble_bluetooth_out)))
    val out = state.settings.output
    if (!out.compatibilityOutput && !out.outputEffects) {
        add(
            Check(CheckStatus.Info, s.get(R.string.trouble_compat_suggest), s.get(R.string.trouble_turn_on)) {
                SoundPush.updateSettings { it.copy(output = it.output.copy(compatibilityOutput = true)) }
            },
        )
    }
    add(Check(CheckStatus.Info, s.get(R.string.trouble_pc_side)))
}

private fun micChecks(context: Context, s: TroubleStrings, state: EngineState) = buildList {
    val granted = ContextCompat.checkSelfPermission(context, Manifest.permission.RECORD_AUDIO) == PackageManager.PERMISSION_GRANTED
    if (granted) {
        add(Check(CheckStatus.Ok, s.get(R.string.trouble_mic_permission_ok)))
    } else {
        add(
            Check(CheckStatus.Problem, s.get(R.string.trouble_mic_permission_off), s.get(R.string.trouble_app_settings)) {
                BatteryGuides.open(context, listOf(BatteryGuides.appDetails(context)))
            },
        )
    }
    state.connectedPeers.firstOrNull { it.platform != "android" && it.platform != "ios" }?.let { computer ->
        add(
            if (computer.hasVirtualMic) {
                Check(CheckStatus.Ok, s.get(R.string.trouble_virtual_mic_ok, computer.name))
            } else {
                Check(CheckStatus.Problem, s.get(R.string.trouble_virtual_mic_missing, computer.name))
            },
        )
    }
    state.routes.filter { it.isMic && it.isSending && it.muted }.takeIf { it.isNotEmpty() }?.let { muted ->
        add(
            Check(CheckStatus.Problem, s.get(R.string.trouble_mic_muted), s.get(R.string.trouble_unmute)) {
                muted.forEach { r -> SoundPush.command { setRouteMuted(r.routeId, false) } }
            },
        )
    }
    // Android 10+ silences a recorder while another app (a call, a voice assistant) holds the mic.
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
        val silenced = context.getSystemService(AudioManager::class.java)?.activeRecordingConfigurations.orEmpty().any { it.isClientSilenced }
        if (silenced) add(Check(CheckStatus.Problem, s.get(R.string.trouble_mic_silenced)))
    }
    add(Check(CheckStatus.Info, s.get(R.string.trouble_mic_app_tip)))
}

private fun backgroundChecks(context: Context, s: TroubleStrings, state: EngineState, onOpenBatteryGuide: () -> Unit) = buildList {
    if (BatteryGuides.isUnrestricted(context)) {
        add(Check(CheckStatus.Ok, s.get(R.string.trouble_battery_ok)))
    } else {
        add(
            Check(CheckStatus.Problem, s.get(R.string.trouble_battery_restricted), s.get(R.string.trouble_battery_allow)) {
                BatteryGuides.requestUnrestricted(context)
            },
        )
    }
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P &&
        context.getSystemService(ActivityManager::class.java)?.isBackgroundRestricted == true
    ) {
        add(
            Check(CheckStatus.Problem, s.get(R.string.trouble_background_restricted), s.get(R.string.trouble_app_settings)) {
                BatteryGuides.open(context, listOf(BatteryGuides.appDetails(context)))
            },
        )
    }
    if (NotificationManagerCompat.from(context).areNotificationsEnabled()) {
        add(Check(CheckStatus.Ok, s.get(R.string.trouble_notifications_ok)))
    } else {
        add(
            Check(CheckStatus.Problem, s.get(R.string.trouble_notifications_off), s.get(R.string.common_open_settings)) {
                BatteryGuides.open(
                    context,
                    listOf(
                        Intent(AndroidSettings.ACTION_APP_NOTIFICATION_SETTINGS).putExtra(AndroidSettings.EXTRA_APP_PACKAGE, context.packageName),
                        BatteryGuides.appDetails(context),
                    ),
                )
            },
        )
    }
    if (BatteryGuides.detect() != PhoneMaker.Generic) {
        add(
            Check(
                CheckStatus.Info,
                s.get(R.string.trouble_oem_guide, BatteryGuides.makerName()),
                s.get(R.string.onboarding_battery_open),
                onOpenBatteryGuide,
            ),
        )
    }
    if (!state.settings.mobile.stayAvailable) add(Check(CheckStatus.Info, s.get(R.string.trouble_stay_available)))
}
