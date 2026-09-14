package net.soundpush.settings

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class UpdateCheckerTest {
    @Test
    fun newerReleaseIsDetected() {
        assertTrue(UpdateChecker.isNewer("v0.2.0", "0.1.0"))
        assertTrue(UpdateChecker.isNewer("v1.0.0", "0.9.12"))
        assertTrue(UpdateChecker.isNewer("0.1.10", "0.1.9"))
    }

    @Test
    fun sameOrOlderIsNotNewer() {
        assertFalse(UpdateChecker.isNewer("v0.1.0", "0.1.0"))
        assertFalse(UpdateChecker.isNewer("v0.1.0", "0.2.0"))
    }

    @Test
    fun releaseBeatsItsPreRelease() {
        assertTrue(UpdateChecker.isNewer("v1.0.0", "1.0.0-beta.2"))
        assertFalse(UpdateChecker.isNewer("v1.0.0-beta.2", "1.0.0"))
    }

    @Test
    fun malformedVersionsAreIgnored() {
        assertFalse(UpdateChecker.isNewer("nightly", "0.1.0"))
        assertFalse(UpdateChecker.isNewer("v0.2.0", "dev"))
    }
}
