//! Ranking of candidate addresses for a peer.
//!
//! The dialer races the best candidates (250 ms stagger) and keeps the first
//! authenticated connection.

use std::net::{IpAddr, SocketAddr};

/// Lower is better.
pub fn rank(addr: &SocketAddr) -> u8 {
    match addr.ip() {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            if v4.is_loopback() || v4.is_unspecified() || v4.is_broadcast() {
                255
            } else if o[0] == 192 && o[1] == 168 {
                0
            } else if o[0] == 10 {
                1
            } else if o[0] == 172 && (16..=31).contains(&o[1]) {
                // Docker/WSL bridges commonly use 172.17–172.31.
                if o[1] >= 17 { 20 } else { 2 }
            } else if v4.is_link_local() {
                30
            } else {
                10
            }
        }
        IpAddr::V6(v6) => {
            if v6.is_loopback() || v6.is_unspecified() {
                255
            } else if (v6.segments()[0] & 0xffc0) == 0xfe80 {
                // link-local
                15
            } else if (v6.segments()[0] & 0xfe00) == 0xfc00 {
                // unique local
                5
            } else {
                12
            }
        }
    }
}

/// Deduplicate and sort candidates best-first, dropping unusable addresses.
pub fn order(mut addrs: Vec<SocketAddr>) -> Vec<SocketAddr> {
    addrs.retain(|a| rank(a) < 255 && a.port() != 0);
    addrs.sort_by_key(|a| (rank(a), a.is_ipv6()));
    addrs.dedup();
    addrs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_home_lan_and_drops_loopback() {
        let input: Vec<SocketAddr> = vec![
            "172.17.0.1:1".parse().unwrap(),
            "127.0.0.1:1".parse().unwrap(),
            "[fe80::1]:1".parse().unwrap(),
            "192.168.1.9:1".parse().unwrap(),
            "10.0.0.4:1".parse().unwrap(),
            "192.168.1.9:1".parse().unwrap(),
        ];
        let out = order(input);
        assert_eq!(out[0], "192.168.1.9:1".parse().unwrap());
        assert_eq!(out[1], "10.0.0.4:1".parse().unwrap());
        assert_eq!(out.len(), 4);
        assert_eq!(out.last().unwrap(), &"172.17.0.1:1".parse::<SocketAddr>().unwrap());
    }
}
