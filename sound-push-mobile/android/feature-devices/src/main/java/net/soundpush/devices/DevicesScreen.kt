package net.soundpush.devices

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import net.soundpush.engine.EngineState
import net.soundpush.engine.PeerView
import net.soundpush.engine.SoundPush
import net.soundpush.ui.R
import net.soundpush.ui.components.Choice
import net.soundpush.ui.components.IconTile
import net.soundpush.ui.components.Labels
import net.soundpush.ui.components.QualityBadge
import net.soundpush.ui.components.SectionTitle
import net.soundpush.ui.components.SettingChoice
import net.soundpush.ui.components.SettingSwitch
import net.soundpush.ui.components.SpCard
import net.soundpush.ui.icons.SpIcons
import net.soundpush.ui.theme.Tokens

@Composable
fun DevicesScreen(state: EngineState, onScan: () -> Unit, onShowMessage: (String) -> Unit) {
    // Survive rotation: keep the address dialog and the open device sheet.
    var addressOpen by rememberSaveable { mutableStateOf(false) }
    var selectedId by rememberSaveable { mutableStateOf<String?>(null) }

    LazyColumn(
        modifier = Modifier.fillMaxSize(),
        contentPadding = PaddingValues(start = Tokens.Space.md, end = Tokens.Space.md, top = Tokens.Space.xs, bottom = Tokens.Space.lg),
        verticalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        item(key = "actions") {
            Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.sm)) {
                Button(onClick = onScan, modifier = Modifier.weight(1f)) {
                    Icon(SpIcons.Qr, null, modifier = Modifier.size(18.dp))
                    Spacer(Modifier.width(Tokens.Space.sm))
                    Text(stringResource(R.string.devices_scan))
                }
                OutlinedButton(onClick = { addressOpen = true }, modifier = Modifier.weight(1f)) {
                    Text(stringResource(R.string.devices_address))
                }
            }
        }

        item(key = "paired-title") { SectionTitle(stringResource(R.string.devices_paired)) }
        if (state.trustedPeers.isEmpty()) {
            item(key = "paired-empty") { EmptyLine(stringResource(R.string.devices_none)) }
        }
        items(state.trustedPeers, key = { "paired-${it.deviceId}" }) { peer ->
            DeviceRow(peer, onClick = { selectedId = peer.deviceId })
        }

        item(key = "nearby-title") { SectionTitle(stringResource(R.string.devices_nearby)) }
        if (state.nearbyUntrusted.isEmpty()) {
            item(key = "nearby-empty") { EmptyLine(stringResource(R.string.devices_nearby_none)) }
        }
        items(state.nearbyUntrusted, key = { "nearby-${it.deviceId}" }) { peer ->
            val hint = stringResource(R.string.devices_pair_hint, peer.name)
            DeviceRow(peer, actionLabel = stringResource(R.string.devices_pair)) {
                SoundPush.command { pairWithDevice(peer.deviceId) }
                onShowMessage(hint)
            }
        }
    }

    if (addressOpen) AddressDialog(onDismiss = { addressOpen = false })
    state.trustedPeers.firstOrNull { it.deviceId == selectedId }?.let { peer ->
        DeviceSheet(peer, onDismiss = { selectedId = null })
    }
}

@Composable
private fun EmptyLine(text: String) {
    Text(
        text,
        style = MaterialTheme.typography.bodyMedium,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = Modifier.padding(horizontal = 4.dp, vertical = Tokens.Space.sm),
    )
}

@Composable
private fun DeviceRow(peer: PeerView, actionLabel: String? = null, onClick: () -> Unit) {
    Surface(
        onClick = onClick,
        shape = MaterialTheme.shapes.medium,
        color = MaterialTheme.colorScheme.background,
        modifier = Modifier.fillMaxWidth(),
    ) {
        Row(Modifier.heightIn(min = 64.dp).padding(horizontal = 4.dp), verticalAlignment = Alignment.CenterVertically) {
            IconTile(SpIcons.forPlatform(peer.platform), active = peer.trusted && peer.connection == "connected")
            Spacer(Modifier.width(Tokens.Space.md))
            Column(Modifier.weight(1f)) {
                Text(peer.name, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
                if (peer.trusted) {
                    Text(
                        stringResource(if (peer.blocked) R.string.devices_block else Labels.status(peer.connection)),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
            if (actionLabel != null) {
                OutlinedButton(onClick = onClick) { Text(actionLabel) }
            } else {
                QualityBadge(peer.quality, Labels.quality(peer.quality)?.let { stringResource(it) } ?: "")
                Spacer(Modifier.width(4.dp))
                Icon(SpIcons.Chevron, null, tint = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.size(18.dp))
            }
        }
    }
}

@Composable
private fun AddressDialog(onDismiss: () -> Unit) {
    var address by remember { mutableStateOf("") }
    val submit = {
        val target = address.trim()
        if (target.isNotEmpty()) {
            SoundPush.command { pairWithAddress(target) }
            onDismiss()
        }
    }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.devices_address)) },
        text = {
            OutlinedTextField(
                value = address,
                onValueChange = { address = it.take(253) },
                label = { Text(stringResource(R.string.devices_address_hint)) },
                placeholder = { Text("192.168.1.20") },
                singleLine = true,
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri, imeAction = ImeAction.Go),
                keyboardActions = KeyboardActions(onGo = { submit() }),
                modifier = Modifier.fillMaxWidth(),
            )
        },
        confirmButton = {
            Button(enabled = address.isNotBlank(), onClick = { submit() }) { Text(stringResource(R.string.devices_connect)) }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.common_cancel)) } },
    )
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun DeviceSheet(peer: PeerView, onDismiss: () -> Unit) {
    var confirmForget by remember { mutableStateOf(false) }
    var alias by remember(peer.deviceId) { mutableStateOf(peer.name) }
    val policies = listOf(
        Choice("allow", stringResource(R.string.policy_allow)),
        Choice("ask", stringResource(R.string.policy_ask)),
        Choice("deny", stringResource(R.string.policy_deny)),
    )

    ModalBottomSheet(onDismissRequest = onDismiss) {
        Column(
            Modifier
                .verticalScroll(rememberScrollState())
                .padding(horizontal = Tokens.Space.md)
                .navigationBarsPadding()
                .padding(bottom = Tokens.Space.md),
            verticalArrangement = Arrangement.spacedBy(Tokens.Space.sm),
        ) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                IconTile(SpIcons.forPlatform(peer.platform), active = peer.connection == "connected")
                Spacer(Modifier.width(Tokens.Space.md))
                Column(Modifier.weight(1f)) {
                    Text(peer.name, style = MaterialTheme.typography.titleLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    Text(
                        stringResource(Labels.status(peer.connection)),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                if (peer.connection == "connected") {
                    OutlinedButton(onClick = { SoundPush.command { disconnect(peer.deviceId) } }) {
                        Text(stringResource(R.string.devices_disconnect))
                    }
                } else {
                    Button(onClick = { SoundPush.command { connect(peer.deviceId) } }) { Text(stringResource(R.string.devices_connect)) }
                }
            }

            SpCard {
                OutlinedTextField(
                    value = alias,
                    onValueChange = { alias = it.take(64) },
                    label = { Text(stringResource(R.string.devices_rename)) },
                    singleLine = true,
                    keyboardOptions = KeyboardOptions(imeAction = ImeAction.Done),
                    keyboardActions = KeyboardActions(onDone = {
                        val value = alias.trim()
                        SoundPush.command { renameDevice(peer.deviceId, value.ifEmpty { null }) }
                    }),
                    modifier = Modifier.fillMaxWidth().padding(vertical = Tokens.Space.sm),
                )
                SettingSwitch(stringResource(R.string.devices_auto_connect), peer.autoConnect) { v ->
                    SoundPush.command { setAutoConnect(peer.deviceId, v) }
                }
            }

            peer.permissions?.let { p ->
                SectionTitle(stringResource(R.string.devices_permissions))
                SpCard {
                    SettingChoice(stringResource(R.string.perm_receiveMyAudio), p.receive_my_audio, policies) { v ->
                        SoundPush.command { setPermission(peer.deviceId, "receiveMyAudio", v) }
                    }
                    HorizontalDivider(color = MaterialTheme.colorScheme.outline)
                    SettingChoice(stringResource(R.string.perm_useMyMicrophone), p.use_my_microphone, policies) { v ->
                        SoundPush.command { setPermission(peer.deviceId, "useMyMicrophone", v) }
                    }
                    HorizontalDivider(color = MaterialTheme.colorScheme.outline)
                    SettingChoice(stringResource(R.string.perm_sendAudioToMe), p.send_audio_to_me, policies) { v ->
                        SoundPush.command { setPermission(peer.deviceId, "sendAudioToMe", v) }
                    }
                    HorizontalDivider(color = MaterialTheme.colorScheme.outline)
                    SettingChoice(stringResource(R.string.perm_controlMe), p.control_me, policies) { v ->
                        SoundPush.command { setPermission(peer.deviceId, "controlMe", v) }
                    }
                }
            }

            SpCard {
                SettingSwitch(
                    stringResource(R.string.devices_block),
                    peer.blocked,
                    stringResource(R.string.devices_block_desc),
                ) { v -> SoundPush.command { setDeviceBlocked(peer.deviceId, v) } }
            }

            TextButton(onClick = { confirmForget = true }, modifier = Modifier.fillMaxWidth()) {
                Text(stringResource(R.string.devices_forget), color = MaterialTheme.colorScheme.error)
            }
        }
    }

    if (confirmForget) {
        AlertDialog(
            onDismissRequest = { confirmForget = false },
            title = { Text(stringResource(R.string.devices_forget)) },
            text = { Text(stringResource(R.string.devices_forget_confirm, peer.name)) },
            confirmButton = {
                TextButton(onClick = {
                    confirmForget = false
                    SoundPush.command { forgetDevice(peer.deviceId) }
                    onDismiss()
                }) { Text(stringResource(R.string.devices_forget), color = MaterialTheme.colorScheme.error) }
            },
            dismissButton = { TextButton(onClick = { confirmForget = false }) { Text(stringResource(R.string.common_cancel)) } },
        )
    }
}
