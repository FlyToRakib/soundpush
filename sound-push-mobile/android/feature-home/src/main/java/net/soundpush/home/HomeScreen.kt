package net.soundpush.home

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
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
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.onClick
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import net.soundpush.engine.DeviceStatus
import net.soundpush.engine.EngineState
import net.soundpush.engine.PeerView
import net.soundpush.engine.RouteView
import net.soundpush.engine.SoundPush
import net.soundpush.ui.R
import net.soundpush.ui.components.BannerModel
import net.soundpush.ui.components.EmptyState
import net.soundpush.ui.components.IconTile
import net.soundpush.ui.components.Labels
import net.soundpush.ui.components.LocalWidthClass
import net.soundpush.ui.components.QualityBadge
import net.soundpush.ui.components.SectionTitle
import net.soundpush.ui.components.SettingSlider
import net.soundpush.ui.components.SettingSwitch
import net.soundpush.ui.components.StatusBanner
import net.soundpush.ui.components.TaskCard
import net.soundpush.ui.components.WidthClass
import net.soundpush.ui.components.formatElapsed
import net.soundpush.ui.components.readableWidth
import net.soundpush.ui.components.rememberFormat
import net.soundpush.ui.icons.SpIcons
import net.soundpush.ui.theme.Tokens
import kotlin.math.roundToInt

private data class Task(
    val titleRes: Int,
    val descRes: Int,
    val icon: ImageVector,
    val kinds: List<String>,
    /** Shown when no connected device can do this. */
    val reasonRes: Int,
    val supports: (PeerView) -> Boolean,
)

/** Why one device in the picker can't do [task]. */
private fun peerReasonRes(task: Task, peer: PeerView): Int = when {
    task.kinds.contains("receiveSystemAudio") && !peer.canSendSystemAudio -> R.string.home_peer_reason_listen
    task.kinds.contains("receiveMixed") && !peer.canSendMixed -> R.string.home_peer_reason_listen
    task.kinds.contains("sendMicToVirtualMic") && !peer.hasVirtualMic -> R.string.home_peer_reason_mic
    else -> R.string.home_peer_reason_play
}

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
    Task(R.string.task_send_apps, R.string.task_send_apps_desc, SpIcons.Apps, listOf("sendAppAudio"), R.string.home_reason_play) {
        it.canPlay
    },
    // The computer's sound and its microphone in one stream (plan §5.1); it offers the mixed
    // source only where it can capture both, so the task hides itself on devices that cannot.
    Task(R.string.task_listen_mixed, R.string.task_listen_mixed_desc, SpIcons.Headset, listOf("receiveMixed"), R.string.home_reason_listen) {
        it.canSendMixed
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
    /** Contextual problems and tips (Bluetooth delay, mobile data, notifications off). */
    banners: List<BannerModel> = emptyList(),
    /** Extra link label for a device, e.g. "USB" when it is reached over USB tethering. */
    peerLabel: (PeerView) -> String? = { null },
) {
    // The open device picker survives rotation; tasks are identified by their title resource.
    var pickingRes by rememberSaveable { mutableStateOf<Int?>(null) }
    val picking = TASKS.firstOrNull { it.titleRes == pickingRes }

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

    // A task is running with a peer when every one of its route kinds is open with that peer.
    val running = state.routes.filter { it.status != "stopped" }
    fun runs(task: Task, peerId: String) = task.kinds.all { kind -> running.any { it.peerId == peerId && it.kind == kind } }

    /** Peers [task] runs with. A larger task (headset) claims its parts, so it lights one card, not three. */
    fun activePeers(task: Task): List<PeerView> = state.trustedPeers.filter { peer ->
        runs(task, peer.deviceId) &&
            TASKS.none { other ->
                other !== task && other.kinds.size > task.kinds.size && other.kinds.containsAll(task.kinds) && runs(other, peer.deviceId)
            }
    }

    /** Connection problems and tips. */
    fun LazyListScope.statusItems() {
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

        items(banners, key = { "banner-${it.key}" }) { banner -> StatusBanner(banner) }
    }

    /** What is streaming now. */
    fun LazyListScope.routeItems() {
        if (state.routes.isNotEmpty()) {
            item(key = "active-title") { SectionTitle(stringResource(R.string.home_active)) }
            items(state.routes, key = { it.routeId }) { route ->
                RouteCard(route, state.peers.firstOrNull { it.deviceId == route.peerId })
            }
        }
    }

    /** "What do you want to do?" task cards. */
    fun LazyListScope.taskItems() {
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
            val activeWith = activePeers(task)
            val activeNames = activeWith.joinToString { it.name }
            val activeHint = if (activeWith.isEmpty()) null else stringResource(R.string.home_task_active_hint, activeNames)
            TaskCard(
                title = stringResource(task.titleRes),
                description = when {
                    activeWith.isNotEmpty() -> stringResource(R.string.home_task_active, activeNames)
                    else -> reason ?: stringResource(task.descRes)
                },
                icon = task.icon,
                enabled = reason == null,
                active = activeWith.isNotEmpty(),
            ) {
                when {
                    // Already running with the only device there is: nothing to start, point at Stop.
                    activeHint != null && connected.size <= 1 -> onShowMessage(activeHint)
                    reason != null -> onShowMessage(reason)
                    // With one connected device there is nothing to choose.
                    connected.size == 1 -> onStartRoutes(capable[0].deviceId, task.kinds)
                    // With several, always ask, so audio never goes to a device the user didn't pick.
                    else -> pickingRes = task.titleRes
                }
            }
        }
    }

    /** Paired devices and their connection state. */
    fun LazyListScope.peerItems() {
        item(key = "peers-title") { SectionTitle(stringResource(R.string.devices_paired)) }
        items(state.trustedPeers, key = { "peer-${it.deviceId}" }) { peer -> PeerRow(peer, peerLabel(peer), onOpenDevices) }
    }

    if (LocalWidthClass.current == WidthClass.Expanded) {
        // Tablets and unfolded foldables: what to do on one side, what is running and with whom on the other.
        Row(Modifier.fillMaxSize()) {
            HomeColumn(Modifier.weight(1f)) {
                statusItems()
                taskItems()
            }
            HomeColumn(Modifier.weight(1f)) {
                routeItems()
                peerItems()
            }
        }
    } else {
        HomeColumn(Modifier.readableWidth()) {
            statusItems()
            routeItems()
            taskItems()
            peerItems()
        }
    }

    // The list follows live state: devices that disconnect disappear, the dialog closes when none are left.
    picking?.takeIf { connected.isNotEmpty() }?.let { task ->
        AlertDialog(
            onDismissRequest = { pickingRes = null },
            title = { Text(stringResource(R.string.home_pick_device)) },
            text = {
                Column {
                    connected.forEach { peer ->
                        val alreadyRunning = runs(task, peer.deviceId)
                        val supported = task.supports(peer) && !alreadyRunning
                        Row(
                            Modifier
                                .fillMaxWidth()
                                .heightIn(min = 56.dp)
                                .clickable(enabled = supported) {
                                    pickingRes = null
                                    onStartRoutes(peer.deviceId, task.kinds)
                                }
                                .alpha(if (supported) 1f else 0.5f),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Icon(SpIcons.forPlatform(peer.platform), null, tint = MaterialTheme.colorScheme.onSurfaceVariant)
                            Spacer(Modifier.width(Tokens.Space.md))
                            Column(Modifier.weight(1f)) {
                                Text(peer.name, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
                                if (!supported) {
                                    Text(
                                        stringResource(if (alreadyRunning) R.string.home_peer_reason_active else peerReasonRes(task, peer)),
                                        style = MaterialTheme.typography.bodySmall,
                                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                                    )
                                }
                            }
                        }
                    }
                }
            },
            confirmButton = { TextButton(onClick = { pickingRes = null }) { Text(stringResource(R.string.common_cancel)) } },
        )
    }
}

@Composable
private fun HomeColumn(modifier: Modifier, content: LazyListScope.() -> Unit) {
    LazyColumn(
        modifier = modifier.fillMaxHeight(),
        contentPadding = PaddingValues(start = Tokens.Space.md, end = Tokens.Space.md, top = Tokens.Space.xs, bottom = Tokens.Space.lg),
        verticalArrangement = Arrangement.spacedBy(Tokens.Space.sm),
        content = content,
    )
}

@Composable
private fun PeerRow(peer: PeerView, label: String?, onOpen: () -> Unit) {
    Surface(
        onClick = onOpen,
        shape = MaterialTheme.shapes.medium,
        color = MaterialTheme.colorScheme.background,
        modifier = Modifier.fillMaxWidth(),
    ) {
        Row(Modifier.heightIn(min = 60.dp).padding(horizontal = 4.dp), verticalAlignment = Alignment.CenterVertically) {
            IconTile(SpIcons.forPlatform(peer.platform), active = peer.isConnected)
            Spacer(Modifier.width(Tokens.Space.md))
            Column(Modifier.weight(1f)) {
                Text(peer.name, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
                val status = stringResource(Labels.status(peer.connection))
                Text(
                    if (label != null) stringResource(R.string.status_with_link, status, label) else status,
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

/** Mute (shows the action it performs; the icon tile shows the current state) and Stop. */
@Composable
private fun RouteControls(route: RouteView) {
    IconButton(onClick = { SoundPush.command { setRouteMuted(route.routeId, !route.muted) } }) {
        Icon(
            if (route.muted) SpIcons.Speaker else SpIcons.Mute,
            stringResource(if (route.muted) R.string.route_unmute else R.string.route_mute),
            tint = if (route.muted) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
    FilledTonalButton(onClick = { SoundPush.command { stopRoute(route.routeId) } }) {
        Text(stringResource(R.string.route_stop))
    }
}

@Composable
private fun RouteCard(route: RouteView, peer: PeerView?) {
    var expanded by rememberSaveable(route.routeId) { mutableStateOf(false) }
    val quality = peer?.quality ?: "unknown"
    val qualityLabel = Labels.quality(quality)?.let { stringResource(it) } ?: ""
    val icon = when {
        route.muted -> SpIcons.Mute
        route.isMic -> SpIcons.Mic
        route.kind.contains("App") -> SpIcons.Apps
        else -> SpIcons.Speaker
    }

    val toggleLabel = stringResource(if (expanded) R.string.a11y_hide_details else R.string.a11y_show_details)
    val largeText = LocalDensity.current.fontScale >= 1.5f
    Surface(
        onClick = { expanded = !expanded },
        shape = MaterialTheme.shapes.medium,
        border = BorderStroke(1.dp, MaterialTheme.colorScheme.outline),
        color = MaterialTheme.colorScheme.surface,
        modifier = Modifier.fillMaxWidth().semantics {
            onClick(label = toggleLabel) {
                expanded = !expanded
                true
            }
        },
    ) {
        Column(Modifier.padding(Tokens.Space.md)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                IconTile(icon, route.status == "active")
                Spacer(Modifier.width(12.dp))
                // Route state changes ("Reconnecting…", "Starting…") are announced as they happen.
                Column(Modifier.weight(1f).semantics(mergeDescendants = true) { liveRegion = LiveRegionMode.Polite }) {
                    Text(
                        stringResource(Labels.routeTitle(route.kind), route.peerName),
                        style = MaterialTheme.typography.titleMedium,
                        maxLines = 2,
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
                if (!largeText) RouteControls(route)
            }
            // At large font sizes the buttons get their own row so the title and status keep the full width.
            if (largeText) {
                Row(Modifier.align(Alignment.End).padding(top = Tokens.Space.sm), verticalAlignment = Alignment.CenterVertically) {
                    RouteControls(route)
                }
            }

            if (expanded) {
                HorizontalDivider(Modifier.padding(vertical = 12.dp), color = MaterialTheme.colorScheme.outline)
                if (!route.isSending && !route.isMic) {
                    val percent = rememberFormat(R.string.unit_percent)
                    SettingSlider(
                        label = stringResource(R.string.route_volume),
                        value = route.volume,
                        range = 0f..2f,
                        format = { percent((it * 100).roundToInt()) },
                    ) { v -> SoundPush.command { setRouteVolume(route.routeId, v) } }
                }
                if (route.kind == "receiveSystemAudio") {
                    // The engine's state, not a local flag: survives rotation, reconnects and changes from elsewhere.
                    SettingSwitch(stringResource(R.string.route_mute_pc), peer?.speakersMuted == true) { v ->
                        SoundPush.command { setPeerSpeakersMuted(route.peerId, v) }
                    }
                }
                // Collected only while the details are open.
                val output by DeviceStatus.output.collectAsState()
                val outputLatencyMs by DeviceStatus.outputLatencyMs.collectAsState()
                ConnectionDetails(route, peer, output, outputLatencyMs)
            }
        }
    }
}
