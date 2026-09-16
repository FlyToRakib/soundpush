package net.soundpush.engine

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class StateUpdatesTest {
    private val route = RouteView(
        routeId = "r1",
        peerId = "p1",
        peerName = "Desk PC",
        kind = "receiveSystemAudio",
        status = "active",
        elapsedSecs = 12,
        stats = RouteStats(latencyMs = 40.0, levelDb = -20f),
    )
    private val peer = PeerView(deviceId = "p1", name = "Desk PC", trusted = true, connection = "connected", rttMs = 8.0)
    private val state = EngineState(revision = 7, peers = listOf(peer), routes = listOf(route), micLevelDb = -30f)

    @Test
    fun statsMetersAndElapsedTimeAreJustNumbers() {
        val next = state.copy(
            revision = 8,
            micLevelDb = -18f,
            // The clip light moves with the level meter it belongs to.
            micClipping = true,
            routes = listOf(route.copy(elapsedSecs = 13, stats = route.stats.copy(latencyMs = 44.0, levelDb = -14f))),
            peers = listOf(peer.copy(rttMs = 9.5)),
        )
        assertTrue(StateUpdates.onlyLiveNumbersChanged(state, next))
        // A snapshot published with nothing new in it but its own revision counts the same way.
        assertTrue(StateUpdates.onlyLiveNumbersChanged(state, state.copy(revision = 8)))
    }

    @Test
    fun anythingSomeoneCanActOnGetsThrough() {
        // A route ending, a device dropping, a setting changing and a notice all count as real.
        assertFalse(StateUpdates.onlyLiveNumbersChanged(state, state.copy(revision = 8, routes = emptyList())))
        assertFalse(
            StateUpdates.onlyLiveNumbersChanged(state, state.copy(revision = 8, routes = listOf(route.copy(muted = true)))),
        )
        assertFalse(
            StateUpdates.onlyLiveNumbersChanged(state, state.copy(revision = 8, peers = listOf(peer.copy(connection = "disconnected")))),
        )
        assertFalse(StateUpdates.onlyLiveNumbersChanged(state, state.copy(revision = 8, settings = Settings(audioCues = true))))
        assertFalse(
            StateUpdates.onlyLiveNumbersChanged(state, state.copy(revision = 8, notices = listOf(NoticeView(id = 1, key = "paired")))),
        )
    }

    @Test
    fun anUnchangedSnapshotIsNotAChange() {
        assertFalse(StateUpdates.onlyLiveNumbersChanged(state, state))
    }
}
