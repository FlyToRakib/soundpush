package net.soundpush.engine

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class NetworkChangeFilterTest {
    private val wifi = listOf("192.168.1.20/24")

    @Test
    fun firstReportIsTheStartingPoint() {
        val f = NetworkChangeFilter()
        assertFalse(f.onDefault("100", wifi))
        assertFalse(f.onDefault("100", wifi))
    }

    @Test
    fun newAddressOrNetworkIsAChange() {
        val f = NetworkChangeFilter()
        f.onDefault("100", wifi)
        assertTrue(f.onDefault("100", listOf("192.168.1.21/24")))
        assertTrue(f.onDefault("101", listOf("10.0.0.5/8")))
    }

    @Test
    fun losingTheDefaultNetworkAndGettingOneBackAreChanges() {
        val f = NetworkChangeFilter()
        f.onDefault("100", wifi)
        assertFalse(f.onLost("99"))
        assertTrue(f.onLost("100"))
        assertTrue(f.onDefault("100", wifi))
    }
}
