package net.soundpush.audio

import android.media.AudioManager
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import net.soundpush.engine.AudioEffects
import net.soundpush.engine.DeviceStatus
import net.soundpush.engine.EngineState
import net.soundpush.engine.OutputPreference
import net.soundpush.engine.SoundPush
import net.soundpush.ui.R
import net.soundpush.ui.components.Choice
import net.soundpush.ui.components.SectionTitle
import net.soundpush.ui.components.SettingChoice
import net.soundpush.ui.components.SettingSlider
import net.soundpush.ui.components.SettingSwitch
import net.soundpush.ui.components.SpCard
import net.soundpush.ui.components.readableWidth
import net.soundpush.ui.components.rememberFormat
import net.soundpush.ui.theme.Tokens
import kotlin.math.roundToInt

@Composable
fun AudioScreen(state: EngineState) {
    val s = state.settings
    val unavailable = stringResource(R.string.effect_unavailable)
    val output by DeviceStatus.output.collectAsState()
    val percent = rememberFormat(R.string.unit_percent)
    val ms = rememberFormat(R.string.unit_ms)
    val gain = rememberFormat(R.string.unit_db_gain)

    Column(
        Modifier
            .readableWidth()
            .verticalScroll(rememberScrollState())
            .padding(start = Tokens.Space.md, end = Tokens.Space.md, bottom = Tokens.Space.lg),
        verticalArrangement = Arrangement.spacedBy(Tokens.Space.xs),
    ) {
        SectionTitle(stringResource(R.string.audio_latency))
        SpCard {
            SettingChoice(
                stringResource(R.string.audio_latency),
                s.stream.latency,
                listOf(
                    Choice("lowLatency", stringResource(R.string.latency_lowLatency)),
                    Choice("balanced", stringResource(R.string.latency_balanced)),
                    Choice("stable", stringResource(R.string.latency_stable)),
                    Choice("custom", stringResource(R.string.latency_custom)),
                ),
            ) { v -> SoundPush.updateSettings { it.copy(stream = it.stream.copy(latency = v)) } }
            if (s.stream.latency == "custom") {
                // Same bounds as the desktop: minimum 5–500 ms, maximum from the minimum up to 1000 ms, in 5 ms steps.
                val minMs = s.stream.customMinMs.coerceIn(5, 500)
                SettingSlider(
                    label = stringResource(R.string.latency_custom_min),
                    value = minMs.toFloat(),
                    range = 5f..500f,
                    steps = 98,
                    format = { ms(snap5(it)) },
                ) { v ->
                    val min = snap5(v)
                    SoundPush.updateSettings { it.copy(stream = it.stream.copy(customMinMs = min, customMaxMs = maxOf(min, it.stream.customMaxMs))) }
                }
                SettingSlider(
                    label = stringResource(R.string.latency_custom_max),
                    value = s.stream.customMaxMs.coerceIn(minMs, 1000).toFloat(),
                    range = minMs.toFloat()..1000f,
                    steps = ((1000 - minMs) / 5 - 1).coerceAtLeast(0),
                    format = { ms(snap5(it)) },
                ) { v -> SoundPush.updateSettings { it.copy(stream = it.stream.copy(customMaxMs = maxOf(snap5(v), it.stream.customMinMs))) } }
            }
            if (output == DeviceStatus.Output.Bluetooth) {
                Text(
                    stringResource(R.string.audio_bluetooth_note),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(bottom = Tokens.Space.sm),
                )
            }
            Divider()
            SettingChoice(
                stringResource(R.string.audio_quality),
                s.stream.quality,
                listOf(
                    Choice("auto", stringResource(R.string.quality_auto)),
                    Choice("opus", stringResource(R.string.quality_opus)),
                    Choice("lossless", stringResource(R.string.quality_lossless)),
                ),
            ) { v -> SoundPush.updateSettings { it.copy(stream = it.stream.copy(quality = v)) } }
            if (s.stream.quality == "opus") {
                Divider()
                SettingChoice(
                    stringResource(R.string.audio_bitrate),
                    s.stream.opusBitrate.toString(),
                    listOf(10, 24, 32, 64, 96, 128, 192, 256, 320, 450, 510).map { Choice((it * 1000).toString(), stringResource(R.string.unit_kbps, it)) },
                ) { v -> SoundPush.updateSettings { it.copy(stream = it.stream.copy(opusBitrate = v.toInt())) } }
            }
        }

        SectionTitle(stringResource(R.string.audio_playback))
        SpCard {
            OutputDeviceChoice(output)
            Divider()
            SettingSlider(
                label = stringResource(R.string.audio_volume),
                value = s.output.volume,
                range = 0f..2f,
                format = { percent((it * 100).roundToInt()) },
            ) { v -> SoundPush.updateSettings { it.copy(output = it.output.copy(volume = v)) } }
            SettingSwitch(stringResource(R.string.audio_mono), s.output.mono) { v ->
                SoundPush.updateSettings { it.copy(output = it.output.copy(mono = v)) }
            }
            Divider()
            SettingSlider(
                label = stringResource(R.string.audio_av_offset),
                value = s.output.avOffsetMs.toFloat(),
                range = 0f..500f,
                format = { ms((it / 10).roundToInt() * 10) },
            ) { v -> SoundPush.updateSettings { it.copy(output = it.output.copy(avOffsetMs = (v / 10).roundToInt() * 10)) } }
            Divider()
            SettingChoice(
                stringResource(R.string.audio_focus),
                s.output.audioFocus,
                listOf(
                    Choice("pause", stringResource(R.string.focus_pause)),
                    Choice("duck", stringResource(R.string.focus_duck)),
                    Choice("mix", stringResource(R.string.focus_mix)),
                    Choice("mixDuringCalls", stringResource(R.string.focus_mixDuringCalls)),
                ),
            ) { v -> SoundPush.updateSettings { it.copy(output = it.output.copy(audioFocus = v)) } }
            SettingSwitch(stringResource(R.string.audio_pause_on_disconnect), s.output.pauseOnHeadsetDisconnect) { v ->
                SoundPush.updateSettings { it.copy(output = it.output.copy(pauseOnHeadsetDisconnect = v)) }
            }
        }

        // Both switches leave the low-latency path for Android's regular player; they apply live.
        SectionTitle(stringResource(R.string.audio_output_advanced))
        SpCard {
            SettingSwitch(
                stringResource(R.string.audio_compat_output),
                s.output.compatibilityOutput,
                stringResource(R.string.audio_compat_output_desc),
            ) { v -> SoundPush.updateSettings { it.copy(output = it.output.copy(compatibilityOutput = v)) } }
            SettingSwitch(
                stringResource(R.string.audio_output_effects),
                s.output.outputEffects,
                stringResource(R.string.audio_output_effects_desc),
            ) { v -> SoundPush.updateSettings { it.copy(output = it.output.copy(outputEffects = v)) } }
        }

        SectionTitle(stringResource(R.string.audio_mic))
        SpCard {
            SettingChoice(
                stringResource(R.string.audio_mic_mode),
                s.mic.mode,
                listOf("voiceCommunication", "default", "raw", "voicePerformance", "voiceRecognition", "camcorder", "mic")
                    .map { mode -> Choice(mode, micModeLabel(mode)) },
            ) { v -> SoundPush.updateSettings { it.copy(mic = it.mic.copy(mode = v)) } }
            Divider()
            SettingSlider(
                label = stringResource(R.string.audio_gain),
                value = s.mic.gainDb,
                range = 0f..20f,
                format = { gain(it.roundToInt()) },
            ) { v -> SoundPush.updateSettings { it.copy(mic = it.mic.copy(gainDb = v.roundToInt().toFloat())) } }
            SettingSwitch(
                stringResource(R.string.audio_noise_suppression),
                s.mic.noiseSuppression,
                stringResource(R.string.audio_noise_suppression_desc),
            ) { v -> SoundPush.updateSettings { it.copy(mic = it.mic.copy(noiseSuppression = v)) } }
            SettingSwitch(
                stringResource(R.string.audio_echo),
                s.mic.systemEchoCancellation && AudioEffects.echoCancellation,
                if (AudioEffects.echoCancellation) null else unavailable,
                enabled = AudioEffects.echoCancellation,
            ) { v -> SoundPush.updateSettings { it.copy(mic = it.mic.copy(systemEchoCancellation = v)) } }
            SettingSwitch(
                stringResource(R.string.audio_system_ns),
                s.mic.systemNoiseSuppression && AudioEffects.noiseSuppression,
                if (AudioEffects.noiseSuppression) null else unavailable,
                enabled = AudioEffects.noiseSuppression,
            ) { v -> SoundPush.updateSettings { it.copy(mic = it.mic.copy(systemNoiseSuppression = v)) } }
            SettingSwitch(
                stringResource(R.string.audio_agc),
                s.mic.systemAgc && AudioEffects.automaticGain,
                if (AudioEffects.automaticGain) null else unavailable,
                enabled = AudioEffects.automaticGain,
            ) { v -> SoundPush.updateSettings { it.copy(mic = it.mic.copy(systemAgc = v)) } }
            SettingSwitch(
                stringResource(R.string.audio_monitor),
                s.mic.monitor,
                stringResource(R.string.audio_monitor_desc),
            ) { v -> SoundPush.updateSettings { it.copy(mic = it.mic.copy(monitor = v)) } }
        }
    }
}

@Composable
private fun Divider() = HorizontalDivider(color = MaterialTheme.colorScheme.outline)

private fun snap5(value: Float) = (value / 5).roundToInt() * 5

/**
 * Where this phone plays (plan §23.1 "output", where Android allows): Automatic, or one of the
 * outputs connected now. The list is re-read when the output changes (a headset plugged in).
 */
@Composable
private fun OutputDeviceChoice(output: DeviceStatus.Output) {
    val context = LocalContext.current
    val target by OutputPreference.target.collectAsState()
    val connected = remember(output) {
        context.getSystemService(AudioManager::class.java)?.let(OutputPreference::available).orEmpty()
    }
    val choices = (listOf(OutputPreference.Target.Automatic) + connected + target).distinct()
    SettingChoice(
        stringResource(R.string.audio_output_device),
        target.key,
        choices.map { Choice(it.key, outputTargetLabel(it)) },
    ) { key -> OutputPreference.set(context, OutputPreference.fromKey(key)) }
    Text(
        stringResource(
            when {
                target == OutputPreference.Target.Automatic -> R.string.audio_output_device_auto_desc
                target !in connected -> R.string.audio_output_device_missing
                else -> R.string.audio_output_device_desc
            },
        ),
        style = MaterialTheme.typography.bodySmall,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = Modifier.padding(bottom = Tokens.Space.sm),
    )
}

@Composable
private fun outputTargetLabel(target: OutputPreference.Target): String = stringResource(
    when (target) {
        OutputPreference.Target.Automatic -> R.string.audio_output_auto
        OutputPreference.Target.Speaker -> R.string.output_speaker
        OutputPreference.Target.Wired -> R.string.output_wired
        OutputPreference.Target.Bluetooth -> R.string.output_bluetooth
        OutputPreference.Target.Usb -> R.string.output_usb
    },
)

@Composable
private fun micModeLabel(mode: String): String = when (mode) {
    "default" -> stringResource(R.string.mic_default)
    "raw" -> stringResource(R.string.mic_raw)
    "voicePerformance" -> stringResource(R.string.mic_voicePerformance)
    "voiceRecognition" -> stringResource(R.string.mic_voiceRecognition)
    "camcorder" -> stringResource(R.string.mic_camcorder)
    "mic" -> stringResource(R.string.mic_mic)
    else -> stringResource(R.string.mic_voiceCommunication)
}
