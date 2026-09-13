package net.soundpush.service

import android.service.quicksettings.Tile
import android.service.quicksettings.TileService
import net.soundpush.engine.SoundPush

/** Quick Settings tile: start or stop listening to the first connected computer. */
class StreamTileService : TileService() {

    override fun onStartListening() {
        refresh()
    }

    override fun onClick() {
        val state = SoundPush.state.value
        if (state == null) {
            // Engine not running (process was killed): open the app.
            unlockAndRun { startActivityAndCollapseCompat() }
            return
        }
        val active = state.routes.filter { it.kind == "receiveSystemAudio" }
        if (active.isNotEmpty()) {
            active.forEach { r -> SoundPush.command { stopRoute(r.routeId) } }
        } else {
            val peer = state.connectedPeers.firstOrNull { it.canSendSystemAudio }
            if (peer == null) {
                unlockAndRun { startActivityAndCollapseCompat() }
                return
            }
            SoundPush.command { startRoute(peer.deviceId, "receiveSystemAudio") }
        }
        refresh()
    }

    private fun refresh() {
        val tile = qsTile ?: return
        val listening = SoundPush.state.value?.routes?.any { it.kind == "receiveSystemAudio" } == true
        tile.state = if (listening) Tile.STATE_ACTIVE else Tile.STATE_INACTIVE
        tile.updateTile()
    }

    @Suppress("DEPRECATION")
    private fun startActivityAndCollapseCompat() {
        val intent = packageManager.getLaunchIntentForPackage(packageName) ?: return
        intent.addFlags(android.content.Intent.FLAG_ACTIVITY_NEW_TASK)
        if (android.os.Build.VERSION.SDK_INT >= 34) {
            startActivityAndCollapse(Notifications.openAppIntent(this))
        } else {
            startActivityAndCollapse(intent)
        }
    }
}
