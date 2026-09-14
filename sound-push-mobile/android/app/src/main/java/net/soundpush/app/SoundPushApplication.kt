package net.soundpush.app

import android.app.Application
import android.net.wifi.WifiManager
import androidx.lifecycle.DefaultLifecycleObserver
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.ProcessLifecycleOwner
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.launch
import net.soundpush.engine.SoundPush
import net.soundpush.service.Notifications
import net.soundpush.service.ServiceDelegate

class SoundPushApplication : Application() {
    private val appScope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)

    override fun onCreate() {
        super.onCreate()
        Notifications.ensureChannels(this)
        val delegate = ServiceDelegate(this)
        SoundPush.setDelegate(delegate)
        // Starts on a background thread; failures are shown on screen via SoundPush.startError.
        SoundPush.start(this, BuildVersion.NAME)

        // "Stay available to paired devices" keeps a light foreground service running,
        // so the phone stays reachable while the app is in the background.
        appScope.launch {
            SoundPush.state
                .map { s -> s != null && s.settings.mobile.stayAvailable && s.trustedPeers.any { !it.blocked } }
                .distinctUntilChanged()
                .collect { delegate.setStayAvailable(it) }
        }

        // Android drops multicast (mDNS) packets unless a lock is held. Hold it only while the app
        // is visible so device discovery works without costing battery in the background.
        val multicast = getSystemService(WifiManager::class.java)
            ?.createMulticastLock("SoundPush:discovery")
            ?.apply { setReferenceCounted(false) }

        // Tell the engine when the app is visible (battery policy, background prompts).
        ProcessLifecycleOwner.get().lifecycle.addObserver(object : DefaultLifecycleObserver {
            override fun onStart(owner: LifecycleOwner) {
                runCatching { multicast?.acquire() }
                SoundPush.command { setForeground(true) }
                SoundPush.command { networkChanged() }
                // A foreground start the OS refused while in the background succeeds from here.
                delegate.repush()
            }

            override fun onStop(owner: LifecycleOwner) {
                runCatching { multicast?.release() }
                SoundPush.command { setForeground(false) }
            }
        })
    }
}

object BuildVersion {
    const val NAME = "0.1.0"
}
