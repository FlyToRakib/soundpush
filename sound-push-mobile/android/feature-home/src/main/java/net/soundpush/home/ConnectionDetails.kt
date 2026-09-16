package net.soundpush.home

import androidx.annotation.StringRes
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import net.soundpush.engine.DeviceStatus
import net.soundpush.engine.PeerView
import net.soundpush.engine.RouteView
import net.soundpush.ui.R
import net.soundpush.ui.components.Labels
import net.soundpush.ui.theme.Tokens
import kotlin.math.roundToInt

/**
 * "Connection details" (plan §23.3): the same numbers as the desktop's route card (codec, latency,
 * buffer, jitter, loss, drift, dropouts, round trip), how the phone is connected, and, for audio
 * this phone plays, where it plays and how the delay adds up.
 */
@Composable
internal fun ConnectionDetails(route: RouteView, peer: PeerView?, output: DeviceStatus.Output, outputLatencyMs: Int) {
    val s = route.stats
    val rttMs = (peer?.rttMs ?: 0.0).roundToInt()
    val items = buildList {
        add(Item(R.string.stats_codec, if (s.bitrateKbps > 0) stringResource(R.string.stats_codec_bitrate, s.codec, s.bitrateKbps) else s.codec))
        add(Item(R.string.stats_latency, stringResource(R.string.unit_ms, s.latencyMs.roundToInt())))
        add(Item(R.string.stats_buffer, stringResource(R.string.unit_ms, s.bufferMs.roundToInt())))
        add(Item(R.string.stats_jitter, stringResource(R.string.unit_ms_decimal, oneDecimal(s.jitterMs))))
        add(Item(R.string.stats_loss, stringResource(R.string.unit_percent_decimal, oneDecimal(s.lossPct))))
        add(Item(R.string.stats_drift, stringResource(R.string.unit_ppm, s.driftPpm)))
        add(Item(R.string.stats_dropouts, s.underruns.toString()))
        add(Item(R.string.stats_rtt, stringResource(R.string.unit_ms, rttMs)))
    }

    Column(Modifier.fillMaxWidth().padding(top = Tokens.Space.sm), verticalArrangement = Arrangement.spacedBy(Tokens.Space.sm)) {
        Text(
            stringResource(R.string.route_details),
            style = MaterialTheme.typography.titleSmall,
            modifier = Modifier.semantics { heading() },
        )
        // Two columns; rows grow with the text, so large fonts and long translations wrap instead of clipping.
        items.chunked(2).forEach { pair ->
            Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.md)) {
                pair.forEach { item -> Stat(stringResource(item.label), item.value, Modifier.weight(1f)) }
                if (pair.size == 1) Column(Modifier.weight(1f)) {}
            }
        }
        val link = when (peer?.transport) {
            "tcp" -> stringResource(R.string.transport_usb)
            "quic" -> stringResource(R.string.transport_network)
            else -> null
        }
        if (link != null) {
            val address = peer?.addresses?.firstOrNull()
            Stat(stringResource(R.string.stats_connection), if (address != null) stringResource(R.string.status_with_link, link, address) else link)
        }
        if (!route.isSending) {
            val outputName = stringResource(Labels.output(output))
            Stat(
                stringResource(R.string.stats_output),
                if (outputLatencyMs > 0) stringResource(R.string.status_with_link, outputName, stringResource(R.string.unit_ms, outputLatencyMs)) else outputName,
            )
            // One way over the network is about half the round trip; output delay is known only on the platform player.
            Stat(
                stringResource(R.string.stats_breakdown_title),
                stringResource(
                    R.string.stats_breakdown,
                    (rttMs + 1) / 2,
                    s.bufferMs.roundToInt(),
                    if (outputLatencyMs > 0) stringResource(R.string.unit_ms, outputLatencyMs) else stringResource(R.string.stats_not_measured),
                ),
            )
        }
    }
}

private data class Item(@param:StringRes val label: Int, val value: String)

@Composable
private fun Stat(label: String, value: String, modifier: Modifier = Modifier) {
    // Read as "Jitter, 1.2 ms".
    Column(modifier.semantics(mergeDescendants = true) {}) {
        Text(label, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        Text(value, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurface)
    }
}

private fun oneDecimal(value: Double) = "%.1f".format(value)
