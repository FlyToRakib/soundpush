package net.soundpush.settings

import android.content.Context
import android.content.Intent
import android.net.Uri
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import kotlinx.coroutines.launch
import net.soundpush.engine.EngineState
import net.soundpush.engine.SoundPush
import net.soundpush.ui.R
import net.soundpush.ui.components.NavRow
import net.soundpush.ui.components.SettingSwitch
import net.soundpush.ui.components.SpCard
import net.soundpush.ui.theme.Tokens

/** Settings → About: this device, version and licence, update check, and help links. */
@Composable
internal fun AboutSection(state: EngineState) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val version = state.local.appVersion
    val autoCheck = state.settings.checkForUpdates
    var status by remember { mutableStateOf(UpdateChecker.cached ?: UpdateStatus.Idle) }

    LaunchedEffect(autoCheck) {
        if (autoCheck && status == UpdateStatus.Idle) status = UpdateChecker.check(version, manual = false)
    }

    SpCard {
        Column(Modifier.padding(vertical = Tokens.Space.sm)) {
            Text(state.local.name, style = MaterialTheme.typography.bodyLarge)
            Caption(state.local.displayCode)
            if (state.local.addresses.isNotEmpty()) Caption(state.local.addresses.take(2).joinToString(" · "))
            Caption(stringResource(R.string.settings_version, version))
        }
        HorizontalDivider(color = MaterialTheme.colorScheme.outline)
        SettingSwitch(
            stringResource(R.string.settings_updates_auto),
            autoCheck,
            stringResource(R.string.settings_updates_auto_desc),
        ) { v -> SoundPush.updateSettings { it.copy(checkForUpdates = v) } }
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            val text = when (val s = status) {
                UpdateStatus.Checking -> stringResource(R.string.settings_update_checking)
                UpdateStatus.UpToDate -> stringResource(R.string.settings_update_up_to_date)
                is UpdateStatus.Available -> stringResource(R.string.settings_update_available, s.version)
                UpdateStatus.Failed -> stringResource(R.string.settings_update_failed)
                UpdateStatus.Idle -> ""
            }
            Text(
                text,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.weight(1f).semantics { liveRegion = LiveRegionMode.Polite },
            )
            when (val s = status) {
                is UpdateStatus.Available -> TextButton(onClick = { openUrl(context, s.url) }) {
                    Text(stringResource(R.string.settings_update_download))
                }
                else -> TextButton(
                    enabled = status != UpdateStatus.Checking,
                    onClick = {
                        scope.launch {
                            status = UpdateStatus.Checking
                            status = UpdateChecker.check(version, manual = true)
                        }
                    },
                ) { Text(stringResource(R.string.settings_update_check)) }
            }
        }
        HorizontalDivider(color = MaterialTheme.colorScheme.outline)
        NavRow(stringResource(R.string.settings_user_guide)) { openUrl(context, ProjectLinks.USER_GUIDE) }
        NavRow(stringResource(R.string.settings_report_bug)) { openUrl(context, ProjectLinks.REPORT_BUG) }
        NavRow(stringResource(R.string.settings_privacy_policy)) { openUrl(context, ProjectLinks.PRIVACY) }
        NavRow(stringResource(R.string.settings_license), stringResource(R.string.settings_license_desc)) {
            openUrl(context, ProjectLinks.LICENSE)
        }
        NavRow(stringResource(R.string.settings_source)) { openUrl(context, ProjectLinks.SOURCE) }
    }
}

@Composable
private fun Caption(text: String) {
    Text(
        text,
        style = MaterialTheme.typography.bodySmall,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = Modifier.padding(vertical = Tokens.Space.xs),
    )
}

private fun openUrl(context: Context, url: String) {
    runCatching {
        context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(url)).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
    }
}
