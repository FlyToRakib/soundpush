# SoundPush wire protocol (v1.0)

Implemented in `sound-push-core/sp-protocol`. This document is normative.

## Transport

- QUIC (RFC 9000) over UDP, TLS 1.3 only, ALPN `soundpush/1`.
- Preferred port **47650/udp**; falls back to an ephemeral port. The real port is advertised by discovery and pairing codes.
- Both sides present a self-signed certificate whose key is the device's Ed25519 identity key. Trust is decided by the engine after the handshake (see `docs/security/pairing.md`).
- Keep-alive 1 s, idle timeout 10 s.

## Control stream

The dialer opens the first bidirectional stream and sends first. Messages are
length-prefixed (`u32` big-endian length, max 64 KiB) protobuf `ControlMsg` envelopes.

Session start:

1. Dialer → `Hello`; listener → `Hello`.
2. Each side checks that `Hello.device_id` equals the first 16 bytes of SHA-256(peer public key). A mismatch closes the connection.
3. Protocol version = highest common value of both `[protocol_min, protocol_max]` ranges; no overlap → `Goodbye(IncompatibleVersion)`.
4. An untrusted dialer then sends `PairRequest`; an untrusted listener waits up to 30 s for one.

Routes:

| Message | Direction | Meaning |
|---|---|---|
| `RouteRequest{route, source_endpoint, sink_endpoint, requester_is_source, profile}` | either | Ask to start a route. The requester allocates `route` (dialer even ids, listener odd ids). |
| `RouteAccept{route, profile}` | responder | Accepted with the final profile (receiver's jitter bounds win). |
| `RouteReject{route, reason}` | responder | Declined. |
| `RouteStop{route, reason}` | either | Stop. |
| `RouteUpdate{route, profile}` | either | Live profile change. |
| `VolumeSet` / `MuteSet` | either | Remote control (`target = RouteStream` or `DeviceSpeakers`); requires the ControlMe permission. |
| `StatsReport` | receiver → sender, 1 Hz | Loss, jitter, buffer, drift; drives adaptive bitrate. |
| `Goodbye{reason}` | either | Orderly close. |

Endpoint ids: sources `system`, `apps`, `mic`; sinks `speaker`, `virtual-mic`.

## Media datagrams

Sent as QUIC DATAGRAM frames. Header (16 bytes, big-endian):

```
0      1      2      3      4 .. 11                 12 .. 15
ver    route  codec  flags  sample_timestamp (u64)  seq (u32)
```

- `ver` = 1. `codec`: 1 = PCM s16le, 2 = Opus.
- `flags`: 0x01 DTX, 0x02 REDUNDANT, 0x04 DISCONTINUITY, 0x08 MARKER.
- `sample_timestamp`: position of the first sample on the sender's 48 kHz clock.
- Payload ≤ 1200 bytes. Receivers drop malformed packets silently.

## Compatibility rules

- Never reuse or renumber protobuf tags; new fields are optional.
- Features are gated by `Capabilities` bits in `Hello` and discovery, not by version numbers.
- Supported range: current release and the two previous minor protocol versions.
