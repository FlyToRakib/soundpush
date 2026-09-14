package net.soundpush.engine

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import android.os.Build
import android.provider.Settings as AndroidSettings
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
import kotlinx.coroutines.launch
import uniffi.soundpush_ffi.FfiException
import uniffi.soundpush_ffi.MobilePlatform
import uniffi.soundpush_ffi.SoundPushEngine
import uniffi.soundpush_ffi.StateListener

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
        val wanted = state.settings.output.compatibilityOutput || state.settings.output.outputEffects ||
            OutputPreference.target.value != OutputPreference.Target.Automatic
        if (platformOutput == wanted) return
        platformOutput = wanted
        runCatching { engine.setPlatformOutput(wanted) }.onFailure { Log.w(TAG, "could not switch playback path", it) }
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
            engine.setListener(object : StateListener {
                override fun onState(stateJson: String) {
                    runCatching { EngineJson.decodeFromString<EngineState>(stateJson) }
                        .onSuccess {
                            applyPlatformOutput(it)
                            _state.value = it
                        }
                        .onFailure { Log.e(TAG, "could not decode engine state", it) }
                }
            })
            // Reconnect at once when the network changes, whether or not a stream is running.
            NetworkWatcher.start(app) { command { networkChanged() } }
            // Picking or clearing an output device (Settings → Audio) switches the playback path live.
            scope.launch { OutputPreference.target.collect { _state.value?.let(::applyPlatformOutput) } }
        } catch (e: FfiException.Engine) {
            Log.e(TAG, "engine start failed: ${e.key}: ${e.detail}", e)
            _startError.value = e.detail.ifBlank { e.key }
        } catch (t: Throwable) {
            Log.e(TAG, "engine start failed", t)
            _startError.value = t.message ?: t.javaClass.simpleName
        }
    }

    private const val TAG = "SoundPush"

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
                _errors.tryEmit(ErrorView(key = e.key, message = e.detail, fix = e.fix))
            }
        }
    }

    /** Blocking access for background components (service threads). */
    fun <T> direct(block: SoundPushEngine.() -> T): T? = if (isStarted) runCatching { engine.block() }.getOrNull() else null

    /** The local security log, newest first. Blocking: call off the main thread. Null if the engine isn't running. */
    fun securityLog(): List<AuditEntry>? =
        direct { auditLogJson() }?.let { json -> runCatching { EngineJson.decodeFromString<List<AuditEntry>>(json) }.getOrNull() }

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

        override fun deviceName(): String =
            AndroidSettings.Global.getString(context.contentResolver, AndroidSettings.Global.DEVICE_NAME)
                ?.takeIf { it.isNotBlank() }
                ?: "${Build.MANUFACTURER.replaceFirstChar { it.uppercase() }} ${Build.MODEL}"

        override fun microphonePermitted(): Boolean =
            ContextCompat.checkSelfPermission(context, Manifest.permission.RECORD_AUDIO) ==
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
