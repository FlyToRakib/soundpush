package net.soundpush.engine

import android.annotation.SuppressLint
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.media.AudioAttributes
import android.media.AudioDeviceCallback
import android.media.AudioDeviceInfo
import android.media.AudioManager
import android.media.AudioRecordingConfiguration
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import android.os.BatteryManager
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.PowerManager
import androidx.core.content.ContextCompat
import java.net.Inet4Address
import java.net.NetworkInterface
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.launch

/**
 * What the phone's audio output, network, screen and power look like right now, for hints
 * (Bluetooth delay, mobile data through USB tethering), the troubleshooter, the battery policy
 * (plan §14.6) and the live "microphone in use" notice (plan §8.3). Event-driven: system
 * callbacks, no polling. Started lazily by the first screen or service that needs it.
 */
object DeviceStatus {
    enum class Output { Speaker, Wired, Bluetooth, Usb, Other }

    data class NetworkInfo(
        val wifi: Boolean = false,
        val ethernet: Boolean = false,
        val cellular: Boolean = false,
        val vpn: Boolean = false,
        /** This phone shares its connection over USB (an rndis/ncm interface is up). */
        val usbTethering: Boolean = false,
        /** This phone runs a Wi-Fi hotspot. */
        val hotspot: Boolean = false,
        /** IPv4 subnets of the tethering interfaces, as (address bytes, prefix length). */
        val tetherSubnets: List<Pair<ByteArray, Int>> = emptyList(),
    ) {
        /** A computer on this phone's USB tethering or hotspot gets its internet from mobile data. */
        val sharesMobileData get() = (usbTethering || hotspot) && cellular && !wifi && !ethernet

        /** Nothing local to stream over: only mobile data is up. */
        val mobileDataOnly get() = cellular && !wifi && !ethernet && !usbTethering && !hotspot

        /** Whether [address] ("ip" or "ip:port") is on this phone's USB tethering or hotspot link. */
        fun isTetherAddress(address: String): Boolean {
            val host = address.substringBefore(':')
            // Literal IPv4 only: never trigger a DNS lookup from UI code.
            val bytes = host.split('.').takeIf { it.size == 4 }?.map { it.toIntOrNull()?.takeIf { v -> v in 0..255 } ?: return false }
                ?.map { it.toByte() }?.toByteArray() ?: return false
            return tetherSubnets.any { (net, prefix) -> sameSubnet(bytes, net, prefix) }
        }
    }

    private val _output = MutableStateFlow(Output.Speaker)
    private val _network = MutableStateFlow(NetworkInfo())
    private val _outputLatencyMs = MutableStateFlow(0)
    private val _screenOn = MutableStateFlow(true)
    private val _onPower = MutableStateFlow(false)
    private val _micSilenced = MutableStateFlow(false)
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    /** Always the application context (set in [start]), which lives as long as the process: no leak. */
    @SuppressLint("StaticFieldLeak")
    @Volatile private var app: Context? = null

    /** Where media audio plays now. */
    val output: StateFlow<Output> = _output
    val network: StateFlow<NetworkInfo> = _network
    /** Output latency measured by the platform player (compatibility output); 0 when unknown. */
    val outputLatencyMs: StateFlow<Int> = _outputLatencyMs
    /** Whether the screen is on. True until [start] has run, so nothing is throttled by mistake. */
    val screenOn: StateFlow<Boolean> = _screenOn
    /** Whether the phone is charging (a charger, or the computer's USB port). */
    val onPower: StateFlow<Boolean> = _onPower
    /**
     * Whether Android is giving this app silence because another app holds the microphone
     * (a call, a voice assistant). Android 10+ only; always false below that, where capture is
     * exclusive and the recorder simply fails instead.
     */
    val micSilenced: StateFlow<Boolean> = _micSilenced

    fun reportOutputLatency(ms: Int) {
        _outputLatencyMs.value = ms
    }

    /** Safe to call many times; only the first call registers the callbacks. */
    @Synchronized
    fun start(context: Context) {
        if (app != null) return
        val ctx = context.applicationContext
        app = ctx
        val main = Handler(Looper.getMainLooper())
        ctx.getSystemService(AudioManager::class.java)?.let { am ->
            am.registerAudioDeviceCallback(object : AudioDeviceCallback() {
                override fun onAudioDevicesAdded(addedDevices: Array<out AudioDeviceInfo>) = refreshOutput(am)
                override fun onAudioDevicesRemoved(removedDevices: Array<out AudioDeviceInfo>) = refreshOutput(am)
            }, main)
            refreshOutput(am)
            watchRecording(am, main)
        }
        watchScreen(ctx)
        watchPower(ctx)
        ctx.getSystemService(ConnectivityManager::class.java)?.registerDefaultNetworkCallback(object : ConnectivityManager.NetworkCallback() {
            override fun onCapabilitiesChanged(network: Network, networkCapabilities: NetworkCapabilities) = refreshNetwork()
            override fun onLost(network: Network) = refreshNetwork()
        })
        // Tethering does not change the default network, so listen for it directly (system broadcast).
        ContextCompat.registerReceiver(
            ctx,
            object : BroadcastReceiver() {
                override fun onReceive(context: Context, intent: Intent) = refreshNetwork()
            },
            IntentFilter(ACTION_TETHER_STATE_CHANGED),
            ContextCompat.RECEIVER_NOT_EXPORTED,
        )
        refreshNetwork()
    }

    /**
     * Android 10+ hands a silenced recording to an app whose microphone another app has taken
     * (plan §8.3). The system reports the app's own recordings only, which is exactly what is
     * needed: while SoundPush records, `isClientSilenced` says whether anything is getting through.
     */
    private fun watchRecording(am: AudioManager, main: Handler) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) return
        am.registerAudioRecordingCallback(
            object : AudioManager.AudioRecordingCallback() {
                override fun onRecordingConfigChanged(configs: MutableList<AudioRecordingConfiguration>) {
                    _micSilenced.value = configs.any { it.isClientSilenced }
                }
            },
            main,
        )
        _micSilenced.value = am.activeRecordingConfigurations.any { it.isClientSilenced }
    }

    /** Screen on or off, for the battery policy (plan §14.6). The broadcasts cannot be declared. */
    private fun watchScreen(ctx: Context) {
        _screenOn.value = ctx.getSystemService(PowerManager::class.java)?.isInteractive != false
        ContextCompat.registerReceiver(
            ctx,
            object : BroadcastReceiver() {
                override fun onReceive(context: Context, intent: Intent) {
                    _screenOn.value = intent.action == Intent.ACTION_SCREEN_ON
                }
            },
            IntentFilter().apply {
                addAction(Intent.ACTION_SCREEN_ON)
                addAction(Intent.ACTION_SCREEN_OFF)
            },
            ContextCompat.RECEIVER_NOT_EXPORTED,
        )
    }

    /**
     * On a charger or on battery. The connect and disconnect broadcasts wake the app twice a day
     * at most; `ACTION_BATTERY_CHANGED` would wake it every time the level moves.
     */
    private fun watchPower(ctx: Context) {
        _onPower.value = ctx.getSystemService(BatteryManager::class.java)?.isCharging == true
        ContextCompat.registerReceiver(
            ctx,
            object : BroadcastReceiver() {
                override fun onReceive(context: Context, intent: Intent) {
                    _onPower.value = intent.action == Intent.ACTION_POWER_CONNECTED
                }
            },
            IntentFilter().apply {
                addAction(Intent.ACTION_POWER_CONNECTED)
                addAction(Intent.ACTION_POWER_DISCONNECTED)
            },
            ContextCompat.RECEIVER_NOT_EXPORTED,
        )
    }

    private fun refreshOutput(am: AudioManager) {
        val devices = if (Build.VERSION.SDK_INT >= 33) {
            runCatching {
                am.getAudioDevicesForAttributes(AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_MEDIA).build())
                    .map { it.type }
            }.getOrNull()
        } else {
            null
        } ?: am.getDevices(AudioManager.GET_DEVICES_OUTPUTS).map { it.type }
        _output.value = classify(devices)
    }

    /** Android sends media to Bluetooth first, then USB and wired headsets, then the speaker. */
    internal fun classify(types: List<Int>): Output = when {
        types.any { it in BLUETOOTH_TYPES } -> Output.Bluetooth
        types.any { it == AudioDeviceInfo.TYPE_USB_HEADSET || it == AudioDeviceInfo.TYPE_USB_DEVICE } -> Output.Usb
        types.any { it == AudioDeviceInfo.TYPE_WIRED_HEADPHONES || it == AudioDeviceInfo.TYPE_WIRED_HEADSET } -> Output.Wired
        types.isEmpty() || types.any { it == AudioDeviceInfo.TYPE_BUILTIN_SPEAKER } -> Output.Speaker
        else -> Output.Other
    }

    /** Re-read the network state off the main thread (interface enumeration can block). */
    fun refreshNetwork() {
        val ctx = app ?: return
        scope.launch {
            val cm = ctx.getSystemService(ConnectivityManager::class.java)
            val caps = cm?.activeNetwork?.let { cm.getNetworkCapabilities(it) }
            val subnets = mutableListOf<Pair<ByteArray, Int>>()
            var usb = false
            var hotspot = false
            runCatching {
                NetworkInterface.getNetworkInterfaces()?.toList().orEmpty().filter { it.isUp && !it.isLoopback }.forEach { nif ->
                    val name = nif.name.lowercase()
                    val isUsb = USB_TETHER_PREFIXES.any { name.startsWith(it) }
                    val isHotspot = HOTSPOT_PREFIXES.any { name.startsWith(it) }
                    if (!isUsb && !isHotspot) return@forEach
                    val v4 = nif.interfaceAddresses.filter { it.address is Inet4Address }
                    if (v4.isEmpty()) return@forEach
                    if (isUsb) usb = true else hotspot = true
                    v4.forEach { subnets += it.address.address to it.networkPrefixLength.toInt() }
                }
            }
            _network.value = NetworkInfo(
                wifi = caps?.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) == true,
                ethernet = caps?.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET) == true,
                cellular = caps?.hasTransport(NetworkCapabilities.TRANSPORT_CELLULAR) == true,
                vpn = caps?.hasTransport(NetworkCapabilities.TRANSPORT_VPN) == true,
                usbTethering = usb,
                hotspot = hotspot,
                tetherSubnets = subnets,
            )
        }
    }

    internal fun sameSubnet(a: ByteArray, b: ByteArray, prefix: Int): Boolean {
        if (a.size != b.size) return false
        var bits = prefix.coerceIn(0, a.size * 8)
        for (i in a.indices) {
            if (bits <= 0) return true
            val mask = if (bits >= 8) 0xFF else (0xFF shl (8 - bits)) and 0xFF
            if ((a[i].toInt() and mask) != (b[i].toInt() and mask)) return false
            bits -= 8
        }
        return true
    }

    private const val ACTION_TETHER_STATE_CHANGED = "android.net.conn.TETHER_STATE_CHANGED"

    /** rndis (older phones), ncm (Android 11+ USB tethering), usb (some OEM kernels). */
    private val USB_TETHER_PREFIXES = listOf("rndis", "ncm", "usb")
    private val HOTSPOT_PREFIXES = listOf("ap", "swlan", "softap")

    // A2DP, hearing aids (28+), LE Audio headsets/speakers/broadcast (31+/33+). Literal values keep
    // older API levels compiling; the system only reports types it knows.
    private val BLUETOOTH_TYPES = setOf(
        AudioDeviceInfo.TYPE_BLUETOOTH_A2DP,
        AudioDeviceInfo.TYPE_BLUETOOTH_SCO,
        23, // TYPE_HEARING_AID
        26, // TYPE_BLE_HEADSET
        27, // TYPE_BLE_SPEAKER
        30, // TYPE_BLE_BROADCAST
    )
}
