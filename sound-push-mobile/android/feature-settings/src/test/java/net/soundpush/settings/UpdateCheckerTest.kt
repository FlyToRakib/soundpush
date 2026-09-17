package net.soundpush.settings

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
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
    fun zeroXPreviewsCountAsStableButBetasAndDraftsDoNot() {
        val releases = listOf(
            UpdateChecker.Release("v0.1.0", "u1", preRelease = true),
            UpdateChecker.Release("v0.2.0-beta.1", "u2", preRelease = true),
            UpdateChecker.Release("v0.1.1", "u3", draft = true),
            UpdateChecker.Release("updates", "u4", preRelease = true),
        )
        assertEquals("v0.1.0", UpdateChecker.newestStable(releases)?.tag)
    }

    @Test
    fun fromOnePointZeroAPreReleaseFlagKeepsAVersionOffStable() {
        val releases = listOf(UpdateChecker.Release("v1.0.0", "a"), UpdateChecker.Release("v1.0.1", "b", preRelease = true))
        assertEquals("v1.0.0", UpdateChecker.newestStable(releases)?.tag)
        assertNull(UpdateChecker.newestStable(emptyList()))
    }

    @Test
    fun malformedVersionsAreIgnored() {
        assertFalse(UpdateChecker.isNewer("nightly", "0.1.0"))
        assertFalse(UpdateChecker.isNewer("v0.2.0", "dev"))
    }
}
