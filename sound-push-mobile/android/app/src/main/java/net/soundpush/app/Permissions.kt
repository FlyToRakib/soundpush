package net.soundpush.app

import android.content.SharedPreferences
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextAlign
import net.soundpush.ui.R
import net.soundpush.ui.icons.SpIcons

/** What the user can do about one runtime permission (plan §26.1). */
enum class PermissionState {
    Granted,

    /** Never refused: asking shows the system dialog. */
    NotAsked,

    /** Refused once; asking again still shows the system dialog. */
    Denied,

    /** Refused for good ("Don't ask again", or twice on Android 11+): only app settings can change it. */
    Blocked,
    ;

    companion object {
        /**
         * Android offers no direct "blocked" query. After a refusal it asks the app to explain
         * (rationale); once refused for good it stops asking, and the system dialog never appears
         * again. A permission that was refused before and needs no explanation is therefore blocked.
         */
        fun of(granted: Boolean, refusedBefore: Boolean, shouldShowRationale: Boolean): PermissionState = when {
            granted -> Granted
            shouldShowRationale -> Denied
            refusedBefore -> Blocked
            else -> NotAsked
        }
    }
}

/** Remembers which permissions the user has refused, so a blocked one can be told apart from a new one. */
internal class PermissionMemory(private val prefs: SharedPreferences) {
    fun refusedBefore(permission: String): Boolean = prefs.getBoolean(KEY_PREFIX + permission, false)

    fun record(permission: String, granted: Boolean) {
        if (refusedBefore(permission) != !granted) prefs.edit().putBoolean(KEY_PREFIX + permission, !granted).apply()
    }

    private companion object {
        const val KEY_PREFIX = "refused."
    }
}

/** A permission was refused for good: say what stopped working and open the app's settings. */
@Composable
internal fun PermissionBlockedDialog(title: String, body: String, onOpenSettings: () -> Unit, onDismiss: () -> Unit) {
    AlertDialog(
        onDismissRequest = onDismiss,
        icon = { Icon(SpIcons.Alert, null) },
        title = { Text(title, textAlign = TextAlign.Center) },
        text = { Text(body) },
        confirmButton = {
            Button(onClick = {
                onDismiss()
                onOpenSettings()
            }) { Text(stringResource(R.string.common_open_settings)) }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.common_not_now)) } },
    )
}
