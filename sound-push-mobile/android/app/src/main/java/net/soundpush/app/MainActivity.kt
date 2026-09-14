package net.soundpush.app

import android.Manifest
import android.content.pm.PackageManager
import android.graphics.Color as AndroidColor
import android.media.projection.MediaProjectionManager
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.SystemBarStyle
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.selection.toggleable
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Checkbox
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.content.ContextCompat
import androidx.core.os.BundleCompat
import androidx.navigation.NavHostController
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.currentBackStackEntryAsState
import androidx.navigation.compose.rememberNavController
import androidx.navigation.NavGraph.Companion.findStartDestination
import java.io.Serializable
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.distinctUntilChangedBy
import kotlinx.coroutines.flow.mapNotNull
import kotlinx.coroutines.launch
import net.soundpush.audio.AudioScreen
import net.soundpush.devices.DevicesScreen
import net.soundpush.devices.QrScanner
import net.soundpush.engine.EngineState
import net.soundpush.engine.RouteRequestPrompt
import net.soundpush.engine.SoundPush
import net.soundpush.home.HomeScreen
import net.soundpush.service.StreamingService
import net.soundpush.settings.SettingsScreen
import net.soundpush.ui.R
import net.soundpush.ui.components.Labels
import net.soundpush.ui.components.ScreenHeader
import net.soundpush.ui.icons.SpIcons
import net.soundpush.ui.theme.SoundPushTheme
import net.soundpush.ui.theme.Tokens

class MainActivity : ComponentActivity() {

    /** What to finish once a permission dialog or the screen-capture consent returns. Survives recreation. */
    private sealed interface Pending : Serializable {
        /** Routes the user started from Home. */
        data class Start(val peerId: String, val kinds: List<String>) : Pending

        /** A remote request the user allowed. */
        data class Respond(val requestId: Long, val remember: Boolean) : Pending

        /** App-audio routes that opened without consent (a remembered permission, resume on start). */
        data class Consent(val routeIds: List<String>) : Pending
    }

    private var pending: Pending? = null

    /** Route ids the consent safety net already asked about, so a refused consent is not asked again. */
    private var consentAskedFor: List<String> = emptyList()

    private var afterCameraGranted: (() -> Unit)? = null
    private val messages = MutableSharedFlow<String>(extraBufferCapacity = 4)

    private fun takePending(): Pending? = pending.also { pending = null }

    private val micPermission = registerForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
        when (val p = takePending()) {
            is Pending.Start -> if (granted) startRoutes(p.peerId, p.kinds)
            is Pending.Respond -> SoundPush.command { respondRouteRequest(p.requestId.toULong(), granted, p.remember && granted) }
            else -> {}
        }
        if (!granted) messages.tryEmit(getString(R.string.permission_mic_needed))
    }

    private val cameraPermission = registerForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
        val next = afterCameraGranted
        afterCameraGranted = null
        if (granted) next?.invoke() else messages.tryEmit(getString(R.string.scan_camera_denied))
    }

    private val notificationPermission = registerForActivityResult(ActivityResultContracts.RequestPermission()) { }

    private val projectionConsent = registerForActivityResult(ActivityResultContracts.StartActivityForResult()) { result ->
        val data = result.data
        val ok = result.resultCode == RESULT_OK && data != null
        if (result.resultCode == RESULT_OK && data != null) StreamingService.startAppAudio(this, result.resultCode, data)
        when (val p = takePending()) {
            is Pending.Start -> if (ok) p.kinds.forEach { kind -> SoundPush.command { startRoute(p.peerId, kind) } }
            is Pending.Respond -> SoundPush.command { respondRouteRequest(p.requestId.toULong(), ok, p.remember && ok) }
            // Without consent the route would carry silence: end it rather than pretend.
            is Pending.Consent -> if (!ok) p.routeIds.forEach { id -> SoundPush.command { stopRoute(id) } }
            null -> {}
        }
        if (!ok) messages.tryEmit(getString(R.string.permission_capture_needed))
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        pending = savedInstanceState?.let { BundleCompat.getSerializable(it, KEY_PENDING, Pending::class.java) }
        // Ask once per launch, not again on every rotation after a "don't allow".
        if (savedInstanceState == null && Build.VERSION.SDK_INT >= 33 && !hasPermission(Manifest.permission.POST_NOTIFICATIONS)) {
            notificationPermission.launch(Manifest.permission.POST_NOTIFICATIONS)
        }
        setContent {
            val state by SoundPush.state.collectAsState()
            val startError by SoundPush.startError.collectAsState()
            val theme = state?.settings?.theme ?: "system"
            val dark = when (theme) {
                "light" -> false
                "dark" -> true
                else -> isSystemInDarkTheme()
            }
            // Status/navigation bar icons must follow the app theme, not only the system theme.
            LaunchedEffect(dark) {
                val style = if (dark) {
                    SystemBarStyle.dark(AndroidColor.TRANSPARENT)
                } else {
                    SystemBarStyle.light(AndroidColor.TRANSPARENT, AndroidColor.TRANSPARENT)
                }
                enableEdgeToEdge(statusBarStyle = style, navigationBarStyle = style)
            }
            SoundPushTheme(theme) {
                val current = state
                val error = startError
                when {
                    current != null -> App(current)
                    error != null -> StartupError(error) { SoundPush.start(applicationContext, BuildVersion.NAME) }
                    else -> StartupLoading()
                }
            }
        }
    }

    override fun onSaveInstanceState(outState: Bundle) {
        super.onSaveInstanceState(outState)
        outState.putSerializable(KEY_PENDING, pending)
    }

    private fun hasPermission(permission: String) =
        ContextCompat.checkSelfPermission(this, permission) == PackageManager.PERMISSION_GRANTED

    /** Screen-capture consent is single-use and only needed while no app-audio recorder runs. */
    private fun needsCaptureConsent() = Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q && !StreamingService.capturing.value

    private fun askCaptureConsent(next: Pending) {
        pending = next
        val manager = getSystemService(MediaProjectionManager::class.java)
        projectionConsent.launch(manager.createScreenCaptureIntent())
    }

    /** Check the permissions each route kind needs, then start. */
    private fun startRoutes(peerId: String, kinds: List<String>) {
        val needsMic = kinds.any { it.startsWith("sendMic") }
        val needsProjection = kinds.contains("sendAppAudio")
        if (needsMic && !hasPermission(Manifest.permission.RECORD_AUDIO)) {
            pending = Pending.Start(peerId, kinds)
            micPermission.launch(Manifest.permission.RECORD_AUDIO)
            return
        }
        if (needsProjection && needsCaptureConsent()) {
            askCaptureConsent(Pending.Start(peerId, kinds))
            return
        }
        kinds.forEach { kind -> SoundPush.command { startRoute(peerId, kind) } }
    }

    private fun withCamera(block: () -> Unit) {
        if (hasPermission(Manifest.permission.CAMERA)) {
            block()
        } else {
            afterCameraGranted = block
            cameraPermission.launch(Manifest.permission.CAMERA)
        }
    }

    @Composable
    private fun App(state: EngineState) {
        val nav = rememberNavController()
        val backStack by nav.currentBackStackEntryAsState()
        val route = backStack?.destination?.route ?: "home"
        val snackbar = remember { SnackbarHostState() }
        val scope = rememberCoroutineScope()
        val context = LocalContext.current
        val showMessage: (String) -> Unit = { message -> scope.launch { snackbar.showSnackbar(message) } }

        LaunchedEffect(Unit) {
            SoundPush.errors.collect { snackbar.showSnackbar(context.getString(Labels.error(it.key))) }
        }
        LaunchedEffect(Unit) {
            messages.collect { snackbar.showSnackbar(it) }
        }
        // Engine notices (paired, a store was reset, a device forgot us) show once each, in order.
        LaunchedEffect(Unit) {
            SoundPush.state
                .mapNotNull { it?.notices?.firstOrNull() }
                .distinctUntilChangedBy { it.id }
                .collect { notice ->
                    SoundPush.command { dismissNotice(notice.id.toULong()) }
                    val text = notice.error?.let { context.getString(Labels.error(it.key)) }
                        ?: context.getString(Labels.notice(notice.key), *notice.args.toTypedArray())
                    snackbar.showSnackbar(text)
                }
        }

        // App-audio routes opened without this screen (a remembered permission, resume on start) carry
        // silence until the user grants screen capture. Ask once such a route has been open a moment;
        // the moment covers the normal path, where the recorder starts right after consent.
        val capturing by StreamingService.capturing.collectAsState()
        val unconsented = if (capturing) emptyList() else state.routes.filter { it.kind == "sendAppAudio" }.map { it.routeId }
        LaunchedEffect(unconsented) {
            if (unconsented.isEmpty() || unconsented == consentAskedFor) return@LaunchedEffect
            delay(1_500)
            if (pending == null && needsCaptureConsent()) {
                consentAskedFor = unconsented
                askCaptureConsent(Pending.Consent(unconsented))
            }
        }

        Scaffold(
            containerColor = MaterialTheme.colorScheme.background,
            snackbarHost = { SnackbarHost(snackbar) },
            topBar = {
                if (route != "scan") {
                    ScreenHeader(
                        title = stringResource(titleFor(route)),
                        onBack = if (route == "audio") ({ nav.popBackStack() }) else null,
                    )
                }
            },
            bottomBar = {
                if (route != "scan") BottomBar(route) { destination -> nav.navigateToTab(destination) }
            },
        ) { padding ->
            NavHost(nav, startDestination = "home", modifier = Modifier.padding(padding)) {
                composable("home") {
                    HomeScreen(
                        state = state,
                        onStartRoutes = ::startRoutes,
                        onPair = { withCamera { nav.navigate("scan") } },
                        onOpenDevices = { nav.navigateToTab("devices") },
                        onShowMessage = showMessage,
                    )
                }
                composable("devices") {
                    DevicesScreen(state, onScan = { withCamera { nav.navigate("scan") } }, onShowMessage = showMessage)
                }
                composable("settings") { SettingsScreen(state, onOpenAudio = { nav.navigate("audio") }) }
                composable("audio") { AudioScreen(state) }
                composable("scan") {
                    QrScanner(
                        onResult = { uri ->
                            SoundPush.command { pairWithQr(uri) }
                            nav.popBackStack()
                        },
                        onClose = { nav.popBackStack() },
                    )
                }
            }
        }

        Overlays(state)
    }

    private fun NavHostController.navigateToTab(destination: String) {
        navigate(destination) {
            popUpTo(graph.findStartDestination().id) { saveState = true }
            launchSingleTop = true
            restoreState = true
        }
    }

    @Composable
    private fun Overlays(state: EngineState) {
        val prompt = state.pairing.prompts.firstOrNull()
        val request = state.requests.firstOrNull()
        if (prompt != null) {
            AlertDialog(
                onDismissRequest = {},
                title = { Text(stringResource(R.string.pair_code_title, prompt.peerName)) },
                text = {
                    Column(Modifier.fillMaxWidth(), horizontalAlignment = Alignment.CenterHorizontally) {
                        Text(stringResource(R.string.pair_code_body), textAlign = TextAlign.Center)
                        Text(
                            "${prompt.code.take(3)} ${prompt.code.drop(3)}",
                            fontSize = 36.sp,
                            fontWeight = FontWeight.SemiBold,
                            letterSpacing = 4.sp,
                            modifier = Modifier.padding(top = Tokens.Space.md),
                        )
                    }
                },
                confirmButton = {
                    Button(onClick = { SoundPush.command { confirmPairing(prompt.peerId, true) } }) {
                        Text(stringResource(R.string.pair_code_match))
                    }
                },
                dismissButton = {
                    TextButton(onClick = { SoundPush.command { confirmPairing(prompt.peerId, false) } }) {
                        Text(stringResource(R.string.pair_code_cancel))
                    }
                },
            )
        } else if (request != null) {
            var rememberChoice by remember(request.requestId) { mutableStateOf(false) }
            AlertDialog(
                onDismissRequest = {},
                icon = { Icon(if (request.kind.startsWith("sendMic")) SpIcons.Mic else SpIcons.Speaker, null) },
                title = { Text(stringResource(Labels.requestTitle(request.kind), request.peerName), textAlign = TextAlign.Center) },
                text = {
                    Row(
                        Modifier
                            .fillMaxWidth()
                            .heightIn(min = 48.dp)
                            .toggleable(value = rememberChoice, role = Role.Checkbox) { rememberChoice = it },
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Checkbox(checked = rememberChoice, onCheckedChange = null)
                        Spacer(Modifier.width(Tokens.Space.sm))
                        Text(stringResource(R.string.request_remember))
                    }
                },
                confirmButton = {
                    Button(onClick = { respond(request, true, rememberChoice) }) { Text(stringResource(R.string.request_allow)) }
                },
                dismissButton = {
                    TextButton(onClick = { respond(request, false, rememberChoice) }) { Text(stringResource(R.string.request_deny)) }
                },
            )
        }
    }

    /** `kind` is from this phone's point of view: "sendMic…" means the other device wants this phone's microphone. */
    private fun respond(request: RouteRequestPrompt, accept: Boolean, remember: Boolean) {
        if (accept && request.kind.startsWith("sendMic") && !hasPermission(Manifest.permission.RECORD_AUDIO)) {
            pending = Pending.Respond(request.requestId, remember)
            micPermission.launch(Manifest.permission.RECORD_AUDIO)
            return
        }
        // The computer wants this phone's app audio: get screen-capture consent before accepting.
        if (accept && request.kind == "sendAppAudio" && needsCaptureConsent()) {
            askCaptureConsent(Pending.Respond(request.requestId, remember))
            return
        }
        SoundPush.command { respondRouteRequest(request.requestId.toULong(), accept, remember) }
    }

    private companion object {
        const val KEY_PENDING = "pending"
    }
}
