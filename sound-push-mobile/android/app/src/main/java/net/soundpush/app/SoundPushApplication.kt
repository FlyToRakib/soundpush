package net.soundpush.app

import android.app.Application
import android.content.ComponentCallbacks2
import android.net.wifi.WifiManager
import androidx.lifecycle.DefaultLifecycleObserver
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.ProcessLifecycleOwner
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.launch
import net.soundpush.engine.Caches
import net.soundpush.engine.DeviceStatus
import net.soundpush.engine.SoundPush
import net.soundpush.engine.preferLossless
import net.soundpush.service.AudioCues
import net.soundpush.service.ListenWidget
import net.soundpush.service.Notifications
import net.soundpush.service.ServiceDelegate
import net.soundpush.settings.registerSettingsCaches

/**
 * Process setup only. The engine is not started here: the process also starts for broadcasts
 * (boot, widget updates) that don't need it. The activity, the streaming service, the widget
 * and "Stay available" after a restart start it when they need it (SoundPush.ensureStarted).
 */
class SoundPushApplication : Application() {
    private val appScope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)

    /** Whether any screen of this app is in front. Combined with the screen state below. */
    private val appVisible = MutableStateFlow(false)

    override fun onCreate() {
        super.onCreate()
        CrashReports.install(this)
        SoundPush.configure(BuildVersion.NAME)
        Notifications.ensureChannels(this)
        val delegate = ServiceDelegate(this)
        SoundPush.setDelegate(delegate)
        registerSettingsCaches()
        Caches.onTrim { Diagnostics.clearOldExports(this) }

        // "Stay available to paired devices" keeps a light foreground service running,
        // so the phone stays reachable while the app is in the background.
        appScope.launch {
            SoundPush.state
                .map { s -> s != null && s.settings.mobile.stayAvailable && s.trustedPeers.any { !it.blocked } }
                .distinctUntilChanged()
                .collect { delegate.setStayAvailable(it) }
        }
        ListenWidget.observe(appScope, this)
        AudioCues.observe(appScope)

        // Statistics and level meters are only worth decoding and drawing while someone can see
        // them (plan §14.6). Streams, the notification and reconnection are unaffected.
        appScope.launch {
            combine(appVisible, DeviceStatus.screenOn) { visible, screenOn -> visible && screenOn }
                .distinctUntilChanged()
                .collect { live -> SoundPush.setUiLive(live) }
        }

        // "Auto" quality follows the power state (plan §14.6): Opus on battery, uncompressed on a
        // charger over a link with room for it. The engine keeps its own bitrate adaptation.
        appScope.launch {
            combine(DeviceStatus.onPower, SoundPush.state, DeviceStatus.network, ::preferLossless)
                .distinctUntilChanged()
                .collect { prefer -> SoundPush.command { setPreferLossless(prefer) } }
        }

        // Android drops multicast (mDNS) packets unless a lock is held. Hold it only while the app
        // is visible so device discovery works without costing battery in the background.
        val multicast = getSystemService(WifiManager::class.java)
            ?.createMulticastLock("SoundPush:discovery")
            ?.apply { setReferenceCounted(false) }

        // Tell the engine when the app is visible (battery policy, background prompts).
        ProcessLifecycleOwner.get().lifecycle.addObserver(object : DefaultLifecycleObserver {
            override fun onStart(owner: LifecycleOwner) {
                appVisible.value = true
                runCatching { multicast?.acquire() }
                DeviceStatus.refreshNetwork()
                SoundPush.command { setForeground(true) }
                SoundPush.command { networkChanged() }
                // A foreground start the OS refused while in the background succeeds from here.
                delegate.repush()
            }

            override fun onStop(owner: LifecycleOwner) {
                appVisible.value = false
                runCatching { multicast?.release() }
                SoundPush.command { setForeground(false) }
            }
        })
    }

    /**
     * Android is short of memory (plan §8.4): drop what the app can rebuild. Streams keep running —
     * nothing an active route needs is registered with [Caches].
     */
    override fun onTrimMemory(level: Int) {
        super.onTrimMemory(level)
        if (level >= ComponentCallbacks2.TRIM_MEMORY_BACKGROUND) Caches.trim()
    }
}

object BuildVersion {
    const val NAME = "0.1.0"
}
