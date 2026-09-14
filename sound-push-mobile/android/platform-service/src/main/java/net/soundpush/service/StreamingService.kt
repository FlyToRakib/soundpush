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
import android.net.wifi.WifiManager
import android.os.Build
import android.os.Bundle
import android.os.IBinder
import android.os.PowerManager
import android.support.v4.media.MediaMetadataCompat
import android.support.v4.media.session.MediaSessionCompat
import android.support.v4.media.session.PlaybackStateCompat
import androidx.annotation.RequiresApi
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
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.mapNotNull
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeoutOrNull
import net.soundpush.engine.DeviceStatus
import net.soundpush.engine.EngineState
import net.soundpush.engine.MicSettings
import net.soundpush.engine.PlatformDelegate
import net.soundpush.engine.SoundPush
import net.soundpush.ui.R
import net.soundpush.ui.components.Labels
import net.soundpush.service.R as ServiceR

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
    private var duckedByFocus = false
    private var capture: AppAudioCapture? = null
        set(value) {
            field = value
            _capturing.value = value != null
        }
    private var mic: MicCapture? = null
    private var micSettingsInUse: MicSettings? = null
    private var stopJob: Job? = null
    private var listenJob: Job? = null
    private var consentAskedFor: String? = null
    private var playback: PlatformPlayback? = null
    private var playbackLegacy = false
    private var session: MediaSessionCompat? = null
    private var sessionKey: Any? = null
    private var modeListener: Any? = null
    private var mutedByCall = false

    private val streaming get() = types.playback || types.microphone || types.appAudio

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        Notifications.ensureChannels(this)
        DeviceStatus.start(this)
        ContextCompat.registerReceiver(
            this,
            noisyReceiver,
            IntentFilter(AudioManager.ACTION_AUDIO_BECOMING_NOISY),
            ContextCompat.RECEIVER_NOT_EXPORTED,
        )
        // Network changes reach the engine through its own process-level watcher (NetworkWatcher).
        scope.launch {
            SoundPush.state.collectLatest { state ->
                if (state == null) return@collectLatest
                updateMediaSession(state)
                updateNotification()
                restartMicIfSettingsChanged(state)
                if (types.playback) updateAudioFocus()
                updateCallWatch()
                syncPlatformPlayback(state)
                syncAppAudioCapture(state)
            }
        }
        // The notification names the current output (speaker, headphones, Bluetooth).
        scope.launch { DeviceStatus.output.collect { updateNotification() } }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            Notifications.ACTION_STOP_ALL -> stopAll()
            Notifications.ACTION_TOGGLE_MUTE -> {
                // Follow the routes' own mute state, the same one the app's Mute button uses, so the
                // notification never fights a mute set from the app or from the computer.
                val micRoutes = SoundPush.state.value?.routes?.filter { it.isMic }.orEmpty()
                val muted = micRoutes.isNotEmpty() && micRoutes.all { it.muted }
                micRoutes.forEach { r -> SoundPush.command { setRouteMuted(r.routeId, !muted) } }
            }
            Notifications.ACTION_TOGGLE_PLAYBACK_MUTE -> {
                val receiving = receivingRoutes(SoundPush.state.value)
                setPlaybackMuted(!(receiving.isNotEmpty() && receiving.all { it.muted }))
            }
            ACTION_TOGGLE_LISTEN -> toggleListen()
            ACTION_UPDATE -> {
                types = KeepAliveTypes(
                    playback = intent.getBooleanExtra(EXTRA_PLAYBACK, false),
                    microphone = intent.getBooleanExtra(EXTRA_MIC, false),
                    // App-audio capture needs the user's screen-capture consent, so the recorder is the
                    // truth here, not the engine's wish: without a recorder there is nothing to keep alive.
                    appAudio = capture != null,
                    stayAvailable = intent.getBooleanExtra(EXTRA_STAY, false),
                    listenStarting = types.listenStarting,
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

    private fun receivingRoutes(state: EngineState?) = state?.routes?.filter { !it.isSending && it.status != "stopped" }.orEmpty()

    private fun stopAll() {
        SoundPush.state.value?.routes?.forEach { r -> SoundPush.command { stopRoute(r.routeId) } }
    }

    private fun setPlaybackMuted(muted: Boolean) {
        receivingRoutes(SoundPush.state.value).forEach { r -> SoundPush.command { setRouteMuted(r.routeId, muted) } }
    }

    /**
     * Home-screen widget: stop listening, or start listening to the first connected computer. The
     * widget tap started this service in the foreground, so it can wait for the engine (which may
     * be starting from a cold process) and for a computer to connect, then start the route.
     */
    private fun toggleListen() {
        val listening = SoundPush.state.value?.routes?.filter { it.kind == LISTEN_KIND }.orEmpty()
        if (listening.isNotEmpty()) {
            listening.forEach { r -> SoundPush.command { stopRoute(r.routeId) } }
            return
        }
        if (listenJob?.isActive == true) return
        types = types.copy(listenStarting = true)
        SoundPush.ensureStarted(this)
        listenJob = scope.launch {
            val state = withTimeoutOrNull(LISTEN_WAIT_MS) {
                SoundPush.state.mapNotNull { s -> s?.takeIf { it.connectedPeers.any { p -> p.canSendSystemAudio } } }.first()
            }
            val peer = state?.connectedPeers?.firstOrNull { it.canSendSystemAudio }
            when {
                peer == null -> Notifications.listenUnavailable(this@StreamingService)
                state.routes.none { it.kind == LISTEN_KIND } -> SoundPush.command { startRoute(peer.deviceId, LISTEN_KIND) }
            }
            // The engine's keep-alive takes over once the route opens; the stop grace period covers the gap.
            types = types.copy(listenStarting = false)
            enterForeground()
        }
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

    /**
     * Run the AudioTrack player exactly while a stream plays through the platform path
     * (compatibility output, output audio effects, or the automatic fallback). The native side
     * knows which streams are there; reopen when the user switches between the two player kinds.
     */
    private fun syncPlatformPlayback(state: EngineState?) {
        val wanted = types.playback && SoundPush.direct { platformOutputActive() } == true
        val legacy = state?.settings?.output?.compatibilityOutput == true
        if (wanted && (playback == null || playbackLegacy != legacy)) {
            playback?.stop()
            playback = PlatformPlayback.start(this, legacy)
            playbackLegacy = legacy
        } else if (!wanted && playback != null) {
            playback?.stop()
            playback = null
        }
    }

    private fun appVisible() = ProcessLifecycleOwner.get().lifecycle.currentState.isAtLeast(Lifecycle.State.STARTED)

    private fun enterForeground() {
        if (!streaming && !types.stayAvailable && !types.listenStarting) {
            updateMicCapture()
            releaseLocks()
            abandonFocus()
            updateCallWatch()
            syncPlatformPlayback(SoundPush.state.value)
            updateMediaSession(SoundPush.state.value)
            scheduleStop()
            return
        }
        stopJob?.cancel()
        var serviceTypes = 0
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            if (types.playback || types.listenStarting) serviceTypes = serviceTypes or ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK
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
        updateMediaSession(SoundPush.state.value)
        try {
            ServiceCompat.startForeground(this, Notifications.ID_STREAMING, buildNotification(), serviceTypes)
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
        updateCallWatch()
        updateMicCapture()
        syncPlatformPlayback(SoundPush.state.value)
    }

    private fun buildNotification() =
        Notifications.streaming(
            this,
            SoundPush.state.value,
            types.microphone,
            session?.sessionToken,
            types.listenStarting,
            DeviceStatus.output.value,
        )

    /** Run the microphone recorder exactly while the engine needs mic input. */
    private fun updateMicCapture() {
        val wanted = types.microphone
        if (wanted && mic == null) {
            val settings = recorderMicSettings(SoundPush.state.value) ?: return
            mic = MicCapture.start(settings)
            micSettingsInUse = settings
        } else if (!wanted && mic != null) {
            mic?.stop()
            mic = null
            micSettingsInUse = null
        }
    }

    /** The user's microphone settings, with the voice-call preset and echo cancellation during the headset task. */
    private fun recorderMicSettings(state: EngineState?): MicSettings? =
        state?.let { HeadsetMode.recorderSettings(it.settings.mic, HeadsetMode.active(it.routes)) }

    /**
     * The recorder's source and platform effects are fixed when it opens: reopen it when they change
     * mid-stream, including when the headset task starts or ends (back to the user's own preset).
     */
    private fun restartMicIfSettingsChanged(state: EngineState) {
        val inUse = micSettingsInUse ?: return
        val settings = recorderMicSettings(state) ?: return
        if (mic == null || MicCapture.recorderSettings(inUse) == MicCapture.recorderSettings(settings)) return
        mic?.stop()
        mic = MicCapture.start(settings)
        micSettingsInUse = settings
    }

    private fun updateNotification() {
        if (!streaming && !types.stayAvailable && !types.listenStarting) return
        val nm = getSystemService(android.app.NotificationManager::class.java)
        runCatching { nm.notify(Notifications.ID_STREAMING, buildNotification()) }
    }

    // ------------------------------------------------------------------ media session

    /**
     * While this phone plays audio, a media session gives the notification media controls, lets
     * headset buttons pause (mute) and resume, and shows the system output switcher (Android 11+).
     * Updated only when what it shows changes, not on every stats tick.
     */
    private fun updateMediaSession(state: EngineState?) {
        val receiving = receivingRoutes(state)
        if (!types.playback || receiving.isEmpty()) {
            session?.run {
                isActive = false
                release()
            }
            session = null
            sessionKey = null
            return
        }
        val first = receiving.first()
        val title = getString(Labels.routeTitle(first.kind), first.peerName)
        val muted = receiving.all { it.muted }
        val key = title to muted
        val s = session ?: MediaSessionCompat(this, "SoundPush").also {
            it.setCallback(sessionCallback)
            it.setSessionActivity(Notifications.openAppIntent(this))
            session = it
        }
        if (key == sessionKey) return
        sessionKey = key
        s.setMetadata(
            MediaMetadataCompat.Builder()
                .putString(MediaMetadataCompat.METADATA_KEY_TITLE, title)
                .putString(MediaMetadataCompat.METADATA_KEY_ARTIST, getString(R.string.app_name))
                .build(),
        )
        s.setPlaybackState(
            PlaybackStateCompat.Builder()
                .setActions(
                    PlaybackStateCompat.ACTION_PLAY or PlaybackStateCompat.ACTION_PAUSE or
                        PlaybackStateCompat.ACTION_PLAY_PAUSE or PlaybackStateCompat.ACTION_STOP,
                )
                // Android 13+ media controls show custom actions, not the notification's own buttons.
                .addCustomAction(CUSTOM_ACTION_STOP, getString(R.string.route_stop), ServiceR.drawable.ic_action_stop)
                .setState(
                    if (muted) PlaybackStateCompat.STATE_PAUSED else PlaybackStateCompat.STATE_PLAYING,
                    PlaybackStateCompat.PLAYBACK_POSITION_UNKNOWN,
                    1f,
                )
                .build(),
        )
        s.isActive = true
    }

    private val sessionCallback = object : MediaSessionCompat.Callback() {
        override fun onPlay() = setPlaybackMuted(false)
        override fun onPause() = setPlaybackMuted(true)
        override fun onStop() = stopAll()
        override fun onCustomAction(action: String?, extras: Bundle?) {
            if (action == CUSTOM_ACTION_STOP) stopAll()
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
                setDucked(false)
                mutedByFocus = true
                receiving.forEach { r -> SoundPush.command { setRouteMuted(r.routeId, true) } }
            }
            AudioManager.AUDIOFOCUS_LOSS_TRANSIENT_CAN_DUCK -> {
                if (mode == "duck") {
                    setDucked(true)
                } else {
                    mutedByFocus = true
                    receiving.forEach { r -> SoundPush.command { setRouteMuted(r.routeId, true) } }
                }
            }
            AudioManager.AUDIOFOCUS_GAIN -> {
                // Auto-resume after the interruption ends. Volumes were never touched, so nothing to restore.
                setDucked(false)
                if (mutedByFocus) receiving.forEach { r -> SoundPush.command { setRouteMuted(r.routeId, false) } }
                mutedByFocus = false
            }
        }
    }

    /**
     * "Lower volume" ducks the whole output with a separate gain in the audio backend, so the
     * volume the user set on each stream is never overwritten and survives the interruption.
     */
    private fun setDucked(ducked: Boolean) {
        if (duckedByFocus == ducked) return
        duckedByFocus = ducked
        SoundPush.command { setOutputDuck(if (ducked) DUCK_GAIN else 1f) }
    }

    private fun abandonFocus() {
        setDucked(false)
        val request = focusRequest ?: return
        getSystemService(AudioManager::class.java)?.abandonAudioFocusRequest(request)
        focusRequest = null
        focusMode = null
    }

    // ------------------------------------------------------------------ calls in "Keep playing" mode

    /**
     * "Keep playing" mixes with other apps but still goes quiet during phone and VoIP calls
     * ("Keep playing, even during calls" doesn't). Without audio focus the call is noticed through
     * the audio mode, which Android reports to listeners from 12 on; older versions keep playing.
     */
    private fun updateCallWatch() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.S) return
        val wanted = types.playback && SoundPush.state.value?.settings?.output?.audioFocus == "mix"
        if (wanted && modeListener == null) {
            modeListener = addModeListener()
        } else if (!wanted && modeListener != null) {
            removeModeListener(modeListener)
            modeListener = null
            if (mutedByCall) setPlaybackMuted(false)
            mutedByCall = false
        }
    }

    @RequiresApi(Build.VERSION_CODES.S)
    private fun addModeListener(): Any? {
        val am = getSystemService(AudioManager::class.java) ?: return null
        val listener = AudioManager.OnModeChangedListener { mode ->
            val inCall = mode == AudioManager.MODE_IN_CALL || mode == AudioManager.MODE_IN_COMMUNICATION ||
                mode == AudioManager.MODE_RINGTONE || mode == AudioManager.MODE_CALL_SCREENING
            scope.launch {
                if (inCall && !mutedByCall) {
                    mutedByCall = true
                    setPlaybackMuted(true)
                } else if (!inCall && mutedByCall) {
                    mutedByCall = false
                    setPlaybackMuted(false)
                }
            }
        }
        am.addOnModeChangedListener(mainExecutor, listener)
        return listener
    }

    @RequiresApi(Build.VERSION_CODES.S)
    private fun removeModeListener(listener: Any?) {
        val l = listener as? AudioManager.OnModeChangedListener ?: return
        getSystemService(AudioManager::class.java)?.removeOnModeChangedListener(l)
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

    override fun onDestroy() {
        runCatching { unregisterReceiver(noisyReceiver) }
        capture?.stop()
        capture = null
        mic?.stop()
        playback?.stop()
        playback = null
        session?.release()
        session = null
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) removeModeListener(modeListener)
        modeListener = null
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
        /** The widget asked to listen; stay in the foreground while the engine connects. */
        val listenStarting: Boolean = false,
    )

    companion object {
        const val ACTION_UPDATE = "net.soundpush.UPDATE"
        const val ACTION_START_APP_AUDIO = "net.soundpush.START_APP_AUDIO"
        const val ACTION_TOGGLE_LISTEN = "net.soundpush.TOGGLE_LISTEN"
        const val EXTRA_PLAYBACK = "playback"
        const val EXTRA_MIC = "mic"
        const val EXTRA_APP_AUDIO = "appAudio"
        const val EXTRA_STAY = "stayAvailable"
        const val EXTRA_PROJECTION_CODE = "projectionCode"
        const val EXTRA_PROJECTION_DATA = "projectionData"
        private const val CUSTOM_ACTION_STOP = "net.soundpush.session.STOP"
        private const val LISTEN_KIND = "receiveSystemAudio"
        private const val LISTEN_WAIT_MS = 20_000L
        /** Output level while another app briefly asks us to lower the volume (about −10 dB). */
        private const val DUCK_GAIN = 0.3f

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
