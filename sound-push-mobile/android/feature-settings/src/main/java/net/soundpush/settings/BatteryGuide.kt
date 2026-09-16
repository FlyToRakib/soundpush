package net.soundpush.settings

import android.annotation.SuppressLint
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.PowerManager
import androidx.annotation.StringRes
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.LifecycleResumeEffect
import net.soundpush.ui.R
import net.soundpush.ui.components.NavRow
import net.soundpush.ui.components.SectionTitle
import net.soundpush.ui.components.SpCard
import net.soundpush.ui.theme.Tokens
import android.provider.Settings as AndroidSettings

/** Phone makers whose battery managers are known to stop background apps (see dontkillmyapp.com). */
enum class PhoneMaker { Xiaomi, Samsung, Huawei, Oppo, OnePlus, Generic }

/**
 * Per-manufacturer "keep SoundPush running" guides: the steps in words, plus deep links to the
 * matching settings screens. OEM screens move between OS versions, so every link tries several
 * known screens and falls back to the app's own settings page.
 */
object BatteryGuides {
    fun detect(manufacturer: String = Build.MANUFACTURER, brand: String = Build.BRAND): PhoneMaker {
        val name = "$manufacturer $brand".lowercase()
        return when {
            listOf("xiaomi", "redmi", "poco").any { it in name } -> PhoneMaker.Xiaomi
            "samsung" in name -> PhoneMaker.Samsung
            listOf("huawei", "honor").any { it in name } -> PhoneMaker.Huawei
            // OnePlus before Oppo: newer OnePlus phones report Oppo parts but keep their own settings.
            "oneplus" in name -> PhoneMaker.OnePlus
            listOf("oppo", "realme").any { it in name } -> PhoneMaker.Oppo
            else -> PhoneMaker.Generic
        }
    }

    /** "Xiaomi", "Samsung"… for sentences such as "Xiaomi phones may stop apps". */
    fun makerName(): String = Build.MANUFACTURER.replaceFirstChar { it.uppercase() }

    @StringRes
    fun steps(maker: PhoneMaker): List<Int> = when (maker) {
        PhoneMaker.Xiaomi -> listOf(R.string.battery_xiaomi_1, R.string.battery_xiaomi_2, R.string.battery_xiaomi_3)
        PhoneMaker.Samsung -> listOf(R.string.battery_samsung_1, R.string.battery_samsung_2)
        PhoneMaker.Huawei -> listOf(R.string.battery_huawei_1, R.string.battery_huawei_2)
        PhoneMaker.Oppo -> listOf(R.string.battery_oppo_1, R.string.battery_oppo_2)
        PhoneMaker.OnePlus -> listOf(R.string.battery_oneplus_1, R.string.battery_oneplus_2)
        PhoneMaker.Generic -> listOf(R.string.battery_generic_1)
    }

    data class Link(@param:StringRes val label: Int, val intents: (Context) -> List<Intent>)

    fun links(maker: PhoneMaker): List<Link> {
        val app = Link(R.string.battery_open_app) { listOf(appDetails(it)) }
        return when (maker) {
            PhoneMaker.Xiaomi -> listOf(
                Link(R.string.battery_open_autostart) {
                    listOf(component("com.miui.securitycenter", "com.miui.permcenter.autostart.AutoStartManagementActivity"))
                },
                Link(R.string.battery_open_battery) { ctx ->
                    listOf(
                        component("com.miui.powerkeeper", "com.miui.powerkeeper.ui.HiddenAppsConfigActivity")
                            .putExtra("package_name", ctx.packageName)
                            .putExtra("package_label", ctx.getString(R.string.app_name)),
                    )
                },
                app,
            )
            PhoneMaker.Samsung -> listOf(
                Link(R.string.battery_open_battery) {
                    listOf(
                        component("com.samsung.android.lool", "com.samsung.android.sm.battery.ui.BatteryActivity"),
                        component("com.samsung.android.lool", "com.samsung.android.sm.ui.battery.BatteryActivity"),
                    )
                },
                app,
            )
            PhoneMaker.Huawei -> listOf(
                Link(R.string.battery_open_autostart) {
                    listOf(
                        component("com.huawei.systemmanager", "com.huawei.systemmanager.startupmgr.ui.StartupNormalAppListActivity"),
                        component("com.huawei.systemmanager", "com.huawei.systemmanager.optimize.process.ProtectActivity"),
                    )
                },
                app,
            )
            PhoneMaker.Oppo -> listOf(
                Link(R.string.battery_open_autostart) {
                    listOf(
                        component("com.coloros.safecenter", "com.coloros.safecenter.permission.startup.StartupAppListActivity"),
                        component("com.coloros.safecenter", "com.coloros.safecenter.startupapp.StartupAppListActivity"),
                        component("com.oppo.safe", "com.oppo.safe.permission.startup.StartupAppListActivity"),
                    )
                },
                app,
            )
            PhoneMaker.OnePlus -> listOf(
                Link(R.string.battery_open_autostart) {
                    listOf(component("com.oneplus.security", "com.oneplus.security.chainlaunch.view.ChainLaunchAppListActivity"))
                },
                Link(R.string.battery_open_battery) { listOf(Intent(AndroidSettings.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS)) },
                app,
            )
            PhoneMaker.Generic -> listOf(
                Link(R.string.battery_open_battery) { listOf(Intent(AndroidSettings.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS)) },
                app,
            )
        }
    }

    fun isUnrestricted(context: Context): Boolean = context.getSystemService(PowerManager::class.java)?.isIgnoringBatteryOptimizations(context.packageName) == true

    /**
     * Ask Android to exempt SoundPush from battery optimisation. Only offered when the user wants
     * long background use (Stay available, this guide); a companion streaming app qualifies.
     */
    @SuppressLint("BatteryLife")
    fun requestUnrestricted(context: Context) {
        open(
            context,
            listOf(
                Intent(AndroidSettings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS, Uri.fromParts("package", context.packageName, null)),
                Intent(AndroidSettings.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS),
                appDetails(context),
            ),
        )
    }

    fun appDetails(context: Context) = Intent(AndroidSettings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.fromParts("package", context.packageName, null))

    /** Start the first intent that opens; OEM screens may be missing or not exported. */
    fun open(context: Context, intents: List<Intent>): Boolean {
        for (intent in intents) {
            if (runCatching { context.startActivity(intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)) }.isSuccess) return true
        }
        return false
    }

    private fun component(pkg: String, cls: String) = Intent().setComponent(ComponentName(pkg, cls))
}

@Composable
fun BatteryGuideScreen() {
    val context = LocalContext.current
    val maker = remember { BatteryGuides.detect() }
    // Re-check when the user comes back from a settings screen.
    var resumes by remember { mutableIntStateOf(0) }
    LifecycleResumeEffect(Unit) {
        resumes++
        onPauseOrDispose { }
    }
    val unrestricted = remember(resumes) { BatteryGuides.isUnrestricted(context) }

    Column(
        Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(start = Tokens.Space.md, end = Tokens.Space.md, bottom = Tokens.Space.lg),
        verticalArrangement = Arrangement.spacedBy(Tokens.Space.xs),
    ) {
        Text(
            stringResource(R.string.battery_intro),
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(top = Tokens.Space.sm, start = 4.dp, end = 4.dp),
        )
        SpCard(Modifier.padding(top = Tokens.Space.md)) {
            if (unrestricted) {
                CheckRow(Check(CheckStatus.Ok, stringResource(R.string.trouble_battery_ok)))
            } else {
                CheckRow(
                    Check(CheckStatus.Problem, stringResource(R.string.trouble_battery_restricted), stringResource(R.string.trouble_battery_allow)) {
                        BatteryGuides.requestUnrestricted(context)
                    },
                )
            }
        }

        SectionTitle(stringResource(R.string.battery_steps))
        SpCard {
            BatteryGuides.steps(maker).forEachIndexed { i, step ->
                if (i > 0) HorizontalDivider(color = MaterialTheme.colorScheme.outline)
                Row(Modifier.padding(vertical = Tokens.Space.sm)) {
                    // The number is visual; TalkBack reads the steps in order.
                    Text(
                        "${i + 1}.",
                        style = MaterialTheme.typography.bodyLarge,
                        color = MaterialTheme.colorScheme.primary,
                        modifier = Modifier.widthIn(min = 24.dp).clearAndSetSemantics { },
                    )
                    Text(stringResource(step), style = MaterialTheme.typography.bodyLarge)
                }
            }
        }
        SpCard(Modifier.padding(top = Tokens.Space.sm)) {
            BatteryGuides.links(maker).forEach { link ->
                NavRow(stringResource(link.label)) { BatteryGuides.open(context, link.intents(context)) }
            }
            NavRow(stringResource(R.string.battery_learn_more)) {
                BatteryGuides.open(context, listOf(Intent(Intent.ACTION_VIEW, Uri.parse("https://dontkillmyapp.com/${maker.name.lowercase()}"))))
            }
        }
    }
}
