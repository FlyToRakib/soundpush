package net.soundpush.ui.icons

import androidx.compose.ui.graphics.vector.PathNode
import androidx.compose.ui.graphics.vector.VectorPath
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Every icon must parse into real drawing commands. A compact SVG arc flag such as
 * "a5 5 0 010 7" parses incorrectly on Android and renders a broken icon.
 */
class SpIconsTest {
    private val icons = mapOf(
        "Home" to SpIcons.Home, "Devices" to SpIcons.Devices, "Audio" to SpIcons.Audio,
        "Settings" to SpIcons.Settings, "Speaker" to SpIcons.Speaker, "Mic" to SpIcons.Mic,
        "Headset" to SpIcons.Headset, "Phone" to SpIcons.Phone, "Laptop" to SpIcons.Laptop,
        "Apps" to SpIcons.Apps, "Plus" to SpIcons.Plus, "Close" to SpIcons.Close,
        "Stop" to SpIcons.Stop, "Mute" to SpIcons.Mute, "Qr" to SpIcons.Qr,
        "Chevron" to SpIcons.Chevron, "Alert" to SpIcons.Alert, "Back" to SpIcons.Back, "Wifi" to SpIcons.Wifi,
    )

    @Test
    fun everyIconHasDrawablePaths() {
        for ((name, icon) in icons) {
            val paths = (0 until icon.root.size).map { icon.root[it] }.filterIsInstance<VectorPath>()
            assertTrue("$name has no paths", paths.isNotEmpty())
            for (path in paths) {
                assertTrue("$name has an empty path", path.pathData.isNotEmpty())
                assertTrue("$name path must start with a move", path.pathData.first() is PathNode.MoveTo)
                val coords = path.pathData.flatMap { coordinates(it) }
                assertTrue("$name has coordinates outside the 24×24 grid: $coords", coords.all { it in -24f..24f })
            }
        }
    }

    @Test
    fun arcsKeepTheirFlags() {
        // Speaker waves are two quarter arcs: exactly two relative arc commands with sweep flag set.
        val arcs = (0 until SpIcons.Speaker.root.size)
            .map { SpIcons.Speaker.root[it] }
            .filterIsInstance<VectorPath>()
            .flatMap { it.pathData }
            .filterIsInstance<PathNode.RelativeArcTo>()
        assertTrue("expected 2 arcs, got ${arcs.size}", arcs.size == 2)
        assertTrue(arcs.all { it.isPositiveArc && !it.isMoreThanHalf })
    }

    private fun coordinates(node: PathNode): List<Float> = when (node) {
        is PathNode.MoveTo -> listOf(node.x, node.y)
        is PathNode.RelativeMoveTo -> listOf(node.dx, node.dy)
        is PathNode.LineTo -> listOf(node.x, node.y)
        is PathNode.RelativeLineTo -> listOf(node.dx, node.dy)
        is PathNode.HorizontalTo -> listOf(node.x)
        is PathNode.VerticalTo -> listOf(node.y)
        is PathNode.RelativeHorizontalTo -> listOf(node.dx)
        is PathNode.RelativeVerticalTo -> listOf(node.dy)
        is PathNode.ArcTo -> listOf(node.arcStartX, node.arcStartY)
        is PathNode.RelativeArcTo -> listOf(node.arcStartDx, node.arcStartDy)
        else -> emptyList()
    }
}
