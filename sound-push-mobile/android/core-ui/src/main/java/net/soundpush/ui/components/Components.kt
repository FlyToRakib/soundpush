package net.soundpush.ui.components

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Slider
import androidx.compose.material3.SliderDefaults
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.SwitchDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import kotlin.math.roundToInt
import net.soundpush.ui.R
import net.soundpush.ui.icons.SpIcons
import net.soundpush.ui.theme.LocalSpColors
import net.soundpush.ui.theme.Tokens

/** Screen title that respects the real status-bar height, with an optional back arrow. */
@Composable
fun ScreenHeader(title: String, onBack: (() -> Unit)? = null) {
    Row(
        Modifier
            .fillMaxWidth()
            .statusBarsPadding()
            .heightIn(min = 64.dp)
            .padding(start = if (onBack == null) Tokens.Space.md else 4.dp, end = Tokens.Space.md),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (onBack != null) {
            IconButton(onClick = onBack) { Icon(SpIcons.Back, stringResource(R.string.common_back)) }
        }
        Text(
            title,
            style = MaterialTheme.typography.headlineSmall.copy(fontWeight = FontWeight.SemiBold),
            modifier = Modifier.semantics { heading() },
        )
    }
}

@Composable
fun SectionTitle(text: String, modifier: Modifier = Modifier) {
    Text(
        text,
        style = MaterialTheme.typography.titleMedium,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = modifier
            .padding(top = Tokens.Space.lg, bottom = Tokens.Space.sm, start = 4.dp)
            .semantics { heading() },
    )
}

@Composable
fun SpCard(modifier: Modifier = Modifier, content: @Composable () -> Unit) {
    Surface(
        modifier = modifier.fillMaxWidth(),
        shape = MaterialTheme.shapes.medium,
        color = MaterialTheme.colorScheme.surface,
        border = BorderStroke(1.dp, MaterialTheme.colorScheme.outline),
    ) {
        Column(Modifier.padding(horizontal = Tokens.Space.md, vertical = Tokens.Space.sm)) { content() }
    }
}

/** [filled]: solid accent tile, used for the one task that is running so it stands out from the tinted ones. */
@Composable
fun IconTile(icon: ImageVector, active: Boolean = true, filled: Boolean = false) {
    val background = when {
        filled -> MaterialTheme.colorScheme.primary
        active -> MaterialTheme.colorScheme.primaryContainer
        else -> MaterialTheme.colorScheme.surfaceVariant
    }
    val tint = when {
        filled -> MaterialTheme.colorScheme.onPrimary
        active -> MaterialTheme.colorScheme.primary
        else -> MaterialTheme.colorScheme.onSurfaceVariant
    }
    Box(
        Modifier.size(44.dp).clip(RoundedCornerShape(12.dp)).background(background),
        contentAlignment = Alignment.Center,
    ) {
        Icon(icon, contentDescription = null, tint = tint)
    }
}

/**
 * Home task. Stays tappable when unavailable so it can explain why; looks muted and shows the reason.
 * When [active] a route for this task is running: accent outline and tint, a solid icon tile and an
 * "Active" badge in place of the chevron, so the running task is obvious in the list.
 */
@Composable
fun TaskCard(
    title: String,
    description: String,
    icon: ImageVector,
    enabled: Boolean,
    active: Boolean = false,
    onClick: () -> Unit,
) {
    val accent = MaterialTheme.colorScheme.primary
    val muted = !enabled && !active
    val stateText = when {
        active -> stringResource(R.string.a11y_active)
        muted -> stringResource(R.string.a11y_unavailable)
        else -> null
    }
    Surface(
        onClick = onClick,
        shape = MaterialTheme.shapes.medium,
        color = if (active) MaterialTheme.colorScheme.primaryContainer else MaterialTheme.colorScheme.surface,
        border = BorderStroke(if (active) 1.5.dp else 1.dp, if (active) accent else MaterialTheme.colorScheme.outline),
        // Stays enabled when unavailable (tapping explains why), so say so instead of "disabled".
        modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp).semantics {
            role = Role.Button
            selected = active
            if (stateText != null) stateDescription = stateText
        },
    ) {
        Row(Modifier.padding(Tokens.Space.md), verticalAlignment = Alignment.CenterVertically) {
            IconTile(icon, active = !muted, filled = active)
            Spacer(Modifier.width(Tokens.Space.md))
            Column(Modifier.weight(1f)) {
                Text(
                    title,
                    style = MaterialTheme.typography.titleMedium,
                    color = if (muted) MaterialTheme.colorScheme.onSurfaceVariant else MaterialTheme.colorScheme.onSurface,
                )
                Text(
                    description,
                    style = MaterialTheme.typography.bodySmall,
                    color = if (active) accent else MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            if (active) {
                ActiveBadge(stringResource(R.string.home_task_active_badge))
            } else {
                Icon(SpIcons.Chevron, null, tint = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.size(18.dp))
            }
        }
    }
}

/** Small accent pill with a live dot, e.g. "Active". Decorative: the card's caption already says what runs. */
@Composable
private fun ActiveBadge(label: String) {
    Row(
        Modifier
            .clip(RoundedCornerShape(999.dp))
            .background(MaterialTheme.colorScheme.primary)
            .padding(horizontal = 10.dp, vertical = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(Modifier.size(6.dp).clip(RoundedCornerShape(3.dp)).background(MaterialTheme.colorScheme.onPrimary))
        Spacer(Modifier.width(6.dp))
        Text(label, style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.onPrimary)
    }
}

/** Clear on/off contrast in both themes: accent track when on, neutral track with a visible thumb when off. */
@Composable
fun spSwitchColors() = SwitchDefaults.colors(
    checkedThumbColor = androidx.compose.ui.graphics.Color.White,
    checkedTrackColor = MaterialTheme.colorScheme.primary,
    checkedBorderColor = MaterialTheme.colorScheme.primary,
    checkedIconColor = MaterialTheme.colorScheme.primary,
    uncheckedThumbColor = MaterialTheme.colorScheme.onSurfaceVariant,
    uncheckedTrackColor = MaterialTheme.colorScheme.surfaceVariant,
    uncheckedBorderColor = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.5f),
    disabledUncheckedThumbColor = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.38f),
    disabledUncheckedTrackColor = MaterialTheme.colorScheme.surfaceVariant,
    disabledUncheckedBorderColor = MaterialTheme.colorScheme.outline,
)

/** A full-row switch: the whole row is the touch target and is announced as one switch. */
@Composable
fun SettingSwitch(
    label: String,
    checked: Boolean,
    description: String? = null,
    enabled: Boolean = true,
    onChange: (Boolean) -> Unit,
) {
    Row(
        Modifier
            .fillMaxWidth()
            .heightIn(min = 56.dp)
            .toggleable(value = checked, enabled = enabled, role = Role.Switch, onValueChange = onChange)
            .padding(vertical = Tokens.Space.xs),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f).padding(end = Tokens.Space.md)) {
            Text(
                label,
                style = MaterialTheme.typography.bodyLarge,
                color = if (enabled) MaterialTheme.colorScheme.onSurface else MaterialTheme.colorScheme.onSurfaceVariant,
            )
            if (description != null) {
                Text(description, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        }
        Switch(checked = checked, onCheckedChange = null, enabled = enabled, colors = spSwitchColors())
    }
}

/**
 * Slider that updates its label live while dragging but only reports the final value,
 * so settings are not rewritten on every drag step.
 */
@Composable
fun SettingSlider(
    label: String,
    value: Float,
    range: ClosedFloatingPointRange<Float>,
    steps: Int = 0,
    format: (Float) -> String,
    onValueChangeFinished: (Float) -> Unit,
) {
    var local by remember(value) { mutableFloatStateOf(value) }
    Column(Modifier.fillMaxWidth().padding(vertical = Tokens.Space.xs)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(label, style = MaterialTheme.typography.bodyLarge, modifier = Modifier.weight(1f))
            Text(format(local), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        Slider(
            value = local,
            onValueChange = { local = it },
            onValueChangeFinished = { onValueChangeFinished(local) },
            valueRange = range,
            steps = steps,
            colors = SliderDefaults.colors(inactiveTrackColor = MaterialTheme.colorScheme.surfaceVariant),
            // Announce the formatted value ("+6 dB", "120%") rather than a raw percentage.
            modifier = Modifier.semantics {
                contentDescription = label
                stateDescription = format(local)
            },
        )
    }
}

data class Choice(val value: String, val label: String)

@Composable
fun SettingChoice(label: String, value: String, choices: List<Choice>, onChange: (String) -> Unit) {
    var open by remember { mutableStateOf(false) }
    val current = choices.firstOrNull { it.value == value }?.label ?: value
    Box {
        Row(
            Modifier
                .fillMaxWidth()
                .heightIn(min = 56.dp)
                .clickable(role = Role.DropdownList) { open = true }
                .semantics(mergeDescendants = true) { stateDescription = current },
            verticalAlignment = Alignment.CenterVertically,
        ) {
            // Both sides may wrap at large font sizes; neither can push the other off screen.
            Text(label, style = MaterialTheme.typography.bodyLarge, modifier = Modifier.weight(1f).padding(end = Tokens.Space.md))
            Text(
                current,
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.primary,
                textAlign = androidx.compose.ui.text.style.TextAlign.End,
                modifier = Modifier.weight(1f, fill = false).clearAndSetSemantics { },
            )
        }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            choices.forEach { choice ->
                DropdownMenuItem(
                    text = { Text(choice.label) },
                    onClick = {
                        open = false
                        onChange(choice.value)
                    },
                    trailingIcon = if (choice.value == value) ({ Icon(SpIcons.Check, null) }) else null,
                    modifier = Modifier.semantics { selected = choice.value == value },
                )
            }
        }
    }
}

/** A row that opens another screen or system setting. */
@Composable
fun NavRow(label: String, description: String? = null, onClick: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().heightIn(min = 56.dp).clickable(role = Role.Button, onClick = onClick).padding(vertical = Tokens.Space.xs),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f).padding(end = Tokens.Space.md)) {
            Text(label, style = MaterialTheme.typography.bodyLarge)
            if (description != null) {
                Text(description, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        }
        Icon(SpIcons.Chevron, null, tint = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.size(18.dp))
    }
}

/** A contextual banner (problem or tip) with one fix action and, for tips, Dismiss. */
data class BannerModel(
    val key: String,
    val title: String,
    val message: String? = null,
    val actionLabel: String? = null,
    val onAction: (() -> Unit)? = null,
    val icon: ImageVector = SpIcons.Info,
    val warning: Boolean = false,
    val dismissLabel: String? = null,
    val onDismiss: (() -> Unit)? = null,
)

@Composable
fun StatusBanner(model: BannerModel) = StatusBanner(
    title = model.title,
    message = model.message,
    actionLabel = model.actionLabel,
    onAction = model.onAction,
    icon = model.icon,
    warning = model.warning,
    dismissLabel = model.dismissLabel,
    onDismiss = model.onDismiss,
)

/**
 * Connection-status banner with one clear action. With [onDismiss] the actions move under the text,
 * so two buttons still fit at large font sizes. Announced politely when it appears.
 */
@Composable
fun StatusBanner(
    title: String,
    message: String?,
    actionLabel: String?,
    onAction: (() -> Unit)?,
    icon: ImageVector = SpIcons.Wifi,
    warning: Boolean = false,
    dismissLabel: String? = null,
    onDismiss: (() -> Unit)? = null,
) {
    val tint = if (warning) LocalSpColors.current.warning else MaterialTheme.colorScheme.primary
    val hasAction = actionLabel != null && onAction != null
    val stacked = dismissLabel != null && onDismiss != null
    Surface(
        shape = MaterialTheme.shapes.medium,
        color = if (warning) LocalSpColors.current.warning.copy(alpha = 0.12f) else MaterialTheme.colorScheme.secondaryContainer,
        modifier = Modifier.fillMaxWidth(),
    ) {
        Column(Modifier.padding(start = Tokens.Space.md, end = Tokens.Space.xs, top = 12.dp, bottom = if (stacked) 4.dp else 12.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Icon(icon, null, tint = tint)
                Spacer(Modifier.width(12.dp))
                Column(
                    Modifier
                        .weight(1f)
                        .padding(end = Tokens.Space.xs)
                        .semantics(mergeDescendants = true) { liveRegion = LiveRegionMode.Polite },
                ) {
                    Text(title, style = MaterialTheme.typography.titleMedium, color = MaterialTheme.colorScheme.onSurface)
                    if (message != null) {
                        Text(message, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                }
                if (hasAction && !stacked) TextButton(onClick = onAction!!) { Text(actionLabel!!) }
            }
            if (stacked) {
                Row(Modifier.align(Alignment.End)) {
                    TextButton(onClick = onDismiss!!) { Text(dismissLabel!!) }
                    if (hasAction) TextButton(onClick = onAction!!) { Text(actionLabel!!) }
                }
            }
        }
    }
}

/**
 * Live input level, −60…0 dBFS. [clipping] means the soft limiter after the volume boost is
 * working (plan §8.3), so the bar turns red and the announcement says so instead of only
 * looking different.
 */
@Composable
fun LevelMeter(label: String, db: Float, clipping: Boolean = false, modifier: Modifier = Modifier) {
    val rounded = db.coerceIn(-60f, 0f).roundToInt()
    val fraction = ((rounded + 60) / 60f).coerceIn(0f, 1f)
    val level = stringResource(R.string.a11y_level, rounded)
    val state = if (clipping) stringResource(R.string.a11y_level_clipping, level) else level
    Row(
        modifier
            .fillMaxWidth()
            .padding(vertical = Tokens.Space.xs)
            .semantics(mergeDescendants = true) {
                contentDescription = label
                stateDescription = state
                liveRegion = LiveRegionMode.Polite
            },
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(
            Modifier
                .weight(1f)
                .height(6.dp)
                .clip(RoundedCornerShape(3.dp))
                .background(MaterialTheme.colorScheme.surfaceVariant),
        ) {
            Box(
                Modifier
                    .fillMaxHeight()
                    .fillMaxWidth(fraction)
                    .background(if (clipping || rounded > -3) MaterialTheme.colorScheme.error else LocalSpColors.current.success),
            )
        }
        if (clipping) {
            Spacer(Modifier.width(Tokens.Space.sm))
            Text(
                stringResource(R.string.audio_clipping),
                style = MaterialTheme.typography.labelLarge,
                color = MaterialTheme.colorScheme.error,
            )
        }
    }
}

@Composable
fun QualityBadge(quality: String, label: String, latencyMs: Double = 0.0) {
    if (quality == "unknown" || label.isEmpty()) return
    val color = if (quality == "poor") LocalSpColors.current.warning else LocalSpColors.current.success
    Row(
        verticalAlignment = Alignment.CenterVertically,
        modifier = Modifier.semantics(mergeDescendants = true) { contentDescription = label },
    ) {
        Box(Modifier.size(8.dp).clip(RoundedCornerShape(4.dp)).background(color))
        Spacer(Modifier.width(6.dp))
        val text = if (latencyMs > 0) stringResource(R.string.quality_with_latency, label, latencyMs.toInt()) else label
        Text(text, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}

@Composable
fun EmptyState(
    icon: ImageVector,
    title: String,
    body: String,
    action: String,
    onAction: () -> Unit,
    secondaryAction: String? = null,
    onSecondaryAction: (() -> Unit)? = null,
) {
    Column(
        Modifier.fillMaxWidth().padding(vertical = 56.dp, horizontal = Tokens.Space.lg),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(Tokens.Space.sm),
    ) {
        IconTile(icon)
        Spacer(Modifier.size(Tokens.Space.xs))
        Text(title, style = MaterialTheme.typography.titleLarge)
        Text(
            body,
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            textAlign = androidx.compose.ui.text.style.TextAlign.Center,
        )
        Spacer(Modifier.size(Tokens.Space.sm))
        Button(onClick = onAction) { Text(action) }
        if (secondaryAction != null && onSecondaryAction != null) {
            TextButton(onClick = onSecondaryAction) { Text(secondaryAction) }
        }
    }
}

fun formatElapsed(secs: Long): String {
    val h = secs / 3600
    val m = (secs % 3600) / 60
    val s = secs % 60
    return if (h > 0) "%d:%02d:%02d".format(h, m, s) else "%d:%02d".format(m, s)
}
