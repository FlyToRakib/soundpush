package net.soundpush.service

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import net.soundpush.engine.EngineJson
import net.soundpush.engine.Settings
import java.io.File

/**
 * Android 15+ forbids starting playback/microphone foreground services from
 * BOOT_COMPLETED, so instead of silently failing we post a reconnect reminder.
 */
class BootReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action != Intent.ACTION_BOOT_COMPLETED) return
        val settingsFile = File(context.filesDir, "settings.json")
        val remind = runCatching {
            EngineJson.decodeFromString(Settings.serializer(), settingsFile.readText()).mobile.remindAfterRestart
        }.getOrDefault(false)
        if (remind) {
            Notifications.ensureChannels(context)
            Notifications.reconnectReminder(context)
        }
    }
}
