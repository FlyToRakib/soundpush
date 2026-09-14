package net.soundpush.engine

import android.content.Context
import android.media.AudioDeviceInfo
import android.media.AudioManager
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow

/**
 * The output this phone plays on when the user picks one instead of letting Android decide
 * (Settings → Audio → Output). Device-specific and Android-only, so it is kept in the app's local
 * preferences rather than the settings file shared with the desktop.
 *
 * Android lets an app send its own playback to a connected device (AudioTrack.setPreferredDevice);
 * the low-latency native path has no such hook, so a chosen output moves playback to the platform
 * player, which adds a little delay, the same way output audio effects do.
 */
object OutputPreference {
    enum class Target(val key: String) { Automatic("auto"), Speaker("speaker"), Wired("wired"), Bluetooth("bluetooth"), Usb("usb") }

    private const val PREFS = "soundpush_local"
    private const val KEY = "outputDevice"

    private val _target = MutableStateFlow(Target.Automatic)
    val target: StateFlow<Target> = _target

    /** Read the saved choice. Call off the main thread: the first preferences access reads a file. */
    fun load(context: Context) {
        val key = context.applicationContext.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getString(KEY, null)
        _target.value = fromKey(key)
    }

    fun set(context: Context, target: Target) {
        _target.value = target
        context.applicationContext.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit().putString(KEY, target.key).apply()
    }

    fun fromKey(key: String?): Target = Target.entries.firstOrNull { it.key == key } ?: Target.Automatic

    /** Output kinds connected right now, in a stable order. */
    fun available(am: AudioManager): List<Target> =
        am.getDevices(AudioManager.GET_DEVICES_OUTPUTS).mapNotNull { targetFor(it.type) }.distinct().sorted()

    /** The connected device for [target], or null to let Android route (Automatic, or not connected). */
    fun device(am: AudioManager, target: Target): AudioDeviceInfo? =
        if (target == Target.Automatic) null else am.getDevices(AudioManager.GET_DEVICES_OUTPUTS).firstOrNull { targetFor(it.type) == target }

    /** Media outputs only: Bluetooth SCO and the earpiece are call audio. Literal values cover LE Audio (API 31+). */
    internal fun targetFor(type: Int): Target? = when (type) {
        AudioDeviceInfo.TYPE_BUILTIN_SPEAKER -> Target.Speaker
        AudioDeviceInfo.TYPE_WIRED_HEADPHONES, AudioDeviceInfo.TYPE_WIRED_HEADSET, AudioDeviceInfo.TYPE_LINE_ANALOG -> Target.Wired
        AudioDeviceInfo.TYPE_BLUETOOTH_A2DP, 23, 26, 27 -> Target.Bluetooth // hearing aid, LE headset, LE speaker
        AudioDeviceInfo.TYPE_USB_HEADSET, AudioDeviceInfo.TYPE_USB_DEVICE -> Target.Usb
        else -> null
    }
}
