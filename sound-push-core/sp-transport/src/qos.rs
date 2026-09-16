//! DSCP marking of media traffic (plan §16.3): Expedited Forwarding (46), which WMM Wi-Fi maps to
//! the voice access category. Best effort everywhere: a refusal is logged at debug level and never
//! fails a connection.
//!
//! - **Linux, Android, macOS:** `IP_TOS` and `IPV6_TCLASS` on the socket. TCP connections carry
//!   the mark. QUIC packets currently do not: quinn-udp 0.5 attaches a per-packet TOS/TCLASS
//!   control message holding only the ECN bits, and the kernel uses that instead of the socket
//!   option. The socket option is still set, so the mark applies once quinn-udp can carry a DSCP;
//!   `quic_packets_still_overwrite_the_dscp_mark` below fails as soon as that version changes.
//! - **Windows:** socket TOS options are ignored unless a Group Policy allows them, so every QUIC
//!   peer gets a qWAVE flow instead (traffic type Voice, then outgoing DSCP 46 where the process may
//!   set it; without administrator rights qWAVE keeps the Voice default marking). A flow is bound
//!   to the peer's address when the connection starts; after connection migration the new path is
//!   not marked. Without the qWAVE service nothing is marked.

use std::net::SocketAddr;
use std::sync::Arc;

use tracing::debug;

/// Expedited Forwarding.
pub const DSCP_EF: u8 = 46;

/// Set the EF mark on a socket (UDP or TCP). Windows ignores socket TOS options: nothing to do.
pub(crate) fn mark_socket(socket: socket2::SockRef<'_>, ipv6: bool) {
    #[cfg(not(windows))]
    {
        let tos = u32::from(DSCP_EF) << 2;
        // A dual-stack socket sends IPv4 (mapped) traffic with IP_TOS and IPv6 with IPV6_TCLASS.
        if let Err(e) = socket.set_tos(tos) {
            debug!(error = %e, "IP_TOS not set");
        }
        #[cfg(any(
            target_os = "linux",
            target_os = "android",
            target_os = "macos",
            target_os = "ios"
        ))]
        if ipv6 {
            if let Err(e) = socket.set_tclass_v6(tos) {
                debug!(error = %e, "IPV6_TCLASS not set");
            }
        }
    }
    #[cfg(windows)]
    let _ = (socket, ipv6);
}

/// Marks a QUIC endpoint's traffic: the socket itself, and on Windows one qWAVE flow per peer.
pub(crate) struct Marker {
    #[cfg(windows)]
    qos: Option<qwave::Qos>,
}

impl Marker {
    pub(crate) fn for_socket(socket: &std::net::UdpSocket) -> Arc<Self> {
        let ipv6 = socket.local_addr().is_ok_and(|a| a.is_ipv6());
        mark_socket(socket2::SockRef::from(socket), ipv6);
        #[cfg(windows)]
        let qos = qwave::Qos::open(socket);
        #[cfg(windows)]
        if qos.is_none() {
            debug!(error = %std::io::Error::last_os_error(), "qWAVE unavailable: media is not DSCP-marked");
        }
        Arc::new(Self {
            #[cfg(windows)]
            qos,
        })
    }

    /// Mark traffic to `peer` where that needs a per-peer flow (Windows). The mark is removed
    /// when the last clone of the returned flow is dropped.
    pub(crate) fn mark_peer(self: &Arc<Self>, peer: SocketAddr) -> Option<Arc<Flow>> {
        #[cfg(windows)]
        {
            let id = self.qos.as_ref()?.add(peer, DSCP_EF)?;
            Some(Arc::new(Flow {
                marker: self.clone(),
                id,
            }))
        }
        #[cfg(not(windows))]
        {
            let _ = peer;
            None
        }
    }
}

/// A peer's qWAVE flow (Windows).
#[cfg(windows)]
pub(crate) struct Flow {
    marker: Arc<Marker>,
    id: u32,
}

#[cfg(windows)]
impl Drop for Flow {
    fn drop(&mut self) {
        if let Some(qos) = &self.marker.qos {
            qos.remove(self.id);
        }
    }
}

/// Per-peer flows exist only on Windows.
#[cfg(not(windows))]
pub(crate) enum Flow {}

/// The qWAVE calls. The only `unsafe` in this crate.
#[cfg(windows)]
#[allow(unsafe_code)]
mod qwave {
    use std::net::SocketAddr;
    use std::os::windows::io::AsRawSocket;

    use tracing::debug;
    use windows_sys::Win32::NetworkManagement::QoS::{
        QOS_NON_ADAPTIVE_FLOW, QOS_VERSION, QOSAddSocketToFlow, QOSCloseHandle, QOSCreateHandle,
        QOSRemoveSocketFromFlow, QOSSetFlow, QOSSetOutgoingDSCPValue, QOSTrafficTypeVoice,
    };
    use windows_sys::Win32::Networking::WinSock::{SOCKADDR, SOCKET};

    pub(super) struct Qos {
        /// The qWAVE handle, stored as an integer so the owner is `Send + Sync` without an unsafe
        /// impl; qWAVE handles may be used from any thread.
        handle: isize,
        socket: SOCKET,
    }

    impl Qos {
        pub(super) fn open(socket: &std::net::UdpSocket) -> Option<Self> {
            let version = QOS_VERSION {
                MajorVersion: 1,
                MinorVersion: 0,
            };
            let mut handle = std::ptr::null_mut();
            // SAFETY: `version` and `handle` are valid for the duration of the call.
            let ok = unsafe { QOSCreateHandle(&version, &mut handle) };
            (ok != 0).then(|| Self {
                handle: handle as isize,
                socket: socket.as_raw_socket() as SOCKET,
            })
        }

        /// Add `peer` to a flow with the Voice traffic type and try to set `dscp`. Returns the id.
        pub(super) fn add(&self, peer: SocketAddr, dscp: u8) -> Option<u32> {
            let addr = socket2::SockAddr::from(peer);
            let mut flow = 0u32;
            // SAFETY: the handle stays open until `Drop`; `addr` outlives the call and holds a
            // complete socket address of its family; `flow` is a valid out pointer. The socket is
            // owned by the endpoint, which outlives its connections and so every flow.
            let ok = unsafe {
                QOSAddSocketToFlow(
                    self.handle as _,
                    self.socket,
                    addr.as_ptr().cast::<SOCKADDR>(),
                    QOSTrafficTypeVoice,
                    QOS_NON_ADAPTIVE_FLOW,
                    &mut flow,
                )
            };
            if ok == 0 {
                debug!(error = %std::io::Error::last_os_error(), "qWAVE flow not created");
                return None;
            }
            let value = u32::from(dscp);
            // SAFETY: `value` is a live u32 whose size is passed; a null OVERLAPPED makes the call
            // synchronous.
            let set = unsafe {
                QOSSetFlow(
                    self.handle as _,
                    flow,
                    QOSSetOutgoingDSCPValue,
                    std::mem::size_of::<u32>() as u32,
                    (&value as *const u32).cast(),
                    0,
                    std::ptr::null_mut(),
                )
            };
            if set == 0 {
                debug!(error = %std::io::Error::last_os_error(), "qWAVE keeps the Voice default marking");
            }
            Some(flow)
        }

        pub(super) fn remove(&self, flow: u32) {
            // SAFETY: the handle is open and `flow` came from `QOSAddSocketToFlow` on this socket.
            unsafe { QOSRemoveSocketFromFlow(self.handle as _, self.socket, flow, 0) };
        }
    }

    impl Drop for Qos {
        fn drop(&mut self) {
            // SAFETY: created by `QOSCreateHandle` and closed exactly once, after every flow
            // (flows hold the marker that owns this handle).
            unsafe { QOSCloseHandle(self.handle as _) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The version of quinn-udp whose send path was read for the limitation documented above.
    const CHECKED_QUINN_UDP: &str = "0.5.15";

    /// A tripwire for the day QUIC packets can carry a DSCP.
    ///
    /// quinn-udp 0.5 attaches a per-packet `IP_TOS`/`IPV6_TCLASS` control message holding only the
    /// ECN bits (`quinn-udp/src/unix.rs`, `prepare_msg`: `encoder.push(IPPROTO_IP, IP_TOS, ecn)`).
    /// The kernel uses that instead of the socket option, so the EF mark [`mark_socket`] sets
    /// never reaches the wire for QUIC. Windows needs qWAVE for a different reason: socket TOS
    /// options are ignored there unless a Group Policy allows them.
    ///
    /// When the dependency moves past the version checked here, this test fails on purpose.
    /// Re-read quinn-udp's send path
    /// (<https://github.com/quinn-rs/quinn/blob/main/quinn-udp/src/unix.rs>): if a transmit can
    /// carry a whole TOS byte, hand it `DSCP_EF << 2` from the endpoint and drop the qWAVE flow
    /// wherever that covers it. Otherwise raise the constant here and the version named in
    /// `docs/protocol/wire.md`.
    #[test]
    fn quic_packets_still_overwrite_the_dscp_mark() {
        // The workspace lock file is the one place the real version is decided.
        let lock = include_str!("../../../Cargo.lock");
        let version = lock
            .split("name = \"quinn-udp\"")
            .nth(1)
            .and_then(|rest| rest.split_once("version = \""))
            .and_then(|(_, rest)| rest.split('"').next())
            .expect("quinn-udp is in the workspace lock file");
        assert_eq!(
            version, CHECKED_QUINN_UDP,
            "quinn-udp changed: check whether it can carry a DSCP now (see this test's comment)"
        );
    }

    #[test]
    fn marking_is_best_effort_and_never_fails() {
        let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let marker = Marker::for_socket(&socket);
        // qWAVE may refuse loopback or be missing: either way nothing panics or errors.
        let flow = marker.mark_peer("127.0.0.1:9".parse().unwrap());
        drop(flow);
        #[cfg(any(target_os = "linux", target_os = "android"))]
        assert_eq!(
            socket2::SockRef::from(&socket).tos().unwrap(),
            u32::from(DSCP_EF) << 2
        );

        let tcp = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        mark_socket(socket2::SockRef::from(&tcp), false);
    }
}
