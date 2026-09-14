package net.soundpush.app

import org.junit.Assert.assertEquals
import org.junit.Test

class PermissionStateTest {
    @Test
    fun grantedWinsOverHistory() {
        assertEquals(PermissionState.Granted, PermissionState.of(granted = true, refusedBefore = true, shouldShowRationale = false))
    }

    @Test
    fun neverRefusedIsNotAsked() {
        assertEquals(PermissionState.NotAsked, PermissionState.of(granted = false, refusedBefore = false, shouldShowRationale = false))
    }

    @Test
    fun refusedOnceStillAsks() {
        assertEquals(PermissionState.Denied, PermissionState.of(granted = false, refusedBefore = true, shouldShowRationale = true))
    }

    @Test
    fun refusedWithoutRationaleIsBlocked() {
        assertEquals(PermissionState.Blocked, PermissionState.of(granted = false, refusedBefore = true, shouldShowRationale = false))
    }
}
