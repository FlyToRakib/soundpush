package net.soundpush.devices

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
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
import androidx.compose.material3.VerticalDivider
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import java.util.Locale
import kotlin.math.roundToInt
import net.soundpush.engine.DeviceProfile
import net.soundpush.engine.EngineJson
import net.soundpush.engine.EngineState
import net.soundpush.engine.NetworkTestView
import net.soundpush.engine.PeerView
import net.soundpush.engine.SoundPush
import net.soundpush.ui.R
import net.soundpush.ui.components.Choice
import net.soundpush.ui.components.IconTile
import net.soundpush.ui.components.Labels
import net.soundpush.ui.components.LocalWidthClass
import net.soundpush.ui.components.WidthClass
import net.soundpush.ui.components.readableWidth
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
    val selected = state.trustedPeers.firstOrNull { it.deviceId == selectedId }
    // Expanded windows show the list and the selected device side by side (list-detail); phones use a sheet.
    val twoPane = LocalWidthClass.current == WidthClass.Expanded

    val list = @Composable { modifier: Modifier ->
        DeviceList(
            state,
            selectedId = if (twoPane) selectedId else null,
            onScan = onScan,
            onEnterAddress = { addressOpen = true },
            onSelect = { selectedId = it },
            onShowMessage = onShowMessage,
            modifier = modifier,
        )
    }

    if (twoPane) {
        Row(Modifier.fillMaxSize()) {
            list(Modifier.weight(2f).fillMaxHeight())
            VerticalDivider(color = MaterialTheme.colorScheme.outline)
            Box(Modifier.weight(3f).fillMaxHeight()) {
                if (selected != null) {
                    // Keyed by device, so the rename field and dialogs never carry over to another device.
                    androidx.compose.runtime.key(selected.deviceId) {
                        DeviceDetail(
                            selected,
                            profile = state.settings.deviceProfiles[selected.deviceId] ?: DeviceProfile(),
                            test = state.networkTests.firstOrNull { it.peerId == selected.deviceId },
                            onForgotten = { selectedId = null },
                            modifier = Modifier.padding(top = Tokens.Space.xs),
                        )
                    }
                } else {
                    Text(
                        stringResource(if (state.trustedPeers.isEmpty()) R.string.devices_none else R.string.devices_select),
                        style = MaterialTheme.typography.bodyLarge,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.align(Alignment.Center).padding(Tokens.Space.lg),
                    )
                }
            }
        }
    } else {
        list(Modifier.readableWidth())
        selected?.let { peer ->
            DeviceSheet(
                peer,
                profile = state.settings.deviceProfiles[peer.deviceId] ?: DeviceProfile(),
                test = state.networkTests.firstOrNull { it.peerId == peer.deviceId },
                onDismiss = { selectedId = null },
            )
        }
    }

    if (addressOpen) AddressDialog(onDismiss = { addressOpen = false })
}

@Composable
private fun DeviceList(
    state: EngineState,
    selectedId: String?,
    onScan: () -> Unit,
    onEnterAddress: () -> Unit,
    onSelect: (String) -> Unit,
    onShowMessage: (String) -> Unit,
    modifier: Modifier,
) {
    LazyColumn(
        modifier = modifier,
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
                OutlinedButton(onClick = onEnterAddress, modifier = Modifier.weight(1f)) {
                    Text(stringResource(R.string.devices_address))
                }
            }
        }

        item(key = "paired-title") { SectionTitle(stringResource(R.string.devices_paired)) }
        if (state.trustedPeers.isEmpty()) {
            item(key = "paired-empty") { EmptyLine(stringResource(R.string.devices_none)) }
        }
        items(state.trustedPeers, key = { "paired-${it.deviceId}" }) { peer ->
            DeviceRow(peer, selected = peer.deviceId == selectedId, onClick = { onSelect(peer.deviceId) })
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
private fun DeviceRow(peer: PeerView, actionLabel: String? = null, selected: Boolean = false, onClick: () -> Unit) {
    Surface(
        onClick = onClick,
        shape = MaterialTheme.shapes.medium,
        // The device open in the detail pane stays highlighted (and is announced as selected).
        color = if (selected) MaterialTheme.colorScheme.secondaryContainer else MaterialTheme.colorScheme.background,
        modifier = Modifier.fillMaxWidth().semantics { this.selected = selected },
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

/** Save a device's audio profile; an all-default profile is removed. */
private fun setProfile(deviceId: String, profile: DeviceProfile) {
    val json = if (profile == DeviceProfile()) null else EngineJson.encodeToString(DeviceProfile.serializer(), profile)
    SoundPush.command { setDeviceProfile(deviceId, json) }
}

@Composable
private fun ProfileCard(peer: PeerView, profile: DeviceProfile) {
    val default = Choice("", stringResource(R.string.devices_profile_default))
    SectionTitle(stringResource(R.string.devices_profile))
    SpCard {
        Text(
            stringResource(R.string.devices_profile_desc),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(vertical = Tokens.Space.sm),
        )
        SettingChoice(
            stringResource(R.string.devices_profile_latency),
            profile.latency ?: "",
            listOf(
                default,
                Choice("lowLatency", stringResource(R.string.latency_lowLatency)),
                Choice("balanced", stringResource(R.string.latency_balanced)),
                Choice("stable", stringResource(R.string.latency_stable)),
            ),
        ) { v -> setProfile(peer.deviceId, profile.copy(latency = v.ifEmpty { null })) }
        HorizontalDivider(color = MaterialTheme.colorScheme.outline)
        SettingChoice(
            stringResource(R.string.devices_profile_quality),
            profile.quality ?: "",
            listOf(
                default,
                Choice("auto", stringResource(R.string.quality_auto)),
                Choice("opus", stringResource(R.string.quality_opus)),
                Choice("lossless", stringResource(R.string.quality_lossless)),
            ),
        ) { v -> setProfile(peer.deviceId, profile.copy(quality = v.ifEmpty { null })) }
        HorizontalDivider(color = MaterialTheme.colorScheme.outline)
        SettingChoice(
            stringResource(R.string.devices_profile_redundancy),
            when (profile.redundancy) {
                true -> "on"
                false -> "off"
                null -> ""
            },
            listOf(
                default,
                Choice("on", stringResource(R.string.devices_profile_on)),
                Choice("off", stringResource(R.string.devices_profile_off)),
            ),
        ) { v ->
            setProfile(
                peer.deviceId,
                profile.copy(redundancy = when (v) { "on" -> true; "off" -> false; else -> null }),
            )
        }
    }
}

@Composable
private fun NetworkTestCard(peer: PeerView, test: NetworkTestView?) {
    SectionTitle(stringResource(R.string.nettest_title))
    SpCard {
        Text(
            stringResource(R.string.nettest_desc),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(vertical = Tokens.Space.sm),
        )
        if (test?.status == "running") {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(
                    stringResource(R.string.nettest_running, (test.progress * 100).roundToInt()),
                    modifier = Modifier.weight(1f),
                )
                TextButton(onClick = { SoundPush.command { cancelNetworkTest(peer.deviceId) } }) {
                    Text(stringResource(R.string.nettest_cancel))
                }
            }
        } else {
            OutlinedButton(
                enabled = peer.connection == "connected",
                onClick = { SoundPush.command { runNetworkTest(peer.deviceId) } },
            ) { Text(stringResource(R.string.nettest_run)) }
        }
        if (test?.status == "failed") {
            Text(stringResource(R.string.nettest_failed), color = MaterialTheme.colorScheme.error)
        }
        val report = test?.report
        if (report != null && test.status != "running") {
            Text(
                stringResource(
                    R.string.nettest_result,
                    report.rttMs.roundToInt(),
                    report.jitterMs.roundToInt(),
                    String.format(Locale.getDefault(), "%.1f", report.lossPct),
                    report.achievableKbps,
                ),
                style = MaterialTheme.typography.bodyMedium,
                modifier = Modifier.padding(top = Tokens.Space.sm),
            )
            report.recommendation.tips.forEach { tip ->
                Labels.networkTip(tip)?.let {
                    Text(stringResource(it), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
            TextButton(onClick = {
                val rec = report.recommendation
                setProfile(
                    peer.deviceId,
                    DeviceProfile(
                        latency = rec.latency,
                        quality = rec.quality,
                        opusBitrate = if (rec.quality == "opus") rec.opusBitrate else null,
                        redundancy = rec.redundancy,
                    ),
                )
            }) { Text(stringResource(R.string.nettest_apply)) }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun DeviceSheet(peer: PeerView, profile: DeviceProfile, test: NetworkTestView?, onDismiss: () -> Unit) {
    ModalBottomSheet(onDismissRequest = onDismiss) {
        DeviceDetail(peer, profile, test, onForgotten = onDismiss, modifier = Modifier.navigationBarsPadding())
    }
}

/** Everything about one paired device: the bottom sheet on phones, the detail pane on wide windows. */
@Composable
private fun DeviceDetail(
    peer: PeerView,
    profile: DeviceProfile,
    test: NetworkTestView?,
    onForgotten: () -> Unit,
    modifier: Modifier = Modifier,
) {
    var confirmForget by remember { mutableStateOf(false) }
    var alias by remember(peer.deviceId) { mutableStateOf(peer.name) }
    val policies = listOf(
        Choice("allow", stringResource(R.string.policy_allow)),
        Choice("ask", stringResource(R.string.policy_ask)),
        Choice("deny", stringResource(R.string.policy_deny)),
    )

    Column(
        modifier
            .verticalScroll(rememberScrollState())
            .padding(horizontal = Tokens.Space.md)
            .padding(bottom = Tokens.Space.md),
        verticalArrangement = Arrangement.spacedBy(Tokens.Space.sm),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            IconTile(SpIcons.forPlatform(peer.platform), active = peer.connection == "connected")
            Spacer(Modifier.width(Tokens.Space.md))
            Column(Modifier.weight(1f)) {
                Text(peer.name, style = MaterialTheme.typography.titleLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
                val status = stringResource(Labels.status(peer.connection))
                Text(
                    if (peer.transport == "tcp") stringResource(R.string.status_with_link, status, stringResource(R.string.peer_via_usb)) else status,
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

        ProfileCard(peer, profile)
        NetworkTestCard(peer, test)

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

    if (confirmForget) {
        AlertDialog(
            onDismissRequest = { confirmForget = false },
            title = { Text(stringResource(R.string.devices_forget)) },
            text = { Text(stringResource(R.string.devices_forget_confirm, peer.name)) },
            confirmButton = {
                TextButton(onClick = {
                    confirmForget = false
                    SoundPush.command { forgetDevice(peer.deviceId) }
                    onForgotten()
                }) { Text(stringResource(R.string.devices_forget), color = MaterialTheme.colorScheme.error) }
            },
            dismissButton = { TextButton(onClick = { confirmForget = false }) { Text(stringResource(R.string.common_cancel)) } },
        )
    }
}
