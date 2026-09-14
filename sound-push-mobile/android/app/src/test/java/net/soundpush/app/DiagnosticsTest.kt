package net.soundpush.app

import net.soundpush.engine.EngineState
import net.soundpush.engine.LocalDevice
import net.soundpush.engine.PairingView
import net.soundpush.engine.PeerView
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Test

class DiagnosticsTest {
    @Test
    fun redactsRemoteAddressesIdsAndPairingSecret() {
        val state = EngineState(
            local = LocalDevice(deviceId = "local-device-id", name = "Phone"),
            peers = listOf(PeerView(deviceId = "abcdef0123456789", name = "PC", addresses = listOf("192.168.1.20:47650", "10.0.0.2:47650"))),
            pairing = PairingView(qrUri = "soundpush://pair?secret=very-secret"),
        )
        val redacted = Diagnostics.redact(state)
        val peer = redacted.peers.single()
        assertEquals("abcdef01", peer.deviceId)
        assertEquals(listOf("<redacted>", "<redacted>"), peer.addresses)
        assertEquals("<redacted>", redacted.pairing.qrUri)
        assertFalse(redacted.toString().contains("very-secret"))
        assertFalse(redacted.toString().contains("192.168.1.20"))
    }
}
