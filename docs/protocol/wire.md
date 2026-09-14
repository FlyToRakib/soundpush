# SoundPush wire protocol (v1.1)

Implemented in `sound-push-core/sp-protocol`. This document is normative.

Supported range: **1.0 – 1.1**. Everything added in 1.1 is gated by a capability bit in `Hello`
(below), so a 1.1 build talks to a 1.0 build exactly as 1.0 did.

## Transport

- QUIC (RFC 9000) over UDP, TLS 1.3 only, ALPN `soundpush/1`.
- Preferred port **47650/udp**; falls back to an ephemeral port. The real port is advertised by discovery and pairing codes.
- Both sides present a self-signed certificate whose key is the device's Ed25519 identity key. Trust is decided by the engine after the handshake (see `docs/security/pairing.md`).
- Keep-alive 1 s, idle timeout 10 s.
- Connection migration is enabled: a peer whose address changes within the idle timeout keeps its connection.

### TLS over TCP (1.1, `TRANSPORT_TCP`)

For links that forward TCP only (USB via `adb reverse`, ADR-0005).

- TLS 1.3 only, ALPN `soundpush-tcp/1`, the same certificates and verifiers as QUIC (the dialer pins the device ID).
- Desktops listen on **loopback only**, on the same port number as their UDP port when free. A phone dials
  `127.0.0.1:<computer's port>`, which `adb reverse tcp:<port> tcp:<tcp port>` forwards. The dialer starts this
  candidate 2 s after the network candidates (or immediately when it has none); the first authenticated handshake wins.
- One stream carries typed frames: `kind u8 | length u32 BE | payload`.

| kind | Payload | Max |
|---|---|---|
| 1 Control | one `ControlMsg` | 64 KiB |
| 2 Datagram | one media or probe datagram | 1216 bytes |
| 3 Ping | sender clock, `u64` µs (sent every 1 s) | 256 |
| 4 Pong | the ping payload, echoed | 256 |
| 5 Close | `u32` code | 256 |

  An unknown kind or an oversized length closes the connection. Silence for 10 s closes it. Senders drop media
  instead of queueing it: the datagram send queue is small, and a datagram that waited more than 200 ms is discarded.

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

`RouteUpdate` semantics: the route's requester proposes codec, bitrate, channels, frame size and redundancy; the
receiving side always keeps its own jitter bounds. Bitrate and redundancy apply live. A codec, channel or frame change
is only sent to peers with `FEATURE_ROUTE_RECONFIGURE` (both sides rebuild their pipelines); for other peers the route
is stopped (`Superseded`) and requested again.

### Protocol 1.1 messages

Sent only to peers advertising the capability; a 1.0 peer cannot decode them. Receivers skip well-formed
messages whose type they do not know.

| Message (tag) | Capability | Meaning |
|---|---|---|
| `SessionTicket{token, lifetime_secs}` (26) | `FEATURE_SESSION_RESUME` | After a session is established each side issues the peer a 32-byte token, valid 10 min. |
| `NetTestStart{test_id, duration_ms}` (27) | `FEATURE_NETWORK_TEST` | Ask the peer to echo probe datagrams of this test (at most 15 s). |
| `NetTestReady{test_id, accepted}` (28) | `FEATURE_NETWORK_TEST` | Answer to `NetTestStart`. |
| `NetTestStop{test_id}` (29) | `FEATURE_NETWORK_TEST` | The test ended; stop echoing. |

Session resume: the next connection's `Hello.resume_token` carries the ticket the peer issued. The issuer accepts it
once, only from the same public key, before expiry. A resumed session restores routes that were paused by the
interruption without asking the user again (see `docs/security/pairing.md`).

### Capability bits

| Bit | Name |
|---|---|
| 0, 1, 2 | `SOURCE_SYSTEM_AUDIO`, `SOURCE_APP_AUDIO`, `SOURCE_MICROPHONE` |
| 8, 9 | `SINK_SPEAKER`, `SINK_VIRTUAL_MIC` |
| 16, 17 | `CODEC_OPUS`, `CODEC_PCM` |
| 24, 25, 26 | `FEATURE_REDUNDANCY`, `FEATURE_REMOTE_CONTROL`, `FEATURE_MIC_MONITOR` |
| 27 | `FEATURE_SESSION_RESUME` (1.1) |
| 28 | `FEATURE_NETWORK_TEST` (1.1) |
| 29 | `FEATURE_ROUTE_RECONFIGURE` (1.1) |
| 30 | `TRANSPORT_TCP` (1.1) |
| 31 | `FEATURE_DTX` (receiver handles header-only DTX packets, see below) |

Each peer rate-limits incoming control messages (100/s average, bursts of 400); excess messages are dropped and a
peer that keeps flooding is disconnected.

## Media datagrams

Sent as QUIC DATAGRAM frames. Header (16 bytes, big-endian):

```
0      1      2      3      4 .. 11                 12 .. 15
ver    route  codec  flags  sample_timestamp (u64)  seq (u32)
```

- `ver` = 1. `codec`: 1 = PCM s16le, 2 = Opus.
- `flags`: 0x01 DTX (header-only silence packet, see below), 0x02 REDUNDANT, 0x04 DISCONTINUITY, 0x08 MARKER.
- `sample_timestamp`: position of the first sample on the sender's 48 kHz clock.
- Senders produce payloads of at most **1100 bytes** (1116-byte datagrams): QUIC starts every path at a 1200-byte UDP
  payload, and a short-header packet spends up to ~40 bytes on connection id, packet number, AEAD tag and frame
  header. Receivers accept up to 1200 bytes (what 1.0 senders used). PCM frames are capped to 960 bytes
  (5 ms stereo, 10 ms mono). Receivers drop malformed packets silently.
- One encoder may serve several routes (shared encoder groups): the same encoded payload and timeline go to each
  receiver with its own `route` byte. A receiver that was muted at the sender restarts with `DISCONTINUITY`.

### Silence (DTX, `FEATURE_DTX`)

Opus routes only; PCM (lossless) always sends every frame. A sender uses DTX only towards receivers that advertise
`FEATURE_DTX`; other receivers of the same encoder group keep getting encoded audio.

- The sender's signal after its DSP chain is "silent" when every sample is below −60 dBFS. After **200 ms** of
  silence (hangover) it stops sending audio.
- In place of the frame where DTX starts, and the one after it, it sends a **DTX packet**: `flags` = `DTX`, the
  frame's `sample_timestamp` and `seq`, **empty payload**. While silence lasts it repeats a DTX packet every
  **400 ms** (keep-alive). `seq` keeps counting frames that were not sent.
- The first frame with sound is an ordinary audio packet on the same timeline (no `DISCONTINUITY`).
- A receiver treats a DTX packet as "silent from `sample_timestamp` until the next audio frame": it plays comfort
  silence, does not run concealment, and counts neither loss nor underruns. Its jitter-buffer target does not grow.
  A DTX packet older than the play position is dropped like any late packet.

### Probe datagrams (1.1, `FEATURE_NETWORK_TEST`)

```
0      1      2 .. 5    6 .. 9  10 .. 17        18 .. 25        26 ..
0xA5   kind   test_id   seq     sent_us (u64)   echo_us (u64)   padding
```

`0xA5` is never a valid media header version, so peers not running a test drop probes. `kind` 1 = probe,
2 = echo. The responder echoes only probes of a test it accepted, at most 400 per second, and echoes carry no
padding (never more bytes out than in). Probes are at most 1116 bytes.

## Compatibility rules

- Never reuse or renumber protobuf tags; new fields are optional.
- Features are gated by `Capabilities` bits in `Hello` and discovery, not by version numbers.
- Supported range: current release and the two previous minor protocol versions.
