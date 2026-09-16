package net.soundpush.engine

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class AutoQualityTest {
    private val wifi = DeviceStatus.NetworkInfo(wifi = true)
    private val peer = PeerView(deviceId = "p1", name = "Desk PC", trusted = true, connection = "connected", quality = "excellent")
    private val state = EngineState(peers = listOf(peer))

    @Test
    fun aChargerAndAGoodLinkAreWorthUncompressedAudio() {
        assertTrue(preferLossless(onPower = true, state = state, network = wifi))
        assertTrue(preferLossless(onPower = true, state = state.copy(peers = listOf(peer.copy(quality = "good"))), network = wifi))
    }

    @Test
    fun onBatteryOpusWins() {
        assertFalse(preferLossless(onPower = false, state = state, network = wifi))
    }

    @Test
    fun aQualityTheUserPickedIsNeverOverridden() {
        val fixed = state.copy(settings = Settings(stream = StreamSettings(quality = "lossless")))
        assertFalse(preferLossless(onPower = true, state = fixed, network = wifi))
        assertFalse(
            preferLossless(onPower = true, state = state.copy(settings = Settings(stream = StreamSettings(quality = "opus"))), network = wifi),
        )
    }

    @Test
    fun aPoorOrMeteredLinkHasNoRoomForIt() {
        assertFalse(preferLossless(onPower = true, state = state.copy(peers = listOf(peer.copy(quality = "poor"))), network = wifi))
        assertFalse(preferLossless(onPower = true, state = state, network = DeviceStatus.NetworkInfo(cellular = true)))
        assertFalse(
            preferLossless(onPower = true, state = state, network = DeviceStatus.NetworkInfo(cellular = true, usbTethering = true)),
        )
    }

    @Test
    fun withNothingConnectedThereIsNothingToDecide() {
        assertFalse(preferLossless(onPower = true, state = null, network = wifi))
        assertFalse(preferLossless(onPower = true, state = EngineState(), network = wifi))
    }
}
