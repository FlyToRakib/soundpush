package net.soundpush.service

import android.Manifest
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.support.v4.media.session.MediaSessionCompat
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat
import androidx.media.app.NotificationCompat.MediaStyle
import net.soundpush.engine.DeviceStatus
import net.soundpush.engine.EngineState
import net.soundpush.ui.R
import net.soundpush.ui.components.Labels
import net.soundpush.service.R as ServiceR

object Notifications {
    const val CHANNEL_STREAMING = "streaming"
    const val CHANNEL_ATTENTION = "attention"
    const val ID_STREAMING = 1
    const val ID_ATTENTION = 2
    const val ID_REMINDER = 3

    const val ACTION_STOP_ALL = "net.soundpush.STOP_ALL"
    const val ACTION_TOGGLE_MUTE = "net.soundpush.TOGGLE_MUTE"
    const val ACTION_TOGGLE_PLAYBACK_MUTE = "net.soundpush.TOGGLE_PLAYBACK_MUTE"

    fun ensureChannels(context: Context) {
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

    fun serviceAction(context: Context, action: String, code: Int): PendingIntent = PendingIntent.getService(
        context,
        code,
        Intent(context, StreamingService::class.java).setAction(action),
        PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
    )

    /**
     * The foreground-service notification. While this phone plays audio it is a media notification
     * tied to [session], which gives it media controls and, on Android 11+, the system output
     * switcher. Stop and Mute are always there; the microphone route adds "Microphone in use", or
     * says so when another app has taken the microphone and Android is sending silence ([micSilenced],
     * plan §8.3).
     */
    fun streaming(
        context: Context,
        state: EngineState?,
        micLive: Boolean,
        session: MediaSessionCompat.Token? = null,
        connecting: Boolean = false,
        output: DeviceStatus.Output? = null,
        micSilenced: Boolean = false,
    ) = run {
        val active = state?.routes?.filter { it.status == "active" }.orEmpty()
        val receiving = state?.routes?.filter { !it.isSending && it.status != "stopped" }.orEmpty()
        val peer = active.firstOrNull()?.peerName ?: state?.connectedPeers?.firstOrNull()?.name ?: ""
        val title = when {
            connecting && state?.routes.isNullOrEmpty() -> context.getString(R.string.notif_listen_connecting)
            active.isEmpty() -> context.getString(R.string.notif_available)
            else -> context.getString(R.string.notif_streaming, peer)
        }
        val builder = NotificationCompat.Builder(context, CHANNEL_STREAMING)
            .setSmallIcon(R.drawable.ic_notification)
            .setContentTitle(title)
            .setContentIntent(openAppIntent(context))
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .setShowWhen(false)
            .setVisibility(NotificationCompat.VISIBILITY_PUBLIC)
            .setForegroundServiceBehavior(NotificationCompat.FOREGROUND_SERVICE_IMMEDIATE)

        // Every action is shown in the compact media view too (at most three), in the order added.
        val compact = mutableListOf<Int>()
        fun action(icon: Int, label: String, pending: PendingIntent) {
            compact += compact.size
            builder.addAction(icon, label, pending)
        }

        fun action(icon: Int, label: Int, pending: PendingIntent) = action(icon, context.getString(label), pending)
        if (session != null && receiving.isNotEmpty()) {
            val muted = receiving.all { it.muted }
            builder.setContentText(context.getString(Labels.routeTitle(receiving.first().kind), receiving.first().peerName))
            action(
                if (muted) ServiceR.drawable.ic_action_unmute else ServiceR.drawable.ic_action_mute,
                if (muted) R.string.route_unmute else R.string.route_mute,
                serviceAction(context, ACTION_TOGGLE_PLAYBACK_MUTE, 3),
            )
        }
        if (active.isNotEmpty() || receiving.isNotEmpty()) {
            action(ServiceR.drawable.ic_action_stop, R.string.route_stop, serviceAction(context, ACTION_STOP_ALL, 1))
        }
        if (micLive) {
            val micText = context.getString(if (micSilenced) R.string.notif_mic_silenced else R.string.notif_mic_live)
            builder.setSubText(micText)
            if (session == null) builder.setContentText(micText)
            val micMuted = state?.routes?.filter { it.isMic && it.isSending }.orEmpty().let { it.isNotEmpty() && it.all { r -> r.muted } }
            action(
                ServiceR.drawable.ic_action_mic_off,
                if (micMuted) R.string.route_unmute_mic else R.string.route_mute_mic,
                serviceAction(context, ACTION_TOGGLE_MUTE, 2),
            )
        }
        // Stop / Mute / Output (plan §25.1). Android 13+ media controls also show their own output
        // chip; older versions only have this action.
        if (session != null && receiving.isNotEmpty() && output != null) {
            action(
                ServiceR.drawable.ic_action_output,
                context.getString(R.string.notif_output, context.getString(Labels.output(output))),
                OutputSwitcher.pendingIntent(context),
            )
        }
        if (session != null && receiving.isNotEmpty()) {
            builder.setCategory(NotificationCompat.CATEGORY_TRANSPORT)
            builder.setStyle(
                MediaStyle()
                    .setMediaSession(session)
                    .setShowActionsInCompactView(*compact.take(3).toIntArray()),
            )
        } else {
            builder.setCategory(NotificationCompat.CATEGORY_SERVICE)
        }
        builder.build()
    }

    /** Android 13+ needs the user's permission before any notification is posted. */
    private fun post(context: Context, id: Int, notification: android.app.Notification) {
        if (Build.VERSION.SDK_INT >= 33 &&
            ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
        ) {
            return
        }
        try {
            NotificationManagerCompat.from(context).notify(id, notification)
        } catch (e: SecurityException) {
            // Permission revoked between the check and the call.
        }
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
        post(context, ID_ATTENTION, n)
    }

    fun reconnectReminder(context: Context) {
        val n = NotificationCompat.Builder(context, CHANNEL_ATTENTION)
            .setSmallIcon(R.drawable.ic_notification)
            .setContentTitle(context.getString(R.string.notif_reconnect))
            .setContentIntent(openAppIntent(context))
            .setAutoCancel(true)
            .build()
        post(context, ID_REMINDER, n)
    }

    /** The widget asked to listen, but no computer connected in time: send the user to the app. */
    fun listenUnavailable(context: Context) {
        val n = NotificationCompat.Builder(context, CHANNEL_ATTENTION)
            .setSmallIcon(R.drawable.ic_notification)
            .setContentTitle(context.getString(R.string.notif_listen_unavailable))
            .setContentText(context.getString(R.string.notif_listen_unavailable_body))
            .setContentIntent(openAppIntent(context))
            .setAutoCancel(true)
            .build()
        post(context, ID_ATTENTION, n)
    }
}
