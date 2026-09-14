package net.soundpush.settings

import android.content.ComponentName
import android.content.Intent
import android.provider.Settings as AndroidSettings
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.input.ImeAction
import net.soundpush.engine.DeviceStatus
import net.soundpush.engine.EngineState
import net.soundpush.engine.SoundPush
import net.soundpush.ui.R
import net.soundpush.ui.components.Choice
import net.soundpush.ui.components.NavRow
import net.soundpush.ui.components.SectionTitle
import net.soundpush.ui.components.SettingChoice
import net.soundpush.ui.components.SettingSwitch
import net.soundpush.ui.components.SpCard
import net.soundpush.ui.theme.Tokens

/** One runtime permission in Settings → Permissions. [onClick] asks again or leads to app settings. */
data class PermissionRow(val label: String, val status: String, val needsAction: Boolean, val onClick: () -> Unit)

@Composable
fun SettingsScreen(
    state: EngineState,
    onOpenAudio: () -> Unit,
    onOpenTroubleshooter: () -> Unit = {},
    onOpenBatteryGuide: () -> Unit = {},
    onExportDiagnostics: () -> Unit = {},
    /** Microphone, camera and notifications, with their denied or blocked state (plan §26.1). */
    permissions: List<PermissionRow> = emptyList(),
    /** "System language" first, then the languages this build has; empty hides the setting. */
    languages: List<Choice> = emptyList(),
    language: String = "system",
    onLanguageChange: (String) -> Unit = {},
) {
    val s = state.settings
    val context = LocalContext.current
    val focus = LocalFocusManager.current
    val network by DeviceStatus.network.collectAsState()
    var name by remember(s.deviceName) { mutableStateOf(s.deviceName) }
    val saveName = {
        val value = name.trim()
        if (value.isNotEmpty() && value != s.deviceName) SoundPush.updateSettings { it.copy(deviceName = value) }
        focus.clearFocus()
    }

    Column(
        Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(start = Tokens.Space.md, end = Tokens.Space.md, bottom = Tokens.Space.lg),
        verticalArrangement = Arrangement.spacedBy(Tokens.Space.xs),
    ) {
        SectionTitle(stringResource(R.string.settings_general))
        SpCard {
            OutlinedTextField(
                value = name,
                onValueChange = { name = it.take(64) },
                label = { Text(stringResource(R.string.settings_device_name)) },
                singleLine = true,
                keyboardOptions = KeyboardOptions(imeAction = ImeAction.Done),
                keyboardActions = KeyboardActions(onDone = { saveName() }),
                trailingIcon = {
                    if (name.isNotBlank() && name.trim() != s.deviceName) {
                        TextButton(onClick = { saveName() }) { Text(stringResource(R.string.common_save)) }
                    }
                },
                modifier = Modifier.fillMaxWidth().padding(vertical = Tokens.Space.sm),
            )
            SettingChoice(
                stringResource(R.string.settings_theme),
                s.theme,
                listOf(
                    Choice("system", stringResource(R.string.theme_system)),
                    Choice("light", stringResource(R.string.theme_light)),
                    Choice("dark", stringResource(R.string.theme_dark)),
                ),
            ) { v -> SoundPush.updateSettings { it.copy(theme = v) } }
            if (languages.isNotEmpty()) {
                SettingChoice(stringResource(R.string.settings_language), language, languages, onLanguageChange)
            }
            SettingSwitch(
                stringResource(R.string.settings_audio_cues),
                s.audioCues,
                stringResource(R.string.settings_audio_cues_desc),
            ) { v -> SoundPush.updateSettings { it.copy(audioCues = v) } }
            Divider()
            NavRow(stringResource(R.string.settings_audio), onClick = onOpenAudio)
        }

        SectionTitle(stringResource(R.string.settings_background))
        SpCard {
            SettingSwitch(
                stringResource(R.string.settings_stay_available),
                s.mobile.stayAvailable,
                stringResource(R.string.settings_stay_available_desc),
            ) { v ->
                SoundPush.updateSettings { it.copy(mobile = it.mobile.copy(stayAvailable = v)) }
                // Long background use is exactly when the battery exemption is justified.
                if (v && !BatteryGuides.isUnrestricted(context)) BatteryGuides.requestUnrestricted(context)
            }
            SettingSwitch(stringResource(R.string.settings_remind), s.mobile.remindAfterRestart) { v ->
                SoundPush.updateSettings { it.copy(mobile = it.mobile.copy(remindAfterRestart = v)) }
            }
            Divider()
            NavRow(stringResource(R.string.settings_battery), stringResource(R.string.settings_battery_desc), onOpenBatteryGuide)
        }

        SectionTitle(stringResource(R.string.settings_usb))
        SpCard {
            Caption(stringResource(if (network.usbTethering) R.string.settings_usb_on else R.string.settings_usb_off))
            Caption(stringResource(if (network.sharesMobileData) R.string.hint_tether_data_body else R.string.settings_usb_desc))
            Divider()
            NavRow(stringResource(R.string.settings_usb_open)) {
                BatteryGuides.open(
                    context,
                    listOf(
                        Intent().setComponent(ComponentName("com.android.settings", "com.android.settings.TetherSettings")),
                        Intent(AndroidSettings.ACTION_WIRELESS_SETTINGS),
                    ),
                )
            }
        }

        if (permissions.isNotEmpty()) {
            SectionTitle(stringResource(R.string.settings_permissions))
            SpCard {
                permissions.forEachIndexed { index, permission ->
                    if (index > 0) Divider()
                    if (permission.needsAction) {
                        NavRow(permission.label, permission.status, permission.onClick)
                    } else {
                        InfoRow(permission.label, permission.status)
                    }
                }
            }
        }

        SectionTitle(stringResource(R.string.settings_privacy))
        SpCard {
            SettingChoice(
                stringResource(R.string.settings_visibility),
                s.visibility,
                listOf(
                    Choice("everyone", stringResource(R.string.visibility_everyone)),
                    Choice("trustedOnly", stringResource(R.string.visibility_trustedOnly)),
                    Choice("hidden", stringResource(R.string.visibility_hidden)),
                ),
            ) { v -> SoundPush.updateSettings { it.copy(visibility = v) } }
            Caption(stringResource(R.string.settings_privacy_note))
        }

        SectionTitle(stringResource(R.string.settings_help))
        SpCard {
            NavRow(stringResource(R.string.settings_troubleshoot), stringResource(R.string.settings_troubleshoot_desc), onOpenTroubleshooter)
            Divider()
            NavRow(stringResource(R.string.settings_diagnostics), stringResource(R.string.settings_diagnostics_desc), onExportDiagnostics)
            Divider()
            NavRow(stringResource(R.string.settings_get_desktop)) {
                val share = Intent(Intent.ACTION_SEND)
                    .setType("text/plain")
                    .putExtra(Intent.EXTRA_TEXT, ProjectLinks.RELEASES)
                runCatching { context.startActivity(Intent.createChooser(share, null).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)) }
            }
        }

        SectionTitle(stringResource(R.string.settings_about))
        AboutSection(state)
    }
}

/** Where to get the desktop app (shared with onboarding). */
const val DESKTOP_DOWNLOAD_URL = ProjectLinks.RELEASES

@Composable
private fun Divider() = HorizontalDivider(color = MaterialTheme.colorScheme.outline)

/** A label with a status and nothing to do, read as one item. */
@Composable
private fun InfoRow(label: String, status: String) {
    Column(
        Modifier
            .fillMaxWidth()
            .heightIn(min = 56.dp())
            .semantics(mergeDescendants = true) {}
            .padding(vertical = Tokens.Space.xs),
        verticalArrangement = Arrangement.Center,
    ) {
        Text(label, style = MaterialTheme.typography.bodyLarge)
        Text(status, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}

@Composable
private fun Caption(text: String) {
    Text(
        text,
        style = MaterialTheme.typography.bodySmall,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = Modifier.padding(vertical = 4.dp()),
    )
}

private fun Int.dp() = androidx.compose.ui.unit.Dp(toFloat())
