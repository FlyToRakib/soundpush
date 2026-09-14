package net.soundpush.service

import net.soundpush.engine.MicSettings
import net.soundpush.engine.RouteView
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class HeadsetModeTest {
    private fun route(kind: String, peer: String = "pc", status: String = "active") =
        RouteView(routeId = "$kind-$peer", peerId = peer, peerName = peer, kind = kind, status = status)

    @Test
    fun listeningAndSendingTheMicToTheSameDeviceIsHeadset() {
        assertTrue(HeadsetMode.active(listOf(route("receiveSystemAudio"), route("sendMicToVirtualMic"))))
    }

    @Test
    fun micAloneOrToAnotherDeviceIsNotHeadset() {
        assertFalse(HeadsetMode.active(listOf(route("sendMicToVirtualMic"))))
        assertFalse(HeadsetMode.active(listOf(route("receiveSystemAudio", "pc"), route("sendMicToVirtualMic", "laptop"))))
        assertFalse(HeadsetMode.active(listOf(route("receiveSystemAudio", status = "stopped"), route("sendMicToVirtualMic"))))
    }

    @Test
    fun headsetForcesEchoCancellationAndKeepsTheRest() {
        val user = MicSettings(mode = "raw", systemEchoCancellation = false, gainDb = 6f, systemAgc = true)
        val forced = HeadsetMode.recorderSettings(user, headset = true)
        assertEquals(HeadsetMode.PRESET, forced.mode)
        assertTrue(forced.systemEchoCancellation)
        assertTrue(forced.systemAgc)
        assertEquals(6f, forced.gainDb)
        assertEquals(user, HeadsetMode.recorderSettings(user, headset = false))
    }
}
