package net.soundpush.audio

import android.media.AudioManager
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
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
import kotlin.math.roundToInt
import net.soundpush.engine.AudioEffects
import net.soundpush.engine.DeviceStatus
import net.soundpush.engine.EngineState
import net.soundpush.engine.OutputPreference
import net.soundpush.engine.SoundPush
import net.soundpush.ui.R
import net.soundpush.ui.components.Choice
import net.soundpush.ui.components.LevelMeter
import net.soundpush.ui.components.SectionTitle
import net.soundpush.ui.components.SettingChoice
import net.soundpush.ui.components.SettingSlider
import net.soundpush.ui.components.SettingSwitch
import net.soundpush.ui.components.SpCard
import net.soundpush.ui.components.readableWidth
import net.soundpush.ui.components.rememberFormat
import net.soundpush.ui.theme.Tokens

@Composable
fun AudioScreen(state: EngineState) {
    val s = state.settings
    val unavailable = stringResource(R.string.effect_unavailable)
    val output by DeviceStatus.output.collectAsState()
    val percent = rememberFormat(R.string.unit_percent)
    val ms = rememberFormat(R.string.unit_ms)
    val gain = rememberFormat(R.string.unit_db_gain)
    val balance = rememberBalanceLabel()

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
            Divider()
            // "Resilient" (plan §15.6). Auto is the default and keeps the automatic behaviour.
            SettingChoice(
                stringResource(R.string.audio_redundancy),
                s.stream.redundancy,
                listOf(
                    Choice("auto", stringResource(R.string.audio_redundancy_auto)),
                    Choice("on", stringResource(R.string.audio_redundancy_on)),
                    Choice("off", stringResource(R.string.audio_redundancy_off)),
                ),
            ) { v -> SoundPush.updateSettings { it.copy(stream = it.stream.copy(redundancy = v)) } }
            Caption(
                stringResource(
                    when (s.stream.redundancy) {
                        "on" -> R.string.audio_redundancy_on_desc
                        "off" -> R.string.audio_redundancy_off_desc
                        else -> R.string.audio_redundancy_auto_desc
                    },
                ),
            )
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
            // Balance and mono go together: one earbud in, or a speaker on one side only.
            SettingSlider(
                label = stringResource(R.string.audio_balance),
                value = s.output.balance,
                range = -1f..1f,
                steps = 39,
                format = balance,
            ) { v -> SoundPush.updateSettings { it.copy(output = it.output.copy(balance = snapBalance(v))) } }
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
            // The recommended preset is marked in the list, as the plan describes (§4.3).
            val recommended = stringResource(R.string.mic_recommended_badge)
            SettingChoice(
                stringResource(R.string.audio_mic_mode),
                s.mic.mode,
                listOf("voiceCommunication", "default", "raw", "voicePerformance", "voiceRecognition", "camcorder", "mic")
                    .map { mode ->
                        val label = micModeLabel(mode)
                        Choice(mode, if (mode == RECOMMENDED_MIC_MODE) "$label · $recommended" else label)
                    },
            ) { v -> SoundPush.updateSettings { it.copy(mic = it.mic.copy(mode = v)) } }
            Divider()
            SettingSlider(
                label = stringResource(R.string.audio_gain),
                value = s.mic.gainDb,
                range = 0f..20f,
                format = { gain(it.roundToInt()) },
            ) { v -> SoundPush.updateSettings { it.copy(mic = it.mic.copy(gainDb = v.roundToInt().toFloat())) } }
            // The live level while the microphone is streaming or being monitored, with the clip
            // indicator the volume boost needs (plan §8.3).
            if (state.micLevelDb > -119f) {
                LevelMeter(stringResource(R.string.audio_mic_level), state.micLevelDb, state.micClipping)
            }
            SettingSwitch(
                stringResource(R.string.audio_high_pass),
                s.mic.highPass,
                stringResource(R.string.audio_high_pass_desc),
            ) { v -> SoundPush.updateSettings { it.copy(mic = it.mic.copy(highPass = v)) } }
            if (state.noiseSuppressionSuspended) {
                Caption(stringResource(R.string.audio_noise_suppression_suspended))
            }
            SettingSwitch(
                stringResource(R.string.audio_noise_suppression),
                s.mic.noiseSuppression,
                stringResource(R.string.audio_noise_suppression_desc),
            ) { v -> SoundPush.updateSettings { it.copy(mic = it.mic.copy(noiseSuppression = v)) } }
            if (s.mic.noiseSuppression) {
                // Where the work happens: this phone's battery, or the computer's CPU (plan §4.3).
                SettingChoice(
                    stringResource(R.string.audio_noise_suppression_where),
                    s.mic.noiseSuppressionAt,
                    listOf(
                        Choice("sender", stringResource(R.string.audio_noise_suppression_sender)),
                        Choice("receiver", stringResource(R.string.audio_noise_suppression_receiver)),
                    ),
                ) { v -> SoundPush.updateSettings { it.copy(mic = it.mic.copy(noiseSuppressionAt = v)) } }
                Caption(
                    stringResource(
                        if (s.mic.noiseSuppressionAt == "receiver") {
                            R.string.audio_noise_suppression_receiver_desc
                        } else {
                            R.string.audio_noise_suppression_sender_desc
                        },
                    ),
                )
            }
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
            if (state.micFeedback) {
                Caption(stringResource(R.string.audio_feedback))
            }
        }
    }
}

/** Android's own preset for voice; the list marks it so the choice is obvious (plan §4.3). */
private const val RECOMMENDED_MIC_MODE = "voiceCommunication"

@Composable
private fun Divider() = HorizontalDivider(color = MaterialTheme.colorScheme.outline)

/** Explanatory line under a control, in the same style as the rest of the screen. */
@Composable
private fun Caption(text: String) = Text(
    text,
    style = MaterialTheme.typography.bodySmall,
    color = MaterialTheme.colorScheme.onSurfaceVariant,
    modifier = Modifier.padding(bottom = Tokens.Space.sm),
)

private fun snap5(value: Float) = (value / 5).roundToInt() * 5

/** Balance in twentieths, matching the desktop's 5 % steps. */
private fun snapBalance(value: Float) = (value * 20).roundToInt() / 20f

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

/**
 * "Center", "Left 40 %", "Right 40 %" — the same wording as the desktop. Built outside
 * composition so the slider can relabel itself while it is dragged.
 */
@Composable
private fun rememberBalanceLabel(): (Float) -> String {
    val center = stringResource(R.string.audio_balance_center)
    val left = rememberFormat(R.string.audio_balance_left)
    val right = rememberFormat(R.string.audio_balance_right)
    return remember(center, left, right) {
        { value ->
            val percent = (snapBalance(value) * 100).roundToInt()
            when {
                percent == 0 -> center
                percent < 0 -> left(-percent)
                else -> right(percent)
            }
        }
    }
}

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
