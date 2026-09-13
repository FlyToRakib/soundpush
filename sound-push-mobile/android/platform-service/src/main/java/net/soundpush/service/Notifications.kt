package net.soundpush.service

import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.os.Build
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import net.soundpush.engine.EngineState
import net.soundpush.ui.R

object Notifications {
    const val CHANNEL_STREAMING = "streaming"
    const val CHANNEL_ATTENTION = "attention"
    const val ID_STREAMING = 1
    const val ID_ATTENTION = 2
    const val ID_REMINDER = 3

    const val ACTION_STOP_ALL = "net.soundpush.STOP_ALL"
    const val ACTION_TOGGLE_MUTE = "net.soundpush.TOGGLE_MUTE"

    fun ensureChannels(context: Context) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
        val nm = context.getSystemService(NotificationManager::class.java)
        nm.createNotificationChannel(
            NotificationChannel(CHANNEL_STREAMING, context.getString(R.string.notif_channel_streaming), NotificationManager.IMPORTANCE_LOW)
                .apply { setShowBadge(false) },
        )
        nm.createNotificationChannel(
            NotificationChannel(CHANNEL_ATTENTION, context.getString(R.string.notif_channel_attention), NotificationManager.IMPORTANCE_HIGH),
        )
    }

    /** Launches the app's main activity (resolved by package so this module doesn't depend on :app). */
    fun openAppIntent(context: Context): PendingIntent {
        val launch = context.packageManager.getLaunchIntentForPackage(context.packageName)
            ?.addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP)
            ?: Intent()
        return PendingIntent.getActivity(context, 0, launch, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
    }

    private fun serviceAction(context: Context, action: String, code: Int): PendingIntent =
        PendingIntent.getService(
            context,
            code,
            Intent(context, StreamingService::class.java).setAction(action),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )

    fun streaming(context: Context, state: EngineState?, micLive: Boolean) = run {
        val active = state?.routes?.filter { it.status == "active" }.orEmpty()
        val peer = active.firstOrNull()?.peerName ?: state?.connectedPeers?.firstOrNull()?.name ?: ""
        val title = if (active.isEmpty()) {
            context.getString(R.string.notif_available)
        } else {
            context.getString(R.string.notif_streaming, peer)
        }
        val builder = NotificationCompat.Builder(context, CHANNEL_STREAMING)
            .setSmallIcon(R.drawable.ic_notification)
            .setContentTitle(title)
            .setContentIntent(openAppIntent(context))
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .setCategory(NotificationCompat.CATEGORY_SERVICE)
            .setForegroundServiceBehavior(NotificationCompat.FOREGROUND_SERVICE_IMMEDIATE)
        if (active.isNotEmpty()) {
            builder.addAction(0, context.getString(R.string.route_stop), serviceAction(context, ACTION_STOP_ALL, 1))
        }
        if (micLive) {
            builder.setContentText(context.getString(R.string.notif_mic_live))
            builder.addAction(0, context.getString(R.string.route_mute), serviceAction(context, ACTION_TOGGLE_MUTE, 2))
        }
        builder.build()
    }

    fun attention(context: Context, peerName: String) {
        val n = NotificationCompat.Builder(context, CHANNEL_ATTENTION)
            .setSmallIcon(R.drawable.ic_notification)
            .setContentTitle(context.getString(R.string.notif_request, peerName))
            .setContentIntent(openAppIntent(context))
            .setAutoCancel(true)
            .setPriority(NotificationCompat.PRIORITY_HIGH)
            .setCategory(NotificationCompat.CATEGORY_CALL)
            .build()
        runCatching { NotificationManagerCompat.from(context).notify(ID_ATTENTION, n) }
    }

    fun reconnectReminder(context: Context) {
        val n = NotificationCompat.Builder(context, CHANNEL_ATTENTION)
            .setSmallIcon(R.drawable.ic_notification)
            .setContentTitle(context.getString(R.string.notif_reconnect))
            .setContentIntent(openAppIntent(context))
            .setAutoCancel(true)
            .build()
        runCatching { NotificationManagerCompat.from(context).notify(ID_REMINDER, n) }
    }
}
