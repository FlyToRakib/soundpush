package net.soundpush.engine

import android.media.AudioDeviceInfo
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class DeviceStatusTest {
    @Test
    fun bluetoothWinsOverWiredAndSpeaker() {
        val types = listOf(AudioDeviceInfo.TYPE_BUILTIN_SPEAKER, AudioDeviceInfo.TYPE_WIRED_HEADPHONES, AudioDeviceInfo.TYPE_BLUETOOTH_A2DP)
        assertEquals(DeviceStatus.Output.Bluetooth, DeviceStatus.classify(types))
        assertEquals(DeviceStatus.Output.Bluetooth, DeviceStatus.classify(listOf(26)))
        assertEquals(DeviceStatus.Output.Wired, DeviceStatus.classify(listOf(AudioDeviceInfo.TYPE_BUILTIN_SPEAKER, AudioDeviceInfo.TYPE_WIRED_HEADSET)))
        assertEquals(DeviceStatus.Output.Speaker, DeviceStatus.classify(emptyList()))
    }

    @Test
    fun subnetMatching() {
        val net = byteArrayOf(192.toByte(), 168.toByte(), 42, 129.toByte())
        assertTrue(DeviceStatus.sameSubnet(byteArrayOf(192.toByte(), 168.toByte(), 42, 7), net, 24))
        assertFalse(DeviceStatus.sameSubnet(byteArrayOf(192.toByte(), 168.toByte(), 43, 7), net, 24))
        assertTrue(DeviceStatus.sameSubnet(byteArrayOf(10, 0, 0, 1), net, 0))
    }

    @Test
    fun mobileDataFlags() {
        val tether = DeviceStatus.NetworkInfo(cellular = true, usbTethering = true)
        assertTrue(tether.sharesMobileData)
        assertFalse(tether.mobileDataOnly)
        assertFalse(tether.copy(wifi = true).sharesMobileData)
        assertTrue(DeviceStatus.NetworkInfo(cellular = true).mobileDataOnly)
    }

    @Test
    fun tetherAddressMatching() {
        val info = DeviceStatus.NetworkInfo(usbTethering = true, tetherSubnets = listOf(byteArrayOf(192.toByte(), 168.toByte(), 42, 129.toByte()) to 24))
        assertTrue(info.isTetherAddress("192.168.42.10:47650"))
        assertTrue(info.isTetherAddress("192.168.42.10"))
        assertFalse(info.isTetherAddress("192.168.1.10:47650"))
        assertFalse(info.isTetherAddress("my-pc.local:47650"))
        assertFalse(info.isTetherAddress("[fe80::1]:47650"))
    }
}
