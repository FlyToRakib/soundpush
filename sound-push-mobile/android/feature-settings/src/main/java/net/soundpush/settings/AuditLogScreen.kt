package net.soundpush.settings

import androidx.annotation.StringRes
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.material3.AlertDialog
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
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.semantics
import java.text.DateFormat
import java.util.Date
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import net.soundpush.engine.AuditEntry
import net.soundpush.engine.SoundPush
import net.soundpush.ui.R
import net.soundpush.ui.components.Labels
import net.soundpush.ui.theme.Tokens

/** The local security log (pairing, permission changes, streams, refused connections), newest first. */
@Composable
fun AuditLogScreen() {
    var entries by remember { mutableStateOf<List<AuditEntry>?>(null) }
    var confirmClear by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    val reload: suspend () -> Unit = {
        entries = withContext(Dispatchers.IO) { SoundPush.securityLog() ?: emptyList() }
    }
    LaunchedEffect(Unit) { reload() }

    LazyColumn(Modifier.fillMaxSize().padding(horizontal = Tokens.Space.md)) {
        item {
            Text(
                stringResource(R.string.audit_desc),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(vertical = Tokens.Space.sm),
            )
        }
        val list = entries
        when {
            list == null -> item { Text(stringResource(R.string.audit_loading)) }
            list.isEmpty() -> item { Text(stringResource(R.string.audit_empty)) }
            else -> itemsIndexed(list) { index, entry ->
                if (index > 0) HorizontalDivider(color = MaterialTheme.colorScheme.outline)
                Column(Modifier.fillMaxWidth().padding(vertical = Tokens.Space.xs).semantics(mergeDescendants = true) {}) {
                    Text(auditText(entry), style = MaterialTheme.typography.bodyMedium)
                    Text(
                        DateFormat.getDateTimeInstance(DateFormat.MEDIUM, DateFormat.SHORT).format(Date(entry.timeUnix * 1000)),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
        }
        item {
            TextButton(enabled = !entries.isNullOrEmpty(), onClick = { confirmClear = true }) {
                Text(stringResource(R.string.audit_clear))
            }
        }
    }

    if (confirmClear) {
        AlertDialog(
            onDismissRequest = { confirmClear = false },
            text = { Text(stringResource(R.string.audit_clear_confirm)) },
            confirmButton = {
                TextButton(onClick = {
                    confirmClear = false
                    scope.launch {
                        withContext(Dispatchers.IO) { SoundPush.clearSecurityLog() }
                        reload()
                    }
                }) { Text(stringResource(R.string.audit_clear)) }
            },
            dismissButton = { TextButton(onClick = { confirmClear = false }) { Text(stringResource(R.string.common_cancel)) } },
        )
    }
}

/** One line of the log. Each kind's `detail` is documented in sp-engine audit.rs. */
@Composable
private fun auditText(e: AuditEntry): String {
    val device = e.peerName.ifEmpty { e.peerCode }.ifEmpty { stringResource(R.string.audit_unknown_device) }
    val route = e.route?.let { stringResource(Labels.routeTitle(it), device) } ?: device
    return when (e.kind) {
        "pairingAttempt" -> stringResource(R.string.audit_pairing_attempt, device, e.detail)
        "pairingSucceeded" -> stringResource(R.string.audit_pairing_succeeded, device)
        "pairingRejected" -> stringResource(
            when (e.detail) {
                "proof" -> R.string.audit_pairing_rejected_proof
                "local" -> R.string.audit_pairing_rejected_local
                "peer" -> R.string.audit_pairing_rejected_peer
                else -> R.string.audit_pairing_rejected_closed
            },
            device,
        )
        "pairingRateLimited" -> stringResource(R.string.audit_pairing_rate_limited, e.detail)
        "deviceForgotten" -> stringResource(R.string.audit_device_forgotten, device)
        "deviceBlocked" -> stringResource(R.string.audit_device_blocked, device)
        "deviceUnblocked" -> stringResource(R.string.audit_device_unblocked, device)
        "permissionChanged" -> {
            val parts = e.detail.split("=")
            stringResource(
                R.string.audit_permission_changed,
                device,
                stringResource(permissionLabel(parts.getOrElse(0) { "" })),
                stringResource(policyLabel(parts.getOrElse(1) { "" })),
            )
        }
        "routeApproved" -> stringResource(R.string.audit_route_approved, route)
        "routeDenied" -> stringResource(R.string.audit_route_denied, route)
        "routeStarted" -> stringResource(R.string.audit_route_started, route)
        "routeStopped" -> stringResource(R.string.audit_route_stopped, route)
        "connectionRefused" -> stringResource(R.string.audit_connection_refused, device)
        "logCleared" -> stringResource(R.string.audit_log_cleared)
        else -> e.kind
    }
}

@StringRes
private fun permissionLabel(name: String): Int = when (name) {
    "receiveMyAudio" -> R.string.perm_receiveMyAudio
    "useMyMicrophone" -> R.string.perm_useMyMicrophone
    "sendAudioToMe" -> R.string.perm_sendAudioToMe
    else -> R.string.perm_controlMe
}

@StringRes
private fun policyLabel(name: String): Int = when (name) {
    "allow" -> R.string.policy_allow
    "ask" -> R.string.policy_ask
    else -> R.string.policy_deny
}
