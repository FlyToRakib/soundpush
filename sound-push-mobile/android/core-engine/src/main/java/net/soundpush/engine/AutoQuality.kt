package net.soundpush.engine

/**
 * What the "Auto" quality setting should mean on this phone right now (plan §14.6).
 *
 * Opus decoding is cheap but the radio is not: on battery, the smaller stream wins. On a charger
 * the extra radio time costs nothing, so uncompressed audio is worth it — but only over a link with
 * room for about 1.5 Mb/s, and never over mobile data, where it would also cost the user money.
 *
 * The engine keeps its own bitrate adaptation either way, and a quality the user picked is never
 * overridden: this only decides what "Auto" resolves to.
 */
fun preferLossless(onPower: Boolean, state: EngineState?, network: DeviceStatus.NetworkInfo): Boolean {
    if (!onPower || state == null) return false
    if (state.settings.stream.quality != "auto") return false
    if (network.mobileDataOnly || network.sharesMobileData) return false
    val connected = state.connectedPeers
    return connected.isNotEmpty() && connected.all { it.quality == "excellent" || it.quality == "good" }
}
