package net.soundpush.ui.components

import org.junit.Assert.assertEquals
import org.junit.Test

class WidthClassTest {
    @Test
    fun materialBreakpoints() {
        assertEquals(WidthClass.Compact, WidthClass.of(393))
        assertEquals(WidthClass.Compact, WidthClass.of(599))
        assertEquals(WidthClass.Medium, WidthClass.of(600))
        assertEquals(WidthClass.Medium, WidthClass.of(839))
        assertEquals(WidthClass.Expanded, WidthClass.of(840))
        assertEquals(WidthClass.Expanded, WidthClass.of(1280))
    }
}
