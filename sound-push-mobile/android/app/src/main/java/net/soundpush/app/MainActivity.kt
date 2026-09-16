package net.soundpush.app

import android.Manifest
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.Color as AndroidColor
import android.media.projection.MediaProjectionManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.provider.Settings as AndroidSettings
import androidx.activity.SystemBarStyle
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.annotation.StringRes
import androidx.appcompat.app.AppCompatActivity
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
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat
import androidx.core.os.BundleCompat
import androidx.navigation.NavHostController
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.currentBackStackEntryAsState
import androidx.navigation.compose.rememberNavController
import androidx.navigation.NavGraph.Companion.findStartDestination
import java.io.Serializable
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.distinctUntilChangedBy
import kotlinx.coroutines.flow.mapNotNull
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import net.soundpush.audio.AudioScreen
import net.soundpush.devices.DevicesScreen
import net.soundpush.devices.QrScanner
import net.soundpush.engine.DeviceStatus
import net.soundpush.engine.EngineState
import net.soundpush.engine.ErrorView
import net.soundpush.engine.RouteRequestPrompt
import net.soundpush.engine.SoundPush
import net.soundpush.home.HomeScreen
import net.soundpush.service.StreamingService
import net.soundpush.settings.AuditLogScreen
import net.soundpush.settings.BatteryGuideScreen
import net.soundpush.settings.PermissionRow
import net.soundpush.settings.SettingsScreen
import net.soundpush.settings.TroubleTopic
import net.soundpush.settings.TroubleshootTopicScreen
import net.soundpush.settings.TroubleshooterScreen
import net.soundpush.ui.R
import net.soundpush.ui.components.BannerModel
import net.soundpush.ui.components.Choice
import net.soundpush.ui.components.Labels
import net.soundpush.ui.components.LocalWidthClass
import net.soundpush.ui.components.WidthClass
import net.soundpush.ui.components.ScreenHeader
import net.soundpush.ui.icons.SpIcons
import net.soundpush.ui.theme.SoundPushTheme
import net.soundpush.ui.theme.Tokens

/** AppCompatActivity (not only ComponentActivity) so the per-app language also works on Android 8–12. */
class MainActivity : AppCompatActivity() {

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

    /** Whether notifications can be shown; re-read on every resume (the user may change it in settings). */
    private var notificationsEnabled by mutableStateOf(true)

    /** Explain notifications once per launch, the first time a stream starts without them. */
    private var notificationRationale by mutableStateOf(false)
    private var rationaleShown = false
    private var exporting = false

    private val prefs by lazy { getSharedPreferences(PREFS, MODE_PRIVATE) }
    private val permissionMemory by lazy { PermissionMemory(prefs) }

    /** Runtime permission states, re-read on every resume (the user may change them in settings). */
    private var notificationState by mutableStateOf(PermissionState.Granted)
    private var micState by mutableStateOf(PermissionState.NotAsked)
    private var cameraState by mutableStateOf(PermissionState.NotAsked)

    /** A permission the user just needed was refused for good: explain it and offer app settings. */
    private var blockedPermission by mutableStateOf<String?>(null)

    /** Screen-capture consent was refused for streams the user started: offer to ask again. */
    private var captureRefused by mutableStateOf<Pending.Start?>(null)

    private fun takePending(): Pending? = pending.also { pending = null }

    private val micPermission = registerForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
        when (val p = takePending()) {
            is Pending.Start -> if (granted) startRoutes(p.peerId, p.kinds)
            is Pending.Respond -> SoundPush.command { respondRouteRequest(p.requestId.toULong(), granted, p.remember && granted) }
            else -> {}
        }
        onPermissionResult(Manifest.permission.RECORD_AUDIO, granted, R.string.permission_mic_needed)
    }

    private val cameraPermission = registerForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
        val next = afterCameraGranted
        afterCameraGranted = null
        if (granted) next?.invoke()
        onPermissionResult(Manifest.permission.CAMERA, granted, R.string.scan_camera_denied)
    }

    private val notificationPermission = registerForActivityResult(ActivityResultContracts.RequestPermission()) {
        refreshPermissions()
    }

    private val projectionConsent = registerForActivityResult(ActivityResultContracts.StartActivityForResult()) { result ->
        val data = result.data
        val ok = result.resultCode == RESULT_OK && data != null
        if (result.resultCode == RESULT_OK && data != null) StreamingService.startAppAudio(this, result.resultCode, data)
        when (val p = takePending()) {
            is Pending.Start -> when {
                ok -> p.kinds.forEach { kind -> SoundPush.command { startRoute(p.peerId, kind) } }
                // Consent is asked every time and can't be blocked: explain and let the user try again.
                else -> captureRefused = p
            }
            is Pending.Respond -> SoundPush.command { respondRouteRequest(p.requestId.toULong(), ok, p.remember && ok) }
            // Without consent the route would carry silence: end it rather than pretend.
            is Pending.Consent -> if (!ok) p.routeIds.forEach { id -> SoundPush.command { stopRoute(id) } }
            null -> {}
        }
        if (!ok && captureRefused == null) messages.tryEmit(getString(R.string.permission_capture_needed))
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        // Starts loading the preferences file on the framework's own thread, before the first resume reads it.
        prefs
        // The engine starts with the first screen, not with the process (see SoundPushApplication).
        SoundPush.ensureStarted(applicationContext)
        DeviceStatus.start(this)
        pending = savedInstanceState?.let { BundleCompat.getSerializable(it, KEY_PENDING, Pending::class.java) }
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
                // Window width (split screen and foldables included) picks rail or bottom bar, one or two panes.
                val widthClass = WidthClass.of(LocalConfiguration.current.screenWidthDp)
                CompositionLocalProvider(LocalWidthClass provides widthClass) {
                    when {
                        current != null -> App(current)
                        error != null -> StartupError(error) { SoundPush.start(applicationContext, BuildVersion.NAME) }
                        else -> StartupLoading()
                    }
                }
            }
        }
    }

    override fun onResume() {
        super.onResume()
        refreshPermissions()
    }

    override fun onSaveInstanceState(outState: Bundle) {
        super.onSaveInstanceState(outState)
        outState.putSerializable(KEY_PENDING, pending)
    }

    private fun hasPermission(permission: String) =
        ContextCompat.checkSelfPermission(this, permission) == PackageManager.PERMISSION_GRANTED

    private fun refreshPermissions() {
        notificationsEnabled = NotificationManagerCompat.from(this).areNotificationsEnabled() &&
            (Build.VERSION.SDK_INT < 33 || hasPermission(Manifest.permission.POST_NOTIFICATIONS))
        notificationState = when {
            notificationsEnabled -> PermissionState.Granted
            Build.VERSION.SDK_INT >= 33 && !prefs.getBoolean(KEY_NOTIFICATIONS_ASKED, false) -> PermissionState.NotAsked
            Build.VERSION.SDK_INT >= 33 && shouldShowRequestPermissionRationale(Manifest.permission.POST_NOTIFICATIONS) -> PermissionState.Denied
            // Refused for good, or turned off in the app's notification settings.
            else -> PermissionState.Blocked
        }
        micState = permissionState(Manifest.permission.RECORD_AUDIO)
        cameraState = permissionState(Manifest.permission.CAMERA)
    }

    private fun permissionState(permission: String) = PermissionState.of(
        granted = hasPermission(permission),
        refusedBefore = permissionMemory.refusedBefore(permission),
        shouldShowRationale = shouldShowRequestPermissionRationale(permission),
    )

    /**
     * After a permission dialog: a first refusal says what the feature needs; a refusal for good
     * (the system no longer shows its dialog) opens an explanation with a way to app settings.
     * The history is read before recording this answer, so a dialog dismissed without a choice
     * isn't mistaken for a permanent refusal.
     */
    private fun onPermissionResult(permission: String, granted: Boolean, @StringRes refusedMessage: Int) {
        val state = PermissionState.of(granted, permissionMemory.refusedBefore(permission), shouldShowRequestPermissionRationale(permission))
        permissionMemory.record(permission, granted)
        refreshPermissions()
        when (state) {
            PermissionState.Granted -> {}
            PermissionState.Blocked -> blockedPermission = permission
            else -> messages.tryEmit(getString(refusedMessage))
        }
    }

    /** Settings → Permissions: ask again; a blocked permission ends in the explanation with app settings. */
    private fun askFromSettings(permission: String) {
        when (permission) {
            Manifest.permission.RECORD_AUDIO -> micPermission.launch(permission)
            Manifest.permission.CAMERA -> cameraPermission.launch(permission)
            else -> requestNotifications()
        }
    }

    private fun openAppSettings() = openSettings(
        Intent(AndroidSettings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.fromParts("package", packageName, null)),
    )

    /**
     * Ask for notifications with the system dialog while Android still offers it; after "Don't
     * allow" twice (or when turned off in settings) only the app's notification settings can help.
     */
    private fun requestNotifications() {
        if (Build.VERSION.SDK_INT >= 33 && !hasPermission(Manifest.permission.POST_NOTIFICATIONS)) {
            val asked = prefs.getBoolean(KEY_NOTIFICATIONS_ASKED, false)
            if (!asked || shouldShowRequestPermissionRationale(Manifest.permission.POST_NOTIFICATIONS)) {
                prefs.edit().putBoolean(KEY_NOTIFICATIONS_ASKED, true).apply()
                notificationPermission.launch(Manifest.permission.POST_NOTIFICATIONS)
                return
            }
        }
        openSettings(
            Intent(AndroidSettings.ACTION_APP_NOTIFICATION_SETTINGS).putExtra(AndroidSettings.EXTRA_APP_PACKAGE, packageName),
        )
    }

    private fun openSettings(vararg intents: Intent) {
        for (intent in intents) {
            if (runCatching { startActivity(intent) }.isSuccess) return
        }
    }

    /** Screen-capture consent is single-use and only needed while no app-audio recorder runs. */
    private fun needsCaptureConsent() = Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q && !StreamingService.capturing.value

    private fun askCaptureConsent(next: Pending) {
        pending = next
        val manager = getSystemService(MediaProjectionManager::class.java)
        projectionConsent.launch(manager.createScreenCaptureIntent())
    }

    /** Check the permissions each route kind needs, then start. */
    private fun startRoutes(peerId: String, kinds: List<String>) {
        // Streams work without notifications, so explain alongside the start instead of blocking it.
        if (!notificationsEnabled && !rationaleShown) {
            rationaleShown = true
            notificationRationale = true
        }
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

    /** Settings → General → Language. The engine setting is written first; Android then recreates the screen. */
    private fun setLanguage(tag: String) {
        SoundPush.updateSettings { it.copy(language = tag) }
        AppLanguages.apply(tag)
    }

    private fun dismissTip(key: String) {
        SoundPush.updateSettings { if (key in it.dismissedTips) it else it.copy(dismissedTips = it.dismissedTips + key) }
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

        // First run only; people who already paired a device (an update) skip it silently.
        val startDestination = remember { if (ONBOARDING_TIP !in state.settings.dismissedTips && state.trustedPeers.isEmpty()) "onboarding" else "home" }
        LaunchedEffect(Unit) {
            if (ONBOARDING_TIP !in state.settings.dismissedTips && state.trustedPeers.isNotEmpty()) dismissTip(ONBOARDING_TIP)
        }
        // Android owns the per-app language (it can also change in system settings); the engine mirrors it.
        LaunchedEffect(state.settings.language) {
            val inUse = AppLanguages.current()
            if (state.settings.language != inUse) SoundPush.updateSettings { it.copy(language = inUse) }
        }
        val systemLanguage = stringResource(R.string.language_system)
        val languageChoices = remember(systemLanguage) {
            listOf(Choice(AppLanguages.SYSTEM, systemLanguage)) +
                AppLanguages.available(context).map { Choice(it, AppLanguages.displayName(it)) }
        }
        val finishOnboarding: (String) -> Unit = { destination ->
            dismissTip(ONBOARDING_TIP)
            nav.navigate(destination) { popUpTo("onboarding") { inclusive = true } }
        }
        val fromOnboarding = nav.previousBackStackEntry?.destination?.route == "onboarding"
        val fullScreen = route == "scan" || route == "onboarding"
        val showNavigation = !fullScreen && !fromOnboarding
        val showRail = showNavigation && LocalWidthClass.current != WidthClass.Compact

        val exportDiagnostics: () -> Unit = {
            if (!exporting) {
                exporting = true
                scope.launch {
                    showMessage(context.getString(R.string.diagnostics_preparing))
                    val uri = Diagnostics.export(context)
                    exporting = false
                    if (uri != null) Diagnostics.share(this@MainActivity, uri) else showMessage(context.getString(R.string.diagnostics_failed))
                }
            }
        }

        LaunchedEffect(Unit) {
            SoundPush.errors.collect { snackbar.showSnackbar(errorText(context, it)) }
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
                    val text = notice.error?.let { errorText(context, it) }
                        ?: context.getString(Labels.notice(notice.key), *notice.args.toTypedArray())
                    snackbar.showSnackbar(text)
                }
        }

        // A crash last time is mentioned once; the report itself stays on the phone.
        var crashNotice by rememberSaveable { mutableStateOf(false) }
        LaunchedEffect(Unit) {
            if (withContext(Dispatchers.IO) { CrashReports.hasUnseen(context) }) crashNotice = true
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

        val banners = homeBanners(state)
        val network by DeviceStatus.network.collectAsState()
        val usbLabel = stringResource(R.string.peer_via_usb)

        Scaffold(
            // The rail (medium and expanded windows) is drawn beside the scaffold, at the start edge.
            modifier = Modifier.padding(start = if (showRail) RailWidth else 0.dp),
            containerColor = MaterialTheme.colorScheme.background,
            snackbarHost = { SnackbarHost(snackbar) },
            topBar = {
                if (!fullScreen) {
                    val title = if (route == "troubleshoot/{topic}") {
                        TroubleTopic.from(backStack?.arguments?.getString("topic"))?.title ?: titleFor(route)
                    } else {
                        titleFor(route)
                    }
                    ScreenHeader(
                        title = stringResource(title),
                        onBack = if (route in SETTINGS_SUBSCREENS) ({ nav.popBackStack() }) else null,
                    )
                }
            },
            bottomBar = {
                if (showNavigation && !showRail) BottomBar(route) { destination -> nav.navigateToTab(destination) }
            },
        ) { padding ->
            NavHost(nav, startDestination = startDestination, modifier = Modifier.padding(padding)) {
                composable("onboarding") {
                    OnboardingScreen(
                        state = state,
                        notificationsEnabled = notificationsEnabled,
                        onRequestNotifications = ::requestNotifications,
                        onOpenBatteryGuide = { nav.navigate("battery") },
                        onScan = { withCamera { nav.navigate("scan") } },
                        onEnterAddress = { finishOnboarding("devices") },
                        onFinish = { finishOnboarding("home") },
                    )
                }
                composable("home") {
                    HomeScreen(
                        state = state,
                        onStartRoutes = ::startRoutes,
                        onPair = { withCamera { nav.navigate("scan") } },
                        onOpenDevices = { nav.navigateToTab("devices") },
                        onShowMessage = showMessage,
                        banners = banners,
                        peerLabel = { peer ->
                            val viaUsb = network.usbTethering && peer.isConnected && peer.addresses.any(network::isTetherAddress)
                            if (viaUsb) usbLabel else null
                        },
                    )
                }
                composable("devices") {
                    DevicesScreen(state, onScan = { withCamera { nav.navigate("scan") } }, onShowMessage = showMessage)
                }
                composable("settings") {
                    SettingsScreen(
                        state,
                        onOpenAudio = { nav.navigate("audio") },
                        onOpenTroubleshooter = { nav.navigate("troubleshoot") },
                        onOpenBatteryGuide = { nav.navigate("battery") },
                        onExportDiagnostics = exportDiagnostics,
                        onOpenLicenses = { nav.navigate("licenses") },
                        permissions = permissionRows(),
                        languages = languageChoices,
                        language = AppLanguages.current(),
                        onLanguageChange = ::setLanguage,
                        onOpenAuditLog = { nav.navigate("audit") },
                    )
                }
                composable("audio") { AudioScreen(state) }
                composable("audit") { AuditLogScreen() }
                composable("troubleshoot") {
                    TroubleshooterScreen(onOpenTopic = { nav.navigate("troubleshoot/$it") }, onExportDiagnostics = exportDiagnostics)
                }
                composable("troubleshoot/{topic}") { entry ->
                    TroubleshootTopicScreen(
                        topicKey = entry.arguments?.getString("topic").orEmpty(),
                        state = state,
                        onOpenDevices = { nav.navigateToTab("devices") },
                        onOpenHome = { nav.navigateToTab("home") },
                        onOpenBatteryGuide = { nav.navigate("battery") },
                        onExportDiagnostics = exportDiagnostics,
                    )
                }
                composable("battery") { BatteryGuideScreen() }
                composable("licenses") { LicensesScreen() }
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

        if (showRail) SideRail(route) { destination -> nav.navigateToTab(destination) }

        Overlays(state)
        PermissionOverlays()

        if (crashNotice) {
            val close = {
                crashNotice = false
                scope.launch(Dispatchers.IO) { CrashReports.markSeen(context) }
            }
            AlertDialog(
                onDismissRequest = { close() },
                icon = { Icon(SpIcons.Alert, null) },
                title = { Text(stringResource(R.string.crash_title), textAlign = TextAlign.Center) },
                text = { Text(stringResource(R.string.crash_body)) },
                confirmButton = {
                    Button(onClick = {
                        close()
                        exportDiagnostics()
                    }) { Text(stringResource(R.string.settings_diagnostics)) }
                },
                dismissButton = { TextButton(onClick = { close() }) { Text(stringResource(R.string.common_close)) } },
            )
        } else if (notificationRationale) {
            AlertDialog(
                onDismissRequest = { notificationRationale = false },
                icon = { Icon(SpIcons.Alert, null) },
                title = { Text(stringResource(R.string.notif_rationale_title), textAlign = TextAlign.Center) },
                text = { Text(stringResource(R.string.notif_rationale_body)) },
                confirmButton = {
                    Button(onClick = {
                        notificationRationale = false
                        requestNotifications()
                    }) { Text(stringResource(R.string.notif_rationale_allow)) }
                },
                dismissButton = {
                    TextButton(onClick = { notificationRationale = false }) { Text(stringResource(R.string.common_not_now)) }
                },
            )
        }
    }

    /** Denied and blocked permission states (plan §26.1): each says what stopped working and how to fix it. */
    @Composable
    private fun PermissionOverlays() {
        when (blockedPermission) {
            Manifest.permission.RECORD_AUDIO -> PermissionBlockedDialog(
                title = stringResource(R.string.perm_mic_blocked_title),
                body = stringResource(R.string.perm_mic_blocked_body),
                onOpenSettings = ::openAppSettings,
                onDismiss = { blockedPermission = null },
            )
            Manifest.permission.CAMERA -> PermissionBlockedDialog(
                title = stringResource(R.string.perm_camera_blocked_title),
                body = stringResource(R.string.perm_camera_blocked_body),
                onOpenSettings = ::openAppSettings,
                onDismiss = { blockedPermission = null },
            )
            // Notifications have their own banner and settings row; nothing else is tracked.
            else -> {}
        }
        captureRefused?.let { refused ->
            AlertDialog(
                onDismissRequest = { captureRefused = null },
                icon = { Icon(SpIcons.Apps, null) },
                title = { Text(stringResource(R.string.perm_capture_refused_title), textAlign = TextAlign.Center) },
                text = { Text(stringResource(R.string.perm_capture_refused_body)) },
                confirmButton = {
                    Button(onClick = {
                        captureRefused = null
                        askCaptureConsent(refused)
                    }) { Text(stringResource(R.string.common_retry)) }
                },
                dismissButton = { TextButton(onClick = { captureRefused = null }) { Text(stringResource(R.string.common_cancel)) } },
            )
        }
    }

    /** Settings → Permissions: microphone, camera and notifications with what the user can do about each. */
    @Composable
    private fun permissionRows(): List<PermissionRow> {
        @Composable
        fun row(@StringRes label: Int, state: PermissionState, onClick: () -> Unit) = PermissionRow(
            label = stringResource(label),
            status = stringResource(
                when (state) {
                    PermissionState.Granted -> R.string.perm_status_granted
                    PermissionState.NotAsked -> R.string.perm_status_not_asked
                    PermissionState.Denied -> R.string.perm_status_denied
                    PermissionState.Blocked -> R.string.perm_status_blocked
                },
            ),
            needsAction = state != PermissionState.Granted,
            onClick = onClick,
        )
        return listOf(
            row(R.string.perm_label_mic, micState) { askFromSettings(Manifest.permission.RECORD_AUDIO) },
            row(R.string.perm_label_camera, cameraState) { askFromSettings(Manifest.permission.CAMERA) },
            row(R.string.perm_label_notifications, notificationState) { requestNotifications() },
        )
    }

    /**
     * Contextual banners on Home, each with one fix and (for tips) Dismiss, remembered in
     * settings.dismissedTips like the desktop's tips.
     */
    @Composable
    private fun homeBanners(state: EngineState): List<BannerModel> {
        val output by DeviceStatus.output.collectAsState()
        val network by DeviceStatus.network.collectAsState()
        val measuredMs by DeviceStatus.outputLatencyMs.collectAsState()
        val tips = state.settings.dismissedTips
        val receiving = state.routes.any { !it.isSending && it.status != "stopped" }
        val dismiss = stringResource(R.string.common_dismiss)
        return buildList {
            if (state.routes.isNotEmpty() && !notificationsEnabled) {
                add(
                    BannerModel(
                        key = "notifications",
                        title = stringResource(R.string.home_banner_notifications),
                        message = stringResource(R.string.home_banner_notifications_body),
                        actionLabel = stringResource(
                            if (notificationState == PermissionState.Blocked) R.string.common_open_settings else R.string.notif_rationale_allow,
                        ),
                        onAction = ::requestNotifications,
                        icon = SpIcons.Alert,
                        warning = true,
                    ),
                )
            }
            if (receiving && output == DeviceStatus.Output.Bluetooth && TIP_BLUETOOTH !in tips) {
                val stable = state.settings.stream.latency == "stable"
                add(
                    BannerModel(
                        key = TIP_BLUETOOTH,
                        title = if (measuredMs > 0) stringResource(R.string.hint_bluetooth_measured, measuredMs) else stringResource(R.string.hint_bluetooth_title),
                        message = stringResource(R.string.hint_bluetooth_body),
                        actionLabel = if (stable) null else stringResource(R.string.hint_use_stable),
                        onAction = { SoundPush.updateSettings { it.copy(stream = it.stream.copy(latency = "stable")) } },
                        icon = SpIcons.Bluetooth,
                        dismissLabel = dismiss,
                        onDismiss = { dismissTip(TIP_BLUETOOTH) },
                    ),
                )
            }
            if (network.sharesMobileData && TIP_TETHER_DATA !in tips) {
                add(
                    BannerModel(
                        key = TIP_TETHER_DATA,
                        title = stringResource(R.string.hint_tether_data_title),
                        message = stringResource(R.string.hint_tether_data_body),
                        actionLabel = stringResource(R.string.common_open_settings),
                        onAction = {
                            openSettings(
                                Intent().setComponent(ComponentName("com.android.settings", "com.android.settings.TetherSettings")),
                                Intent(AndroidSettings.ACTION_WIRELESS_SETTINGS),
                            )
                        },
                        icon = SpIcons.Usb,
                        warning = true,
                        dismissLabel = dismiss,
                        onDismiss = { dismissTip(TIP_TETHER_DATA) },
                    ),
                )
            }
            if (state.routes.isNotEmpty() && network.mobileDataOnly && TIP_MOBILE_DATA !in tips) {
                add(
                    BannerModel(
                        key = TIP_MOBILE_DATA,
                        title = stringResource(R.string.hint_mobile_data_title),
                        message = stringResource(R.string.hint_mobile_data_body),
                        icon = SpIcons.Wifi,
                        warning = true,
                        dismissLabel = dismiss,
                        onDismiss = { dismissTip(TIP_MOBILE_DATA) },
                    ),
                )
            }
        }
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
                            textAlign = TextAlign.Center,
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

    /**
     * An engine error with its stable support code ("… (SP-NET-004)", plan §36.4), so what a user
     * reports names one failure whatever language the app is in. Codes: docs/error-codes.md.
     */
    private fun errorText(context: Context, error: ErrorView): String {
        val message = context.getString(Labels.error(error.key))
        return if (error.code.isEmpty()) message else context.getString(R.string.error_with_code, message, error.code)
    }

    private companion object {
        const val KEY_PENDING = "pending"
        const val PREFS = "soundpush_local"
        const val KEY_NOTIFICATIONS_ASKED = "notificationsAsked"
        const val ONBOARDING_TIP = "onboarding"
        const val TIP_BLUETOOTH = "bluetoothLatency"
        const val TIP_TETHER_DATA = "usbTetherData"
        const val TIP_MOBILE_DATA = "mobileData"
    }
}
