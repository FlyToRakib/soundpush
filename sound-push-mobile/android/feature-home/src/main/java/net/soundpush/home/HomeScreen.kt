package net.soundpush.home

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
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
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import kotlin.math.roundToInt
import net.soundpush.engine.EngineState
import net.soundpush.engine.PeerView
import net.soundpush.engine.RouteView
import net.soundpush.engine.SoundPush
import net.soundpush.ui.R
import net.soundpush.ui.components.EmptyState
import net.soundpush.ui.components.IconTile
import net.soundpush.ui.components.Labels
import net.soundpush.ui.components.QualityBadge
import net.soundpush.ui.components.SectionTitle
import net.soundpush.ui.components.SettingSlider
import net.soundpush.ui.components.SettingSwitch
import net.soundpush.ui.components.StatusBanner
import net.soundpush.ui.components.TaskCard
import net.soundpush.ui.components.formatElapsed
import net.soundpush.ui.icons.SpIcons
import net.soundpush.ui.theme.Tokens

private data class Task(
    val titleRes: Int,
    val descRes: Int,
    val icon: ImageVector,
    val kinds: List<String>,
    /** Shown when a device is connected but can't do this. */
    val reasonRes: Int,
    val supports: (PeerView) -> Boolean,
)

private val TASKS = listOf(
    Task(R.string.task_listen, R.string.task_listen_desc, SpIcons.Speaker, listOf("receiveSystemAudio"), R.string.home_reason_listen) {
        it.canSendSystemAudio
    },
    Task(R.string.task_mic, R.string.task_mic_desc, SpIcons.Mic, listOf("sendMicToVirtualMic"), R.string.home_reason_mic) {
        it.hasVirtualMic
    },
    Task(
        R.string.task_headset,
        R.string.task_headset_desc,
        SpIcons.Headset,
        listOf("receiveSystemAudio", "sendMicToVirtualMic"),
        R.string.home_reason_mic,
    ) { it.canSendSystemAudio && it.hasVirtualMic },
    Task(R.string.task_send_apps, R.string.task_send_apps_desc, SpIcons.Apps, listOf("sendAppAudio"), R.string.home_reason_offline) {
        it.canPlay
    },
)

/**
 * @param onStartRoutes starts route kinds with a peer; the activity checks runtime
 *        permissions (microphone, screen-capture consent) before calling the engine.
 */
@Composable
fun HomeScreen(
    state: EngineState,
    onStartRoutes: (peerId: String, kinds: List<String>) -> Unit,
    onPair: () -> Unit,
    onOpenDevices: () -> Unit,
    onShowMessage: (String) -> Unit,
) {
    var picking by remember { mutableStateOf<Pair<List<String>, List<PeerView>>?>(null) }

    if (state.trustedPeers.isEmpty()) {
        EmptyState(
            icon = SpIcons.Laptop,
            title = stringResource(R.string.home_empty_title),
            body = stringResource(R.string.home_empty_body),
            action = stringResource(R.string.home_pair),
            onAction = onPair,
            secondaryAction = stringResource(R.string.home_pair_address),
            onSecondaryAction = onOpenDevices,
        )
        return
    }

    val connected = state.connectedPeers
    val offlinePeer = if (connected.isEmpty()) state.trustedPeers.firstOrNull { !it.blocked } else null

    LazyColumn(
        modifier = Modifier.fillMaxSize(),
        contentPadding = PaddingValues(start = Tokens.Space.md, end = Tokens.Space.md, top = Tokens.Space.xs, bottom = Tokens.Space.lg),
        verticalArrangement = Arrangement.spacedBy(Tokens.Space.sm),
    ) {
        if (offlinePeer != null) {
            item(key = "banner") {
                val reconnecting = offlinePeer.connection == "connecting" || offlinePeer.connection == "reconnecting"
                StatusBanner(
                    title = stringResource(
                        if (reconnecting) R.string.home_banner_reconnecting else R.string.home_banner_offline,
                        offlinePeer.name,
                    ),
                    message = stringResource(R.string.home_banner_hint),
                    actionLabel = stringResource(R.string.common_retry),
                    onAction = { SoundPush.command { connect(offlinePeer.deviceId) } },
                )
            }
        }

        if (state.routes.isNotEmpty()) {
            item(key = "active-title") { SectionTitle(stringResource(R.string.home_active)) }
            items(state.routes, key = { it.routeId }) { route ->
                RouteCard(route, state.peers.firstOrNull { it.deviceId == route.peerId })
            }
        }

        item(key = "tasks-title") { SectionTitle(stringResource(R.string.home_title)) }
        items(TASKS, key = { it.titleRes }) { task ->
            val appsUnsupported = task.kinds.contains("sendAppAudio") && !state.capabilities.appAudio
            val capable = connected.filter(task.supports)
            val reasonRes = when {
                appsUnsupported -> R.string.task_unavailable_apps
                connected.isEmpty() -> R.string.home_reason_offline
                capable.isEmpty() -> task.reasonRes
                else -> null
            }
            val reason = reasonRes?.let { stringResource(it) }
            TaskCard(
                title = stringResource(task.titleRes),
                description = reason ?: stringResource(task.descRes),
                icon = task.icon,
                enabled = reason == null,
            ) {
                when {
                    reason != null -> onShowMessage(reason)
                    capable.size == 1 -> onStartRoutes(capable[0].deviceId, task.kinds)
                    else -> picking = task.kinds to capable
                }
            }
        }

        item(key = "peers-title") { SectionTitle(stringResource(R.string.devices_paired)) }
        items(state.trustedPeers, key = { "peer-${it.deviceId}" }) { peer -> PeerRow(peer, onOpenDevices) }
    }

    picking?.let { (kinds, peers) ->
        AlertDialog(
            onDismissRequest = { picking = null },
            title = { Text(stringResource(R.string.home_pick_device)) },
            text = {
                Column {
                    peers.forEach { peer ->
                        Row(
                            Modifier
                                .fillMaxWidth()
                                .heightIn(min = 56.dp)
                                .clickable {
                                    picking = null
                                    onStartRoutes(peer.deviceId, kinds)
                                },
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Icon(SpIcons.forPlatform(peer.platform), null, tint = MaterialTheme.colorScheme.onSurfaceVariant)
                            Spacer(Modifier.width(Tokens.Space.md))
                            Text(peer.name, style = MaterialTheme.typography.bodyLarge)
                        }
                    }
                }
            },
            confirmButton = { TextButton(onClick = { picking = null }) { Text(stringResource(R.string.common_cancel)) } },
        )
    }
}

@Composable
private fun PeerRow(peer: PeerView, onOpen: () -> Unit) {
    Surface(
        onClick = onOpen,
        shape = MaterialTheme.shapes.medium,
        color = MaterialTheme.colorScheme.background,
        modifier = Modifier.fillMaxWidth(),
    ) {
        Row(Modifier.heightIn(min = 60.dp).padding(horizontal = 4.dp), verticalAlignment = Alignment.CenterVertically) {
            IconTile(SpIcons.forPlatform(peer.platform), active = peer.connection == "connected")
            Spacer(Modifier.width(Tokens.Space.md))
            Column(Modifier.weight(1f)) {
                Text(peer.name, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(
                    stringResource(Labels.status(peer.connection)),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            QualityBadge(peer.quality, Labels.quality(peer.quality)?.let { stringResource(it) } ?: "")
            Spacer(Modifier.width(4.dp))
            Icon(SpIcons.Chevron, null, tint = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.size(18.dp))
        }
    }
}

@Composable
private fun RouteCard(route: RouteView, peer: PeerView?) {
    var expanded by rememberSaveable(route.routeId) { mutableStateOf(false) }
    var pcMuted by rememberSaveable(route.routeId) { mutableStateOf(false) }
    val quality = peer?.quality ?: "unknown"
    val qualityLabel = Labels.quality(quality)?.let { stringResource(it) } ?: ""
    val icon = when {
        route.muted -> SpIcons.Mute
        route.isMic -> SpIcons.Mic
        route.kind.contains("App") -> SpIcons.Apps
        else -> SpIcons.Speaker
    }

    Surface(
        onClick = { expanded = !expanded },
        shape = MaterialTheme.shapes.medium,
        border = BorderStroke(1.dp, MaterialTheme.colorScheme.outline),
        color = MaterialTheme.colorScheme.surface,
        modifier = Modifier.fillMaxWidth(),
    ) {
        Column(Modifier.padding(Tokens.Space.md)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                IconTile(icon, route.status == "active")
                Spacer(Modifier.width(12.dp))
                Column(Modifier.weight(1f)) {
                    Text(
                        stringResource(Labels.routeTitle(route.kind), route.peerName),
                        style = MaterialTheme.typography.titleMedium,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                    val secondary = MaterialTheme.colorScheme.onSurfaceVariant
                    when (route.status) {
                        "active" -> Row(verticalAlignment = Alignment.CenterVertically) {
                            Text(formatElapsed(route.elapsedSecs), style = MaterialTheme.typography.bodySmall, color = secondary)
                            Spacer(Modifier.width(Tokens.Space.sm))
                            QualityBadge(quality, qualityLabel, route.stats.latencyMs)
                        }
                        "paused" -> Text(stringResource(R.string.route_reconnecting), style = MaterialTheme.typography.bodySmall, color = secondary)
                        "starting" -> Text(stringResource(R.string.route_starting), style = MaterialTheme.typography.bodySmall, color = secondary)
                        else -> Text(stringResource(R.string.route_waiting, route.peerName), style = MaterialTheme.typography.bodySmall, color = secondary)
                    }
                }
                IconButton(onClick = { SoundPush.command { setRouteMuted(route.routeId, !route.muted) } }) {
                    Icon(
                        // The button shows the action it performs; the tile on the left shows the current state.
                        if (route.muted) SpIcons.Speaker else SpIcons.Mute,
                        stringResource(if (route.muted) R.string.route_unmute else R.string.route_mute),
                        tint = if (route.muted) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                FilledTonalButton(onClick = { SoundPush.command { stopRoute(route.routeId) } }) {
                    Text(stringResource(R.string.route_stop))
                }
            }

            if (expanded) {
                HorizontalDivider(Modifier.padding(vertical = 12.dp), color = MaterialTheme.colorScheme.outline)
                if (!route.isSending && !route.isMic) {
                    SettingSlider(
                        label = stringResource(R.string.route_volume),
                        value = route.volume,
                        range = 0f..2f,
                        format = { "${(it * 100).roundToInt()}%" },
                    ) { v -> SoundPush.command { setRouteVolume(route.routeId, v) } }
                }
                if (route.kind == "receiveSystemAudio") {
                    SettingSwitch(stringResource(R.string.route_mute_pc), pcMuted) { v ->
                        pcMuted = v
                        SoundPush.command { setPeerSpeakersMuted(route.peerId, v) }
                    }
                }
                Text(
                    "${route.stats.codec} · ${route.stats.bitrateKbps} kb/s · ${route.stats.latencyMs.roundToInt()} ms · " +
                        "buffer ${route.stats.bufferMs.roundToInt()} ms · loss ${"%.1f".format(route.stats.lossPct)} %",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(top = Tokens.Space.xs),
                )
            }
        }
    }
}
