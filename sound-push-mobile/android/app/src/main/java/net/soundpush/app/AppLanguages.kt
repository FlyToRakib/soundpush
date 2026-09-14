package net.soundpush.app

import android.content.Context
import android.content.pm.ApplicationInfo
import androidx.appcompat.app.AppCompatDelegate
import androidx.core.os.LocaleListCompat
import java.util.Locale
import org.xmlpull.v1.XmlPullParser

/**
 * In-app language (plan §4.6: "System language" by default, plus the translations this build has).
 *
 * Android keeps the choice as the per-app language (LocaleManager on 13+, AppCompat's own store on
 * 8–12), so it survives restarts and can also be changed from system settings. The engine's
 * `language` setting mirrors it, so the shared settings file and diagnostics say the same.
 */
internal object AppLanguages {
    const val SYSTEM = "system"
    private const val ANDROID_NS = "http://schemas.android.com/apk/res/android"

    /** Long-text and right-to-left pseudo-locales (debug builds only), see docs/translating.md. */
    private val PSEUDO = listOf("en-XA", "ar-XB")

    /** BCP-47 tags of the languages this build is translated into, from the generated locale config. */
    fun available(context: Context): List<String> {
        val tags = runCatching { parseLocaleConfig(context) }.getOrNull().orEmpty().ifEmpty { listOf("en") }
        val debuggable = context.applicationInfo.flags and ApplicationInfo.FLAG_DEBUGGABLE != 0
        return (tags + if (debuggable) PSEUDO else emptyList()).distinct()
    }

    private fun parseLocaleConfig(context: Context): List<String> =
        context.resources.getXml(R.xml._generated_res_locale_config).use { parser ->
            buildList {
                while (parser.next() != XmlPullParser.END_DOCUMENT) {
                    if (parser.eventType == XmlPullParser.START_TAG && parser.name == "locale") {
                        parser.getAttributeValue(ANDROID_NS, "name")?.let(::add)
                    }
                }
            }
        }

    /** The language in use: a BCP-47 tag, or [SYSTEM] while the app follows the phone. */
    fun current(): String = AppCompatDelegate.getApplicationLocales().toLanguageTags().ifEmpty { SYSTEM }

    /** Switch the app's language; Android recreates the visible screens in it. */
    fun apply(tag: String) {
        AppCompatDelegate.setApplicationLocales(
            if (tag == SYSTEM) LocaleListCompat.getEmptyLocaleList() else LocaleListCompat.forLanguageTags(tag),
        )
    }

    /** A language's own name ("Deutsch", "English"), so people find theirs whatever language is on. */
    fun displayName(tag: String): String {
        val locale = Locale.forLanguageTag(tag)
        return locale.getDisplayName(locale).replaceFirstChar { if (it.isLowerCase()) it.titlecase(locale) else it.toString() }
    }
}
