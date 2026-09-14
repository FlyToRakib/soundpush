//! Local network helpers.

use std::net::{IpAddr, SocketAddr};

use sp_discovery::candidates::rank;

/// Addresses other devices can use to reach this one, best first.
pub fn local_addresses(port: u16, include_loopback: bool) -> Vec<SocketAddr> {
    let mut addrs: Vec<SocketAddr> = if_addrs::get_if_addrs()
        .map(|ifaces| {
            ifaces
                .into_iter()
                .filter(|i| include_loopback || !i.is_loopback())
                .map(|i| SocketAddr::new(i.ip(), port))
                .filter(|a| match a.ip() {
                    // Link-local IPv6 is useless to another device: it needs a scope id that
                    // only means something here.
                    IpAddr::V6(v6) => !v6.is_multicast() && !v6.is_unicast_link_local(),
                    IpAddr::V4(v4) => !v4.is_multicast(),
                })
                .collect()
        })
        .unwrap_or_default();
    if include_loopback && !addrs.iter().any(|a| a.ip().is_loopback()) {
        addrs.push(SocketAddr::from(([127, 0, 0, 1], port)));
    }
    sort_candidates(&mut addrs, include_loopback);
    addrs
}

/// Link-local IPv6 without a scope id can never complete a handshake: the socket picks some
/// interface, and the peer's replies arrive from a scoped address QUIC treats as a stranger.
/// Discovery and saved addresses from other devices carry no usable scope, so drop them.
fn unusable_link_local(a: &SocketAddr) -> bool {
    match a {
        SocketAddr::V6(v6) => v6.ip().is_unicast_link_local() && v6.scope_id() == 0,
        SocketAddr::V4(_) => false,
    }
}

/// Order dial candidates best-first. Loopback is kept only when explicitly allowed.
pub fn sort_candidates(addrs: &mut Vec<SocketAddr>, include_loopback: bool) {
    addrs.retain(|a| {
        a.port() != 0 && !unusable_link_local(a) && (include_loopback || !a.ip().is_loopback())
    });
    addrs.sort_by_key(|a| if a.ip().is_loopback() { 254 } else { rank(a) });
    addrs.dedup();
}

/// Parse "ip", "ip:port", "[v6]:port" or "hostname[:port]".
pub async fn resolve(input: &str, default_port: u16) -> Vec<SocketAddr> {
    let input = input.trim();
    if let Ok(addr) = input.parse::<SocketAddr>() {
        return vec![addr];
    }
    if let Ok(ip) = input.trim_matches(['[', ']']).parse::<IpAddr>() {
        return vec![SocketAddr::new(ip, default_port)];
    }
    if input.is_empty() || input.len() > 253 {
        return Vec::new();
    }
    let with_port = if input
        .rsplit_once(':')
        .is_some_and(|(_, p)| p.parse::<u16>().is_ok())
    {
        input.to_string()
    } else {
        format!("{input}:{default_port}")
    };
    match tokio::time::timeout(
        std::time::Duration::from_secs(3),
        tokio::net::lookup_host(with_port),
    )
    .await
    {
        Ok(Ok(iter)) => iter.collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn resolves_literals() {
        assert_eq!(
            resolve("192.168.1.4", 47650).await,
            vec!["192.168.1.4:47650".parse().unwrap()]
        );
        assert_eq!(
            resolve("10.0.0.2:9000", 47650).await,
            vec!["10.0.0.2:9000".parse().unwrap()]
        );
        assert_eq!(
            resolve("[fe80::1]", 1).await,
            vec!["[fe80::1]:1".parse().unwrap()]
        );
        assert!(resolve("", 1).await.is_empty());
    }

    #[test]
    fn unscoped_link_local_is_dropped() {
        let mut a: Vec<SocketAddr> = vec![
            "[fe80::b023:4ff:feac:9b4b]:5".parse().unwrap(),
            "[fe80::b023:4ff:feac:9b4b%18]:5".parse().unwrap(),
            "192.168.0.2:5".parse().unwrap(),
        ];
        sort_candidates(&mut a, false);
        assert_eq!(a.len(), 2);
        assert!(
            !a.iter()
                .any(|x| matches!(x, SocketAddr::V6(v) if v.scope_id() == 0))
        );
    }

    #[test]
    fn loopback_only_when_allowed() {
        let mut a = vec![
            "127.0.0.1:5".parse().unwrap(),
            "192.168.0.2:5".parse().unwrap(),
        ];
        sort_candidates(&mut a, false);
        assert_eq!(a.len(), 1);
        let mut b = vec![
            "127.0.0.1:5".parse().unwrap(),
            "192.168.0.2:5".parse().unwrap(),
        ];
        sort_candidates(&mut b, true);
        assert_eq!(b[1], "127.0.0.1:5".parse::<SocketAddr>().unwrap());
    }
}
