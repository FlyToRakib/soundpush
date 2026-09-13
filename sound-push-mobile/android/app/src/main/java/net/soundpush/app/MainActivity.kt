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
import androidx.navigation.NavHostController
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.currentBackStackEntryAsState
import androidx.navigation.compose.rememberNavController
import androidx.navigation.NavGraph.Companion.findStartDestination
import kotlinx.coroutines.flow.MutableSharedFlow
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

    /** Routes waiting for the microphone permission or screen-capture consent. */
    private var pendingStart: Pair<String, List<String>>? = null

    /** A remote microphone request the user allowed, waiting for the microphone permission. */
    private var pendingResponse: Pair<Long, Boolean>? = null

    private var afterCameraGranted: (() -> Unit)? = null
    private val messages = MutableSharedFlow<String>(extraBufferCapacity = 4)

    private val micPermission = registerForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
        pendingStart?.let { (peer, kinds) ->
            pendingStart = null
            if (granted) startRoutes(peer, kinds)
        }
        pendingResponse?.let { (requestId, remember) ->
            pendingResponse = null
            SoundPush.command { respondRouteRequest(requestId.toULong(), granted, remember && granted) }
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
        val pending = pendingStart ?: return@registerForActivityResult
        pendingStart = null
        val data = result.data
        if (result.resultCode == RESULT_OK && data != null) {
            StreamingService.startAppAudio(this, result.resultCode, data)
            pending.second.forEach { kind -> SoundPush.command { startRoute(pending.first, kind) } }
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        if (Build.VERSION.SDK_INT >= 33 && !hasPermission(Manifest.permission.POST_NOTIFICATIONS)) {
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

    private fun hasPermission(permission: String) =
        ContextCompat.checkSelfPermission(this, permission) == PackageManager.PERMISSION_GRANTED

    /** Check the permissions each route kind needs, then start. */
    private fun startRoutes(peerId: String, kinds: List<String>) {
        val needsMic = kinds.any { it.startsWith("sendMic") }
        val needsProjection = kinds.contains("sendAppAudio")
        if (needsMic && !hasPermission(Manifest.permission.RECORD_AUDIO)) {
            pendingStart = peerId to kinds
            micPermission.launch(Manifest.permission.RECORD_AUDIO)
            return
        }
        if (needsProjection && Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            pendingStart = peerId to kinds
            val manager = getSystemService(MediaProjectionManager::class.java)
            projectionConsent.launch(manager.createScreenCaptureIntent())
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
            pendingResponse = request.requestId to remember
            micPermission.launch(Manifest.permission.RECORD_AUDIO)
            return
        }
        SoundPush.command { respondRouteRequest(request.requestId.toULong(), accept, remember) }
    }
}
