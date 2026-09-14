package net.soundpush.service

import android.app.Service
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.ServiceInfo
import android.media.AudioAttributes
import android.media.AudioFocusRequest
import android.media.AudioManager
import android.net.ConnectivityManager
import android.net.Network
import android.net.wifi.WifiManager
import android.os.Build
import android.os.IBinder
import android.os.PowerManager
import androidx.core.app.ServiceCompat
import androidx.core.content.ContextCompat
import androidx.core.content.IntentCompat
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.ProcessLifecycleOwner
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.launch
import net.soundpush.engine.EngineState
import net.soundpush.engine.MicSettings
import net.soundpush.engine.PlatformDelegate
import net.soundpush.engine.SoundPush

/**
 * Keeps the process alive while streaming, or while "Stay available" is on. Its
 * foreground-service types follow what is active (playback, microphone, app-audio
 * capture, or connected-device when only staying available). Stops itself shortly
 * after nothing needs it.
 */
class StreamingService : Service() {

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main)
    private var types = KeepAliveTypes()
    private var wifiLock: WifiManager.WifiLock? = null
    private var wakeLock: PowerManager.WakeLock? = null
    private var focusRequest: AudioFocusRequest? = null
    private var focusMode: String? = null
    private var mutedByFocus = false
    private var capture: AppAudioCapture? = null
        set(value) {
            field = value
            _capturing.value = value != null
        }
    private var mic: MicCapture? = null
    private var micSettingsInUse: MicSettings? = null
    private var stopJob: Job? = null
    private var consentAskedFor: String? = null

    private val streaming get() = types.playback || types.microphone || types.appAudio

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        Notifications.ensureChannels(this)
        ContextCompat.registerReceiver(
            this,
            noisyReceiver,
            IntentFilter(AudioManager.ACTION_AUDIO_BECOMING_NOISY),
            ContextCompat.RECEIVER_NOT_EXPORTED,
        )
        getSystemService(ConnectivityManager::class.java)?.registerDefaultNetworkCallback(networkCallback)
        scope.launch {
            SoundPush.state.collectLatest { state ->
                if (state == null) return@collectLatest
                updateNotification()
                restartMicIfSettingsChanged(state.settings.mic)
                if (types.playback) updateAudioFocus()
                syncAppAudioCapture(state)
            }
        }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            Notifications.ACTION_STOP_ALL -> SoundPush.state.value?.routes?.forEach { r ->
                SoundPush.command { stopRoute(r.routeId) }
            }
            Notifications.ACTION_TOGGLE_MUTE -> {
                // Follow the routes' own mute state, the same one the app's Mute button uses, so the
                // notification never fights a mute set from the app or from the computer.
                val micRoutes = SoundPush.state.value?.routes?.filter { it.isMic }.orEmpty()
                val muted = micRoutes.isNotEmpty() && micRoutes.all { it.muted }
                micRoutes.forEach { r -> SoundPush.command { setRouteMuted(r.routeId, !muted) } }
            }
            ACTION_UPDATE -> {
                types = KeepAliveTypes(
                    playback = intent.getBooleanExtra(EXTRA_PLAYBACK, false),
                    microphone = intent.getBooleanExtra(EXTRA_MIC, false),
                    // App-audio capture needs the user's screen-capture consent, so the recorder is the
                    // truth here, not the engine's wish: without a recorder there is nothing to keep alive.
                    appAudio = capture != null,
                    stayAvailable = intent.getBooleanExtra(EXTRA_STAY, false),
                )
            }
            ACTION_START_APP_AUDIO -> {
                // Android 14 requires the mediaProjection foreground type before the projection is created.
                types = types.copy(appAudio = true)
                enterForeground()
                val data = IntentCompat.getParcelableExtra(intent, EXTRA_PROJECTION_DATA, Intent::class.java)
                val code = intent.getIntExtra(EXTRA_PROJECTION_CODE, 0)
                if (data != null && capture == null) {
                    capture = AppAudioCapture.start(this, code, data)
                }
                if (capture == null) types = types.copy(appAudio = false)
            }
        }
        enterForeground()
        return START_NOT_STICKY
    }

    /**
     * The recorder follows the app-audio routes: it stops when the last one ends, and a route that
     * opened without the app on screen (a remembered permission, resume on start) has no recorder
     * until the user grants screen capture. A visible activity asks for that itself; otherwise ask
     * the user to open the app, once per route.
     */
    private fun syncAppAudioCapture(state: EngineState) {
        val routes = state.routes.filter { it.kind == "sendAppAudio" }
        if (routes.isEmpty() && capture != null && SoundPush.direct { appAudioActive() } != true) {
            capture?.stop()
            capture = null
            types = types.copy(appAudio = false)
            enterForeground()
        }
        val first = routes.firstOrNull()
        when {
            first == null -> consentAskedFor = null
            capture == null && consentAskedFor != first.routeId && !appVisible() -> {
                consentAskedFor = first.routeId
                Notifications.attention(this, first.peerName)
            }
        }
    }

    private fun appVisible() = ProcessLifecycleOwner.get().lifecycle.currentState.isAtLeast(Lifecycle.State.STARTED)

    private fun enterForeground() {
        if (!streaming && !types.stayAvailable) {
            updateMicCapture()
            releaseLocks()
            abandonFocus()
            scheduleStop()
            return
        }
        stopJob?.cancel()
        var serviceTypes = 0
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            if (types.playback) serviceTypes = serviceTypes or ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK
            if (types.appAudio) serviceTypes = serviceTypes or ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PROJECTION
            if (types.microphone && Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
                serviceTypes = serviceTypes or ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE
            }
            if (serviceTypes == 0) {
                serviceTypes = if (types.stayAvailable) {
                    ServiceInfo.FOREGROUND_SERVICE_TYPE_CONNECTED_DEVICE
                } else {
                    ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK
                }
            }
        }
        try {
            ServiceCompat.startForeground(
                this,
                Notifications.ID_STREAMING,
                Notifications.streaming(this, SoundPush.state.value, types.microphone),
                serviceTypes,
            )
        } catch (e: Exception) {
            // Background start restrictions (Android 12+ / while-in-use mic): ask the user to open the
            // app. ServiceDelegate re-sends the request when the app comes to the front.
            if (streaming) {
                Notifications.attention(this, SoundPush.state.value?.connectedPeers?.firstOrNull()?.name ?: "SoundPush")
            }
            stopSelf()
            return
        }
        // Wake and Wi-Fi locks cost battery: hold them only while audio is actually flowing.
        if (streaming) acquireLocks() else releaseLocks()
        updateAudioFocus()
        updateMicCapture()
    }

    /** Run the microphone recorder exactly while the engine needs mic input. */
    private fun updateMicCapture() {
        val wanted = types.microphone
        if (wanted && mic == null) {
            val settings = SoundPush.state.value?.settings?.mic ?: return
            mic = MicCapture.start(settings)
            micSettingsInUse = settings
        } else if (!wanted && mic != null) {
            mic?.stop()
            mic = null
            micSettingsInUse = null
        }
    }

    /** The recorder's source and platform effects are fixed when it opens: reopen it when they change mid-stream. */
    private fun restartMicIfSettingsChanged(settings: MicSettings) {
        val inUse = micSettingsInUse ?: return
        if (mic == null || MicCapture.recorderSettings(inUse) == MicCapture.recorderSettings(settings)) return
        mic?.stop()
        mic = MicCapture.start(settings)
        micSettingsInUse = settings
    }

    private fun updateNotification() {
        if (!streaming && !types.stayAvailable) return
        val nm = getSystemService(android.app.NotificationManager::class.java)
        runCatching {
            nm.notify(Notifications.ID_STREAMING, Notifications.streaming(this, SoundPush.state.value, types.microphone))
        }
    }

    private fun scheduleStop() {
        stopJob?.cancel()
        stopJob = scope.launch {
            delay(5_000)
            stopSelf()
        }
    }

    private fun acquireLocks() {
        if (wifiLock == null) {
            val wifi = applicationContext.getSystemService(WifiManager::class.java)
            val mode = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                WifiManager.WIFI_MODE_FULL_LOW_LATENCY
            } else {
                @Suppress("DEPRECATION")
                WifiManager.WIFI_MODE_FULL_HIGH_PERF
            }
            wifiLock = wifi?.createWifiLock(mode, "SoundPush:stream")?.apply {
                setReferenceCounted(false)
                acquire()
            }
        }
        if (wakeLock == null) {
            wakeLock = getSystemService(PowerManager::class.java)
                ?.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "SoundPush:stream")
                ?.apply {
                    setReferenceCounted(false)
                    acquire(12 * 60 * 60 * 1000L)
                }
        }
    }

    private fun releaseLocks() {
        wifiLock?.let { if (it.isHeld) it.release() }
        wifiLock = null
        wakeLock?.let { if (it.isHeld) it.release() }
        wakeLock = null
    }

    // ------------------------------------------------------------------ audio focus

    /** Holds audio focus while playing, in the mode the user chose; re-requests when the mode changes. */
    private fun updateAudioFocus() {
        val mode = SoundPush.state.value?.settings?.output?.audioFocus ?: "pause"
        if (!types.playback || mode == "mix" || mode == "mixDuringCalls") {
            abandonFocus()
            return
        }
        if (focusRequest != null && focusMode == mode) return
        abandonFocus()
        val am = getSystemService(AudioManager::class.java) ?: return
        val request = AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN)
            .setAudioAttributes(
                AudioAttributes.Builder()
                    .setUsage(AudioAttributes.USAGE_MEDIA)
                    .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC)
                    .build(),
            )
            .setWillPauseWhenDucked(mode == "pause")
            .setOnAudioFocusChangeListener { change -> onFocusChange(change) }
            .build()
        am.requestAudioFocus(request)
        focusRequest = request
        focusMode = mode
    }

    private fun onFocusChange(change: Int) {
        val mode = SoundPush.state.value?.settings?.output?.audioFocus ?: "pause"
        val receiving = SoundPush.state.value?.routes?.filter { !it.isSending }.orEmpty()
        when (change) {
            AudioManager.AUDIOFOCUS_LOSS, AudioManager.AUDIOFOCUS_LOSS_TRANSIENT -> {
                mutedByFocus = true
                receiving.forEach { r -> SoundPush.command { setRouteMuted(r.routeId, true) } }
            }
            AudioManager.AUDIOFOCUS_LOSS_TRANSIENT_CAN_DUCK -> {
                if (mode == "duck") {
                    receiving.forEach { r -> SoundPush.command { setRouteVolume(r.routeId, 0.3f) } }
                } else {
                    mutedByFocus = true
                    receiving.forEach { r -> SoundPush.command { setRouteMuted(r.routeId, true) } }
                }
            }
            AudioManager.AUDIOFOCUS_GAIN -> {
                // Auto-resume after the interruption ends.
                receiving.forEach { r ->
                    SoundPush.command { setRouteVolume(r.routeId, 1f) }
                    if (mutedByFocus) SoundPush.command { setRouteMuted(r.routeId, false) }
                }
                mutedByFocus = false
            }
        }
    }

    private fun abandonFocus() {
        val request = focusRequest ?: return
        getSystemService(AudioManager::class.java)?.abandonAudioFocusRequest(request)
        focusRequest = null
        focusMode = null
    }

    private val noisyReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) {
            if (SoundPush.state.value?.settings?.output?.pauseOnHeadsetDisconnect != false) {
                SoundPush.state.value?.routes?.filter { !it.isSending }?.forEach { r ->
                    SoundPush.command { setRouteMuted(r.routeId, true) }
                }
            }
        }
    }

    private val networkCallback = object : ConnectivityManager.NetworkCallback() {
        override fun onAvailable(network: Network) {
            SoundPush.command { networkChanged() }
        }
    }

    override fun onDestroy() {
        runCatching { unregisterReceiver(noisyReceiver) }
        runCatching { getSystemService(ConnectivityManager::class.java)?.unregisterNetworkCallback(networkCallback) }
        capture?.stop()
        capture = null
        mic?.stop()
        abandonFocus()
        releaseLocks()
        scope.cancel()
        super.onDestroy()
    }

    data class KeepAliveTypes(
        val playback: Boolean = false,
        val microphone: Boolean = false,
        val appAudio: Boolean = false,
        val stayAvailable: Boolean = false,
    )

    companion object {
        const val ACTION_UPDATE = "net.soundpush.UPDATE"
        const val ACTION_START_APP_AUDIO = "net.soundpush.START_APP_AUDIO"
        const val EXTRA_PLAYBACK = "playback"
        const val EXTRA_MIC = "mic"
        const val EXTRA_APP_AUDIO = "appAudio"
        const val EXTRA_STAY = "stayAvailable"
        const val EXTRA_PROJECTION_CODE = "projectionCode"
        const val EXTRA_PROJECTION_DATA = "projectionData"

        private val _capturing = MutableStateFlow(false)

        /** True while the app-audio recorder runs, i.e. screen-capture consent was given and is in use. */
        val capturing: StateFlow<Boolean> = _capturing

        /** Begin app-audio capture with a MediaProjection consent result (call from a visible activity). */
        fun startAppAudio(context: Context, resultCode: Int, data: Intent) {
            val intent = Intent(context, StreamingService::class.java)
                .setAction(ACTION_START_APP_AUDIO)
                .putExtra(EXTRA_PROJECTION_CODE, resultCode)
                .putExtra(EXTRA_PROJECTION_DATA, data)
            ContextCompat.startForegroundService(context, intent)
        }
    }
}

/** Connects the engine's keep-alive requests and "Stay available" to the foreground service. */
class ServiceDelegate(private val context: Context) : PlatformDelegate {
    private var playback = false
    private var microphone = false
    private var appAudio = false
    private var stayAvailable = false
    private var serviceWanted = false

    override fun keepAlive(playback: Boolean, microphone: Boolean, appAudio: Boolean) {
        synchronized(this) {
            this.playback = playback
            this.microphone = microphone
            this.appAudio = appAudio
        }
        push()
    }

    fun setStayAvailable(enabled: Boolean) {
        synchronized(this) { stayAvailable = enabled }
        push()
    }

    /**
     * Re-send the current request. Called when the app comes to the front: a foreground start
     * refused while in the background (Android 12+ microphone rules) succeeds from here.
     */
    fun repush() = push()

    private fun push() {
        val intent: Intent
        val streaming: Boolean
        val wanted: Boolean
        synchronized(this) {
            streaming = playback || microphone || appAudio
            wanted = streaming || stayAvailable
            // Nothing running and nothing wanted: don't start the service just to stop it.
            if (!wanted && !serviceWanted) return
            serviceWanted = wanted
            intent = Intent(context, StreamingService::class.java)
                .setAction(StreamingService.ACTION_UPDATE)
                .putExtra(StreamingService.EXTRA_PLAYBACK, playback)
                .putExtra(StreamingService.EXTRA_MIC, microphone)
                .putExtra(StreamingService.EXTRA_APP_AUDIO, appAudio)
                .putExtra(StreamingService.EXTRA_STAY, stayAvailable)
        }
        try {
            if (wanted) ContextCompat.startForegroundService(context, intent) else context.startService(intent)
        } catch (e: IllegalStateException) {
            // Started from the background where the OS forbids it: surface a notification instead.
            if (streaming) Notifications.attention(context, "SoundPush")
        } catch (e: SecurityException) {
            if (streaming) Notifications.attention(context, "SoundPush")
        }
    }

    override fun attentionNeeded(key: String, peerName: String) {
        Notifications.attention(context, peerName)
    }
}
