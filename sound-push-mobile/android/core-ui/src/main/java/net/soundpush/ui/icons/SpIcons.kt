package net.soundpush.ui.icons

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.vector.PathParser
import androidx.compose.ui.unit.dp

/**
 * The same outline icon set as the desktop app (24×24 grid, 1.75 stroke, round caps).
 *
 * Arc commands must spell out every flag separately ("a5 5 0 0 1 0 7", not "a5 5 0 010 7"):
 * Android's path parser does not accept the compact SVG flag form and draws broken shapes.
 */
object SpIcons {
    private fun icon(name: String, vararg paths: String): ImageVector {
        val builder = ImageVector.Builder(name, 24.dp, 24.dp, 24f, 24f)
        for (d in paths) {
            builder.addPath(
                pathData = PathParser().parsePathString(d).toNodes(),
                fill = null,
                stroke = SolidColor(Color.Black),
                strokeLineWidth = 1.75f,
                strokeLineCap = StrokeCap.Round,
                strokeLineJoin = StrokeJoin.Round,
            )
        }
        return builder.build()
    }

    val Home by lazy { icon("home", "M4 10.5 12 4l8 6.5", "M6 9.5V20h4.5v-5.5h3V20H18V9.5") }
    /** A computer screen with a phone in front of it. */
    val Devices by lazy {
        icon(
            "devices",
            "M18 8V6a2 2 0 0 0-2-2H4a2 2 0 0 0-2 2v7a2 2 0 0 0 2 2h8",
            "M10 15v4",
            "M7 19h5",
            "M18 12h2a2 2 0 0 1 2 2v6a2 2 0 0 1-2 2h-2a2 2 0 0 1-2-2v-6a2 2 0 0 1 2-2z",
        )
    }
    val Back by lazy { icon("back", "M19 12H5", "M12 19l-7-7 7-7") }
    val Wifi by lazy { icon("wifi", "M5 12.55a11 11 0 0 1 14.08 0", "M1.42 9a16 16 0 0 1 21.16 0", "M8.53 16.11a6 6 0 0 1 6.95 0", "M12 20h.01") }
    val Audio by lazy { icon("audio", "M4 10v4", "M8 7v10", "M12 4v16", "M16 8v8", "M20 11v2") }
    val Settings by lazy {
        icon(
            "settings",
            "M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6z",
            "M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z",
        )
    }
    val Speaker by lazy { icon("speaker", "M11 5 6 9H3v6h3l5 4V5z", "M15.5 8.5a5 5 0 0 1 0 7", "M18.5 5.5a9 9 0 0 1 0 13") }
    val Mic by lazy { icon("mic", "M12 3a3 3 0 0 0-3 3v6a3 3 0 0 0 6 0V6a3 3 0 0 0-3-3z", "M5 11a7 7 0 0 0 14 0", "M12 18v3") }
    val Headset by lazy {
        icon(
            "headset",
            "M4 15v-3a8 8 0 0 1 16 0v3",
            "M4 15h2a1 1 0 0 1 1 1v3a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1v-4z",
            "M20 15h-2a1 1 0 0 0-1 1v3a1 1 0 0 0 1 1h1a1 1 0 0 0 1-1v-4z",
        )
    }
    val Phone by lazy { icon("phone", "M8 2h8a2 2 0 0 1 2 2v16a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2z", "M11 18h2") }
    val Laptop by lazy { icon("laptop", "M5 5h14a1 1 0 0 1 1 1v9H4V6a1 1 0 0 1 1-1z", "M2 19h20") }
    val Apps by lazy {
        icon(
            "apps",
            "M5 4h4a1 1 0 0 1 1 1v4a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1z",
            "M15 4h4a1 1 0 0 1 1 1v4a1 1 0 0 1-1 1h-4a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1z",
            "M5 14h4a1 1 0 0 1 1 1v4a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1v-4a1 1 0 0 1 1-1z",
            "M15 14h4a1 1 0 0 1 1 1v4a1 1 0 0 1-1 1h-4a1 1 0 0 1-1-1v-4a1 1 0 0 1 1-1z",
        )
    }
    val Plus by lazy { icon("plus", "M12 5v14", "M5 12h14") }
    val Close by lazy { icon("close", "M6 6l12 12", "M18 6 6 18") }
    val Stop by lazy { icon("stop", "M8 7h8a1 1 0 0 1 1 1v8a1 1 0 0 1-1 1H8a1 1 0 0 1-1-1V8a1 1 0 0 1 1-1z") }
    val Mute by lazy { icon("mute", "M11 5 6 9H3v6h3l5 4V5z", "M16 9l5 6", "M21 9l-5 6") }
    val Qr by lazy {
        icon(
            "qr",
            "M4 4h6v6H4z",
            "M14 4h6v6h-6z",
            "M4 14h6v6H4z",
            "M14 14h2v2h-2z",
            "M18 18h2v2h-2z",
            "M14 18h2",
            "M18 14h2",
        )
    }
    val Chevron by lazy { icon("chevron", "M9 6l6 6-6 6") }
    val Alert by lazy { icon("alert", "M12 3 22 21H2L12 3z", "M12 10v4", "M12 17.5h.01") }

    fun forPlatform(platform: String) = if (platform == "android" || platform == "ios") Phone else Laptop
}
