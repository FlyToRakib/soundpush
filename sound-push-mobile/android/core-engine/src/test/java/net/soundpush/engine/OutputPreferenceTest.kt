package net.soundpush.engine

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class OutputPreferenceTest {
    @Test
    fun unknownOrMissingKeysFallBackToAutomatic() {
        assertEquals(OutputPreference.Target.Automatic, OutputPreference.fromKey(null))
        assertEquals(OutputPreference.Target.Automatic, OutputPreference.fromKey("hdmi"))
        assertEquals(OutputPreference.Target.Bluetooth, OutputPreference.fromKey("bluetooth"))
    }

    @Test
    fun onlyMediaOutputsAreOffered() {
        assertEquals(OutputPreference.Target.Speaker, OutputPreference.targetFor(2)) // TYPE_BUILTIN_SPEAKER
        assertEquals(OutputPreference.Target.Wired, OutputPreference.targetFor(3)) // TYPE_WIRED_HEADSET
        assertEquals(OutputPreference.Target.Bluetooth, OutputPreference.targetFor(8)) // TYPE_BLUETOOTH_A2DP
        assertEquals(OutputPreference.Target.Usb, OutputPreference.targetFor(22)) // TYPE_USB_HEADSET
        assertNull(OutputPreference.targetFor(1)) // TYPE_BUILTIN_EARPIECE
        assertNull(OutputPreference.targetFor(7)) // TYPE_BLUETOOTH_SCO (calls)
    }
}
