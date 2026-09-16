package net.soundpush.engine

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import android.os.Build
import android.util.Log
import androidx.annotation.ChecksSdkIntAtLeast
import androidx.core.content.ContextCompat
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.filterNotNull
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeoutOrNull
import uniffi.soundpush_ffi.FfiException
import uniffi.soundpush_ffi.MobilePlatform
import uniffi.soundpush_ffi.SoundPushEngine
import uniffi.soundpush_ffi.StateListener
import android.provider.Settings as AndroidSettings

/** Callbacks the app shell provides (foreground service, attention notifications). */
interface PlatformDelegate {
    fun keepAlive(playback: Boolean, microphone: Boolean, appAudio: Boolean)
    fun attentionNeeded(key: String, peerName: String)
}

/**
 * Process-wide engine access. UI observes [state]; commands run off the main thread.
 */
object SoundPush {
    @Volatile private lateinit var engine: SoundPushEngine
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val _state = MutableStateFlow<EngineState?>(null)
    private val _errors = MutableSharedFlow<ErrorView>(extraBufferCapacity = 8)

    @Volatile private var delegate: PlatformDelegate? = null

    private val _startError = MutableStateFlow<String?>(null)

    val state: StateFlow<EngineState?> = _state
    val errors: SharedFlow<ErrorView> = _errors

    /** Set when the engine could not start; the UI shows it instead of a blank screen. */
    val startError: StateFlow<String?> = _startError
    val isStarted get() = ::engine.isInitialized

    @Volatile private var appVersion: String = ""

    /** Last playback path handed to the native backend (null until the first state). */
    @Volatile private var platformOutput: Boolean? = null

    /** False while nothing can see the UI: the app is in the background, or the screen is off. */
    private val uiLive = MutableStateFlow(true)

    /** The newest snapshot from the engine, still as JSON. Conflated: only the latest is decoded. */
    private val rawState = MutableStateFlow<String?>(null)

    /** A stats-only snapshot held back while the UI was hidden; published when it comes back. */
    @Volatile private var held: EngineState? = null

    /** Remember the app version so any entry point (activity, service, widget, boot) can start the engine. */
    fun configure(appVersion: String) {
        this.appVersion = appVersion
    }

    /**
     * Start the engine if it isn't running. The engine starts lazily from whatever needs it
     * (the activity, the streaming service, the widget, "Stay available" after a restart), never
     * just because the process started for a broadcast.
     */
    fun ensureStarted(context: Context) {
        if (!isStarted) start(context, appVersion)
    }

    /**
     * Compatibility output, output audio effects or a chosen output device move playback to the
     * platform player (AudioTrack), where the device's own effects and preferred-device routing
     * apply; otherwise the low-latency path is used. Runs off the main thread, so the native call
     * never blocks the UI.
     */
    @Synchronized
    private fun applyPlatformOutput(state: EngineState) {
        val wanted = state.settings.output.compatibilityOutput ||
            state.settings.output.outputEffects ||
            OutputPreference.target.value != OutputPreference.Target.Automatic
        if (platformOutput == wanted) return
        platformOutput = wanted
        runCatching { engine.setPlatformOutput(wanted) }.onFailure { log(LogLevel.Warn, TAG, "could not switch playback path", it) }
    }

    /**
     * Start the engine on a background thread so the first frame draws immediately
     * (Keystore, native library loading and network setup all take time).
     * Safe to call again after a failure (the "Try again" button does).
     */
    fun start(context: Context, appVersion: String) {
        val app = context.applicationContext
        scope.launch { startBlocking(app, appVersion) }
    }

    @Synchronized
    private fun startBlocking(app: Context, appVersion: String) {
        if (isStarted) return
        _startError.value = null
        try {
            NativeContext.init(app)
            OutputPreference.load(app)
            engine = SoundPushEngine(AndroidPlatform(app), appVersion)
            // The engine's thread only hands the snapshot over; decoding happens in [consumeState].
            engine.setListener(object : StateListener {
                override fun onState(stateJson: String) {
                    rawState.value = stateJson
                }
            })
            scope.launch { consumeState() }
            // Reconnect at once when the network changes, whether or not a stream is running.
            NetworkWatcher.start(app) { command { networkChanged() } }
            // Picking or clearing an output device (Settings → Audio) switches the playback path live.
            scope.launch { OutputPreference.target.collect { _state.value?.let(::applyPlatformOutput) } }
        } catch (e: FfiException.Engine) {
            log(LogLevel.Error, TAG, "engine start failed: ${e.key}: ${e.detail}", e)
            _startError.value = e.detail.ifBlank { e.key }
        } catch (t: Throwable) {
            log(LogLevel.Error, TAG, "engine start failed", t)
            _startError.value = t.message ?: t.javaClass.simpleName
        }
    }

    private const val TAG = "SoundPush"

    /** The computer's virtual microphone already has a feed; the user decides whether to take it. */
    private const val VIRTUAL_MIC_BUSY = "error.audio.virtualMicBusy"

    /** How long a snapshot waits while nothing can see the UI. */
    private const val BACKGROUND_INTERVAL_MS = 500L

    /**
     * Decode and publish the engine's snapshots.
     *
     * On screen, every snapshot is decoded as it arrives (the engine publishes at most one per
     * 50 ms). While the app is in the background or the screen is off, nothing draws the statistics
     * and level meters and the notification never showed them, so decoding drops to one snapshot
     * per [BACKGROUND_INTERVAL_MS] and a snapshot that moves nothing but those numbers is not
     * published at all (plan §14.6, §8.4). Everything anyone can act on — routes, devices, settings,
     * pairing, notices — still lands, so the notification, the foreground service and reconnection
     * behave exactly as they do on screen. Streaming never passes through here.
     */
    private suspend fun consumeState() {
        rawState.filterNotNull().collect { json ->
            runCatching { EngineJson.decodeFromString<EngineState>(json) }
                .onSuccess(::publish)
                .onFailure { log(LogLevel.Error, TAG, "could not decode engine state", it) }
            // [rawState] keeps only the newest snapshot, so the wait costs nothing but the wait.
            if (!uiLive.value) withTimeoutOrNull(BACKGROUND_INTERVAL_MS) { uiLive.first { it } }
        }
    }

    @Synchronized
    private fun publish(next: EngineState) {
        val last = _state.value
        if (!uiLive.value && last != null && StateUpdates.onlyLiveNumbersChanged(last, next)) {
            held = next
            return
        }
        held = null
        applyPlatformOutput(next)
        _state.value = next
    }

    /**
     * Whether anything can see the UI. The app shell calls this when the app comes to the front or
     * goes to the back, and when the screen turns on or off.
     */
    fun setUiLive(live: Boolean) {
        if (uiLive.value == live) return
        uiLive.value = live
        // Whatever was held back while the screen was off is worth showing again.
        if (live) scope.launch { held?.let(::publish) }
    }

    /**
     * App log line through the engine logger (plan §28.1): the same log files and logcat stream as
     * the engine, so one log tells the whole story. Before the native library is loaded it falls
     * back to logcat only. Never pass secrets or pairing codes.
     */
    fun log(level: LogLevel, tag: String, message: String, error: Throwable? = null) {
        val text = if (error != null) "$message: ${error.javaClass.simpleName}: ${error.message}" else message
        if (isStarted && runCatching { uniffi.soundpush_ffi.logMessage(level.value, tag, text) }.isSuccess) return
        when (level) {
            LogLevel.Error -> Log.e(tag, message, error)
            LogLevel.Warn -> Log.w(tag, message, error)
            LogLevel.Info -> Log.i(tag, message, error)
            LogLevel.Debug -> Log.d(tag, message, error)
        }
    }

    enum class LogLevel(val value: String) { Error("error"), Warn("warn"), Info("info"), Debug("debug") }

    fun setDelegate(d: PlatformDelegate) {
        delegate = d
    }

    /** Run a command on the IO dispatcher, reporting engine errors to [errors]. */
    fun command(block: SoundPushEngine.() -> Unit) {
        if (!isStarted) return
        scope.launch {
            try {
                engine.block()
            } catch (e: FfiException.Engine) {
                _errors.tryEmit(ErrorView(key = e.key, code = e.code, message = e.detail, fix = e.fix))
            }
        }
    }

    /**
     * Start a route. Only one device at a time can be a computer's microphone, so the engine
     * refuses a second one with `error.audio.virtualMicBusy`; [onMicrophoneBusy] then runs on the
     * main thread so the app can ask "Replace current microphone source?" and call this again with
     * [replace] (plan §8.2). Every other error goes to [errors] as usual.
     */
    fun startRoute(peerId: String, kind: String, replace: Boolean = false, onMicrophoneBusy: () -> Unit = {}) {
        if (!isStarted) return
        scope.launch {
            try {
                engine.startRoute(peerId, kind, replace)
            } catch (e: FfiException.Engine) {
                if (e.key == VIRTUAL_MIC_BUSY) {
                    withContext(Dispatchers.Main) { onMicrophoneBusy() }
                } else {
                    _errors.tryEmit(ErrorView(key = e.key, message = e.detail, fix = e.fix))
                }
            }
        }
    }

    /** Blocking access for background components (service threads). */
    fun <T> direct(block: SoundPushEngine.() -> T): T? = if (isStarted) runCatching { engine.block() }.getOrNull() else null

    /** The local security log, newest first. Blocking: call off the main thread. Null if the engine isn't running. */
    fun securityLog(): List<AuditEntry>? = direct { auditLogJson() }?.let { json -> runCatching { EngineJson.decodeFromString<List<AuditEntry>>(json) }.getOrNull() }

    /** Delete the security log (a "log cleared" entry remains). Blocking: call off the main thread. */
    fun clearSecurityLog(): Boolean = direct { clearAuditLog() } != null

    fun updateSettings(transform: (Settings) -> Settings) {
        val current = _state.value?.settings ?: return
        val next = transform(current)
        command { updateSettings(EngineJson.encodeToString(Settings.serializer(), next)) }
    }

    private class AndroidPlatform(private val context: Context) : MobilePlatform {
        override fun dataDir(): String = context.filesDir.absolutePath

        override fun storageKey(): ByteArray = StorageKey.get(context)

        override fun deviceName(): String = AndroidSettings.Global.getString(context.contentResolver, AndroidSettings.Global.DEVICE_NAME)
            ?.takeIf { it.isNotBlank() }
            ?: "${Build.MANUFACTURER.replaceFirstChar { it.uppercase() }} ${Build.MODEL}"

        override fun microphonePermitted(): Boolean = ContextCompat.checkSelfPermission(context, Manifest.permission.RECORD_AUDIO) ==
            PackageManager.PERMISSION_GRANTED

        @ChecksSdkIntAtLeast(api = Build.VERSION_CODES.Q)
        override fun appAudioSupported(): Boolean = Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q

        override fun keepAlive(playback: Boolean, microphone: Boolean, appAudio: Boolean) {
            delegate?.keepAlive(playback, microphone, appAudio)
        }

        override fun attentionNeeded(key: String, peerName: String) {
            delegate?.attentionNeeded(key, peerName)
        }
    }
}
