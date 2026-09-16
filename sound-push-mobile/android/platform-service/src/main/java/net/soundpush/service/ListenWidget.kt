package net.soundpush.service

import android.app.PendingIntent
import android.appwidget.AppWidgetManager
import android.appwidget.AppWidgetProvider
import android.content.BroadcastReceiver
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.view.View
import android.widget.RemoteViews
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.launch
import net.soundpush.engine.EngineState
import net.soundpush.engine.RouteView
import net.soundpush.engine.SoundPush
import net.soundpush.ui.R
import net.soundpush.service.R as ServiceR

/**
 * Home-screen widget: "Listen to computer" with its status, and Mute while listening. Tapping the
 * widget starts or stops listening through the streaming service. Battery-friendly: no periodic
 * updates; the app pushes a new picture only when what the widget shows changes.
 */
class ListenWidget : AppWidgetProvider() {

    override fun onUpdate(context: Context, appWidgetManager: AppWidgetManager, appWidgetIds: IntArray) {
        render(context, appWidgetManager, appWidgetIds, snapshot(SoundPush.state.value))
    }

    /** What the widget shows; compared so unrelated engine updates (stats) don't redraw it. */
    data class Snapshot(val listeningTo: String?, val available: String?, val muted: Boolean = false)

    companion object {
        private const val LISTEN_KIND = "receiveSystemAudio"

        internal fun listeningRoutes(state: EngineState?): List<RouteView> = state?.routes?.filter { it.kind == LISTEN_KIND && it.status != "stopped" }.orEmpty()

        fun snapshot(state: EngineState?): Snapshot {
            val listening = listeningRoutes(state)
            val available = state?.connectedPeers?.firstOrNull { it.canSendSystemAudio }
            return Snapshot(listening.firstOrNull()?.peerName, available?.name, listening.isNotEmpty() && listening.all { it.muted })
        }

        /** Keep placed widgets in sync with the engine for as long as [scope] lives. */
        fun observe(scope: CoroutineScope, context: Context) {
            val app = context.applicationContext
            scope.launch {
                SoundPush.state.map { snapshot(it) }.distinctUntilChanged().collect { update(app, it) }
            }
        }

        /** Redraw from the current engine state (e.g. a tap on a widget that is out of date). */
        internal fun refresh(context: Context) = update(context.applicationContext, snapshot(SoundPush.state.value))

        private fun update(context: Context, snapshot: Snapshot) {
            val manager = AppWidgetManager.getInstance(context) ?: return
            val ids = runCatching { manager.getAppWidgetIds(ComponentName(context, ListenWidget::class.java)) }.getOrNull()
            if (ids == null || ids.isEmpty()) return
            render(context, manager, ids, snapshot)
        }

        private fun render(context: Context, manager: AppWidgetManager, ids: IntArray, snapshot: Snapshot) {
            val title = context.getString(R.string.task_listen)
            val listening = snapshot.listeningTo != null
            val status = when {
                listening && snapshot.muted -> context.getString(R.string.widget_listening_muted, snapshot.listeningTo)
                listening -> context.getString(R.string.widget_listening, snapshot.listeningTo)
                snapshot.available != null -> context.getString(R.string.widget_tap_listen, snapshot.available)
                else -> context.getString(R.string.widget_tap_connect)
            }
            val views = RemoteViews(context.packageName, ServiceR.layout.widget_listen).apply {
                setTextViewText(ServiceR.id.widget_title, title)
                setTextViewText(ServiceR.id.widget_status, status)
                setImageViewResource(
                    ServiceR.id.widget_action,
                    if (listening) ServiceR.drawable.ic_action_stop else ServiceR.drawable.ic_widget_play,
                )
                setContentDescription(
                    ServiceR.id.widget_root,
                    context.getString(if (listening) R.string.widget_action_stop else R.string.widget_action_listen) + ". $status",
                )
                setOnClickPendingIntent(ServiceR.id.widget_root, toggleIntent(context))
                // Mute has its own touch target, shown only while there is something to mute.
                setViewVisibility(ServiceR.id.widget_mute, if (listening) View.VISIBLE else View.GONE)
                if (listening) {
                    setImageViewResource(
                        ServiceR.id.widget_mute,
                        if (snapshot.muted) ServiceR.drawable.ic_action_unmute else ServiceR.drawable.ic_action_mute,
                    )
                    setContentDescription(
                        ServiceR.id.widget_mute,
                        context.getString(if (snapshot.muted) R.string.widget_unmute_desc else R.string.widget_mute_desc),
                    )
                    setOnClickPendingIntent(ServiceR.id.widget_mute, muteIntent(context))
                }
            }
            runCatching { manager.updateAppWidget(ids, views) }
        }

        /** A widget tap may start the foreground service: the user is interacting with the widget. */
        private fun toggleIntent(context: Context): PendingIntent = PendingIntent.getForegroundService(
            context,
            REQUEST_TOGGLE,
            Intent(context, StreamingService::class.java).setAction(StreamingService.ACTION_TOGGLE_LISTEN),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )

        /** Muting needs no service start: a broadcast to this app is enough. */
        private fun muteIntent(context: Context): PendingIntent = PendingIntent.getBroadcast(
            context,
            REQUEST_MUTE,
            Intent(context, WidgetMuteReceiver::class.java),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )

        private const val REQUEST_TOGGLE = 10
        private const val REQUEST_MUTE = 11
    }
}

/**
 * The widget's Mute button (not exported). It is only shown while listening, when the streaming
 * service keeps the process and the engine running; on a stale widget it just redraws.
 */
class WidgetMuteReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val listening = ListenWidget.listeningRoutes(SoundPush.state.value)
        if (listening.isEmpty()) {
            ListenWidget.refresh(context)
            return
        }
        val muted = listening.all { it.muted }
        listening.forEach { r -> SoundPush.command { setRouteMuted(r.routeId, !muted) } }
    }
}
