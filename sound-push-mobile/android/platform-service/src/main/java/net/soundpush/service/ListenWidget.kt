package net.soundpush.service

import android.app.PendingIntent
import android.appwidget.AppWidgetManager
import android.appwidget.AppWidgetProvider
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.widget.RemoteViews
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.launch
import net.soundpush.engine.EngineState
import net.soundpush.engine.SoundPush
import net.soundpush.ui.R
import net.soundpush.service.R as ServiceR

/**
 * Home-screen widget: "Listen to computer" with its status. Tapping starts or stops listening
 * through the streaming service. Battery-friendly: no periodic updates; the app pushes a new
 * picture only when what the widget shows changes.
 */
class ListenWidget : AppWidgetProvider() {

    override fun onUpdate(context: Context, appWidgetManager: AppWidgetManager, appWidgetIds: IntArray) {
        render(context, appWidgetManager, appWidgetIds, snapshot(SoundPush.state.value))
    }

    /** What the widget shows; compared so unrelated engine updates (stats) don't redraw it. */
    data class Snapshot(val listeningTo: String?, val available: String?)

    companion object {
        fun snapshot(state: EngineState?): Snapshot {
            val listening = state?.routes?.firstOrNull { it.kind == "receiveSystemAudio" && it.status != "stopped" }
            val available = state?.connectedPeers?.firstOrNull { it.canSendSystemAudio }
            return Snapshot(listening?.peerName, available?.name)
        }

        /** Keep placed widgets in sync with the engine for as long as [scope] lives. */
        fun observe(scope: CoroutineScope, context: Context) {
            val app = context.applicationContext
            scope.launch {
                SoundPush.state.map { snapshot(it) }.distinctUntilChanged().collect { update(app, it) }
            }
        }

        private fun update(context: Context, snapshot: Snapshot) {
            val manager = AppWidgetManager.getInstance(context) ?: return
            val ids = runCatching { manager.getAppWidgetIds(ComponentName(context, ListenWidget::class.java)) }.getOrNull()
            if (ids == null || ids.isEmpty()) return
            render(context, manager, ids, snapshot)
        }

        private fun render(context: Context, manager: AppWidgetManager, ids: IntArray, snapshot: Snapshot) {
            val title = context.getString(R.string.task_listen)
            val status = when {
                snapshot.listeningTo != null -> context.getString(R.string.widget_listening, snapshot.listeningTo)
                snapshot.available != null -> context.getString(R.string.widget_tap_listen, snapshot.available)
                else -> context.getString(R.string.widget_tap_connect)
            }
            val views = RemoteViews(context.packageName, ServiceR.layout.widget_listen).apply {
                setTextViewText(ServiceR.id.widget_title, title)
                setTextViewText(ServiceR.id.widget_status, status)
                setImageViewResource(
                    ServiceR.id.widget_action,
                    if (snapshot.listeningTo != null) ServiceR.drawable.ic_action_stop else ServiceR.drawable.ic_widget_play,
                )
                setContentDescription(
                    ServiceR.id.widget_root,
                    context.getString(if (snapshot.listeningTo != null) R.string.widget_action_stop else R.string.widget_action_listen) + ". $status",
                )
                setOnClickPendingIntent(ServiceR.id.widget_root, toggleIntent(context))
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

        private const val REQUEST_TOGGLE = 10
    }
}
