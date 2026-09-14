# ADR-0005: QUIC transport with key-pinned mutual TLS

- Status: accepted
- Date: 2026-09-13

## Decision

All traffic uses QUIC (quinn + rustls, TLS 1.3 only, ring crypto). Control messages use the first
bidirectional stream; media uses unreliable QUIC datagrams on the same connection. Both peers present
self-signed certificates bound to their Ed25519 identity; the dialer pins the expected fingerprint when
it knows the peer.

## Why

- Encryption and mutual authentication are mandatory, not optional.
- One handshake and one UDP port for control and media.
- Datagrams avoid head-of-line blocking; connection migration survives Wi-Fi roams.
- ring cross-compiles to Android without CMake.

## Consequences

- `adb reverse` forwards TCP only, so USB-via-ADB uses a TLS-over-TCP transport behind the same
  `SecureConnection` API (`sp-transport/src/tcp.rs`, protocol 1.1): the same certificates and verifiers,
  one stream multiplexing control, datagram and keep-alive frames, and media dropped rather than delayed
  when the socket backs up. Desktops listen on loopback only; phones try the loopback candidate after
  the network candidates (USB tethering works as plain IP).
- Two-engine integration tests exercise the real QUIC and TCP stacks on loopback.
