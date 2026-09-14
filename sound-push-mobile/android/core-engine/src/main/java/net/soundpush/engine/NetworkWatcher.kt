package net.soundpush.engine

import android.content.Context
import android.net.ConnectivityManager
import android.net.LinkProperties
import android.net.Network
import android.os.Handler
import android.os.Looper

/**
 * Tells the engine when the default network changes (Wi-Fi comes back, Wi-Fi ↔ mobile data, a new
 * address on the same Wi-Fi), so sessions migrate or re-dial at once instead of waiting for a
 * timeout.
 *
 * Process-level and tied to the engine, not to the streaming service: the engine keeps paired
 * devices connected while the app is open without a stream. The callback is only a registration
 * in the system's connectivity service; it never starts or wakes the process and ends with it.
 */
internal object NetworkWatcher {
    private const val DEBOUNCE_MS = 300L

    private val handler = Handler(Looper.getMainLooper())
    private val filter = NetworkChangeFilter()
    private var registered = false

    @Synchronized
    fun start(context: Context, onChange: () -> Unit) {
        if (registered) return
        val cm = context.applicationContext.getSystemService(ConnectivityManager::class.java) ?: return
        val notify = Runnable(onChange)
        val callback = object : ConnectivityManager.NetworkCallback() {
            // Reported for every new default network, and again when its addresses change.
            override fun onLinkPropertiesChanged(network: Network, linkProperties: LinkProperties) =
                changed(filter.onDefault(network.toString(), linkProperties.linkAddresses.map { it.toString() }.sorted()))

            override fun onLost(network: Network) = changed(filter.onLost(network.toString()))

            private fun changed(isChange: Boolean) {
                if (!isChange) return
                // A handover reports several callbacks in a burst: tell the engine once.
                handler.removeCallbacks(notify)
                handler.postDelayed(notify, DEBOUNCE_MS)
            }
        }
        // Callbacks arrive on the main looper, where the filter's state is only ever touched.
        registered = runCatching { cm.registerDefaultNetworkCallback(callback, handler) }.isSuccess
    }
}

/**
 * Decides which default-network callbacks are real changes. The first report after registering
 * describes the network the engine already uses, so it is not a change.
 */
internal class NetworkChangeFilter {
    private var current: Pair<String, List<String>>? = null
    private var started = false

    /** The default network is now [network] with [addresses]. True when that differs from before. */
    fun onDefault(network: String, addresses: List<String>): Boolean {
        val next = network to addresses
        val changed = started && next != current
        started = true
        current = next
        return changed
    }

    /** [network] went away. True when it was the default network. */
    fun onLost(network: String): Boolean {
        if (current?.first != network) return false
        current = null
        return true
    }
}
