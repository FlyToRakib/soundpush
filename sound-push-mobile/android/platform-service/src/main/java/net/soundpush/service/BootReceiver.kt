package net.soundpush.service

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import net.soundpush.engine.EngineJson
import net.soundpush.engine.Settings
import net.soundpush.engine.SoundPush
import java.io.File

/**
 * Android 15+ forbids starting playback/microphone foreground services from
 * BOOT_COMPLETED, so instead of silently failing we post a reconnect reminder.
 * The engine itself starts only when something needs it after a restart: "Stay available"
 * (a connected-device service, which the OS still allows from boot).
 */
class BootReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action != Intent.ACTION_BOOT_COMPLETED) return
        val settingsFile = File(context.filesDir, "settings.json")
        val settings = runCatching {
            EngineJson.decodeFromString(Settings.serializer(), settingsFile.readText())
        }.getOrNull() ?: return
        if (settings.mobile.remindAfterRestart) {
            Notifications.ensureChannels(context)
            Notifications.reconnectReminder(context)
        }
        if (settings.mobile.stayAvailable) SoundPush.ensureStarted(context)
    }
}
