package net.soundpush.settings

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import net.soundpush.engine.Caches
import java.net.HttpURLConnection
import java.net.URL

/** Public project pages the app links to (opened in the browser). */
internal object ProjectLinks {
    private const val REPO = "https://github.com/FlyToRakib/soundpush"
    const val RELEASES = "$REPO/releases/latest"
    const val USER_GUIDE = "$REPO/blob/HEAD/docs/user-guide.md"
    const val PRIVACY = "$REPO/blob/HEAD/PRIVACY.md"
    const val LICENSE = "$REPO/blob/HEAD/LICENSE"
    const val REPORT_BUG = "$REPO/issues/new/choose"
    const val SOURCE = REPO
    const val LATEST_API = "https://api.github.com/repos/FlyToRakib/soundpush/releases/latest"
}

/**
 * Register this module's caches so `onTrimMemory` can drop them (plan §8.4). Called once from the
 * application; the update check simply asks GitHub again next time.
 */
fun registerSettingsCaches() {
    Caches.onTrim { UpdateChecker.forget() }
}

internal sealed interface UpdateStatus {
    data object Idle : UpdateStatus
    data object Checking : UpdateStatus
    data object UpToDate : UpdateStatus
    data class Available(val version: String, val url: String) : UpdateStatus
    data object Failed : UpdateStatus
}

/**
 * Lightweight check against GitHub Releases. It never downloads or installs anything (no Play Core, F-Droid
 * friendly); it only tells the user a newer version exists and where to get it.
 */
internal object UpdateChecker {
    /** Last successful result in this process, so reopening Settings does not ask GitHub again. */
    @Volatile var cached: UpdateStatus? = null
        private set

    /** Drop the remembered result (memory pressure); the next check asks GitHub again. */
    fun forget() {
        cached = null
    }

    /**
     * Automatic checks ([manual] false) run once per app process and fail silently (offline, rate-limited);
     * manual checks always ask and report failures.
     */
    suspend fun check(currentVersion: String, manual: Boolean): UpdateStatus {
        if (!manual) cached?.let { return it }
        return try {
            val (tag, url) = fetchLatest()
            val result = if (isNewer(tag, currentVersion)) UpdateStatus.Available(tag.removePrefix("v"), url) else UpdateStatus.UpToDate
            cached = result
            if (manual || result is UpdateStatus.Available) result else UpdateStatus.Idle
        } catch (e: Exception) {
            if (manual) UpdateStatus.Failed else UpdateStatus.Idle
        }
    }

    private suspend fun fetchLatest(): Pair<String, String> = withContext(Dispatchers.IO) {
        val connection = URL(ProjectLinks.LATEST_API).openConnection() as HttpURLConnection
        try {
            connection.connectTimeout = 10_000
            connection.readTimeout = 10_000
            connection.setRequestProperty("Accept", "application/vnd.github+json")
            connection.setRequestProperty("User-Agent", "SoundPush-Android")
            check(connection.responseCode == HttpURLConnection.HTTP_OK) { "HTTP ${connection.responseCode}" }
            val body = connection.inputStream.bufferedReader().use { it.readText() }
            val release = Json.parseToJsonElement(body).jsonObject
            val tag = requireNotNull(release["tag_name"]?.jsonPrimitive?.content)
            val url = release["html_url"]?.jsonPrimitive?.content ?: ProjectLinks.RELEASES
            tag to url
        } finally {
            connection.disconnect()
        }
    }

    /** True when [latest] ("v1.2.0") is a higher version than [current] ("1.1.9"). Pre-releases rank below releases. */
    fun isNewer(latest: String, current: String): Boolean {
        val a = parse(latest) ?: return false
        val b = parse(current) ?: return false
        for (i in 0 until 3) {
            if (a.numbers[i] != b.numbers[i]) return a.numbers[i] > b.numbers[i]
        }
        return !a.preRelease && b.preRelease
    }

    private class Version(val numbers: List<Int>, val preRelease: Boolean)

    private fun parse(value: String): Version? {
        val core = value.trim().removePrefix("v").substringBefore('+')
        val parts = core.substringBefore('-').split('.')
        if (parts.isEmpty() || parts.size > 3) return null
        val numbers = parts.map { it.toIntOrNull() ?: return null }
        return Version(numbers + List(3 - numbers.size) { 0 }, preRelease = core.contains('-'))
    }
}
