package net.soundpush.engine

/**
 * Which parts of an engine snapshot are numbers that move on their own: stream statistics, level
 * meters, elapsed time and round-trip time. Nothing in that list reaches the notification or a
 * screen that is off, so while the app is hidden a snapshot that changes nothing else can be
 * dropped instead of re-rendered (plan §14.6 "Stats UI updates are throttled and stop when the
 * screen is off").
 */
internal object StateUpdates {
    /** True when [next] differs from [last] only in numbers nobody can currently see. */
    fun onlyLiveNumbersChanged(last: EngineState, next: EngineState): Boolean =
        last != next && settled(last) == settled(next)

    /** [state] with those numbers reset, so two snapshots can be compared on everything else. */
    fun settled(state: EngineState): EngineState = state.copy(
        // The revision counts publications, not changes.
        revision = 0,
        micLevelDb = 0f,
        routes = state.routes.map { it.copy(elapsedSecs = 0, stats = RouteStats()) },
        peers = state.peers.map { it.copy(rttMs = 0.0) },
    )
}
