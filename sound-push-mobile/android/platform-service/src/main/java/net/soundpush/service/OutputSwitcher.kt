package net.soundpush.service

import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.media.MediaRouter2
import android.os.Build
import android.provider.Settings

/**
 * Opens the system output switcher (phone speaker, wired, Bluetooth) from the media notification.
 *
 * - Android 14+: MediaRouter2's system output switcher.
 * - Android 12–13: System UI's media output dialog (the one the media controls open).
 * - Android 11: the Settings media output panel.
 * - Android 8–10 (or when the panel is missing): Bluetooth settings, the closest those versions offer.
 *
 * Android 12+ blocks activities started from a notification's service or broadcast (trampolines),
 * so the dialogs go through [OutputSwitcherReceiver] and the activity fallbacks are the
 * notification's own PendingIntent.
 */
object OutputSwitcher {
    internal const val ACTION_SHOW = "net.soundpush.SHOW_OUTPUT_SWITCHER"
    private const val REQUEST = 4
    private const val SYSTEM_UI = "com.android.systemui"
    private const val ACTION_SYSTEM_UI_DIALOG = "com.android.systemui.action.LAUNCH_MEDIA_OUTPUT_DIALOG"
    private const val EXTRA_SYSTEM_UI_PACKAGE = "package_name"
    private const val ACTION_SETTINGS_PANEL = "com.android.settings.panel.action.MEDIA_OUTPUT"
    private const val EXTRA_SETTINGS_PANEL_PACKAGE = "com.android.settings.panel.extra.PACKAGE_NAME"

    fun pendingIntent(context: Context): PendingIntent {
        val flags = PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
        return if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            val intent = Intent(context, OutputSwitcherReceiver::class.java).setAction(ACTION_SHOW)
            PendingIntent.getBroadcast(context, REQUEST, intent, flags)
        } else {
            PendingIntent.getActivity(context, REQUEST, activityIntent(context), flags)
        }
    }

    private fun activityIntent(context: Context): Intent {
        if (Build.VERSION.SDK_INT == Build.VERSION_CODES.R) {
            val panel = Intent(ACTION_SETTINGS_PANEL).putExtra(EXTRA_SETTINGS_PANEL_PACKAGE, context.packageName)
            // Declared in <queries>, so the lookup sees the Settings app.
            if (panel.resolveActivity(context.packageManager) != null) return panel.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        }
        return Intent(Settings.ACTION_BLUETOOTH_SETTINGS).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
    }

    internal fun show(context: Context) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE &&
            runCatching { MediaRouter2.getInstance(context).showSystemOutputSwitcher() }.getOrDefault(false)
        ) {
            return
        }
        runCatching {
            context.sendBroadcast(
                Intent(ACTION_SYSTEM_UI_DIALOG)
                    .setPackage(SYSTEM_UI)
                    .putExtra(EXTRA_SYSTEM_UI_PACKAGE, context.packageName),
            )
        }
    }
}

/** Not exported: only this app's notification can ask for the output switcher. */
class OutputSwitcherReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action == OutputSwitcher.ACTION_SHOW) OutputSwitcher.show(context)
    }
}
