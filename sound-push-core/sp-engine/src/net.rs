//! Local network helpers.

use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use sp_discovery::candidates::rank;

use crate::EngineError;

/// Delay between starting one candidate and the next (plan §17.3, happy-eyeballs style).
pub const CANDIDATE_STAGGER: Duration = Duration::from_millis(250);
/// Candidates raced at most; the list is ranked, so the rest would be the least likely anyway.
pub const MAX_CANDIDATES: usize = 8;

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

/// Race `addrs` best-first with a [`CANDIDATE_STAGGER`] head start per candidate (plan §17.3).
///
/// Every candidate keeps running once it started, so a slow best candidate still wins if it
/// completes first. The first success returns, which drops the [`tokio::task::JoinSet`] and
/// aborts the attempts still in flight. When all of them fail, the error of the best-ranked
/// candidate is reported: it is the one the user's network is expected to use.
pub async fn race_candidates<T, F, Fut>(
    addrs: Vec<SocketAddr>,
    connect: F,
) -> Result<T, EngineError>
where
    T: Send + 'static,
    F: Fn(SocketAddr) -> Fut,
    Fut: Future<Output = Result<T, EngineError>> + Send + 'static,
{
    let mut attempts = tokio::task::JoinSet::new();
    for (i, addr) in addrs.into_iter().take(MAX_CANDIDATES).enumerate() {
        let attempt = connect(addr);
        attempts.spawn(async move {
            tokio::time::sleep(CANDIDATE_STAGGER * i as u32).await;
            (i, attempt.await)
        });
    }
    let mut best: Option<(usize, EngineError)> = None;
    while let Some(joined) = attempts.join_next().await {
        match joined {
            Ok((_, Ok(value))) => return Ok(value),
            Ok((i, Err(e))) if best.as_ref().is_none_or(|(rank, _)| i < *rank) => {
                best = Some((i, e));
            }
            Ok(_) => {}
            // A panicking connect attempt must not take the others with it.
            Err(e) => tracing::warn!(error = %e, "dial attempt failed"),
        }
    }
    Err(best.map_or(EngineError::Unreachable, |(_, e)| e))
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
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    fn addrs(n: u16) -> Vec<SocketAddr> {
        (1..=n)
            .map(|i| SocketAddr::from(([10, 0, 0, i as u8], 1)))
            .collect()
    }

    #[tokio::test(start_paused = true)]
    async fn the_first_connection_to_succeed_wins_and_the_others_are_cancelled() {
        let started = Arc::new(AtomicUsize::new(0));
        let finished = Arc::new(AtomicUsize::new(0));
        let winner = SocketAddr::from(([10, 0, 0, 3], 1));
        let result = race_candidates(addrs(4), |addr| {
            let (started, finished) = (started.clone(), finished.clone());
            async move {
                started.fetch_add(1, Ordering::Relaxed);
                // The best candidate is a black hole; the third answers immediately.
                if addr != winner {
                    tokio::time::sleep(Duration::from_secs(30)).await;
                }
                finished.fetch_add(1, Ordering::Relaxed);
                Ok(addr)
            }
        })
        .await;

        assert_eq!(result.unwrap(), winner);
        // Three candidates started (2 × 250 ms of stagger), and only the winner ran to the end.
        assert_eq!(started.load(Ordering::Relaxed), 3);
        assert_eq!(finished.load(Ordering::Relaxed), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn a_first_candidate_that_answers_quickly_is_not_overtaken() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let result = race_candidates(addrs(4), |addr| {
            let attempts = attempts.clone();
            async move {
                attempts.fetch_add(1, Ordering::Relaxed);
                tokio::time::sleep(Duration::from_millis(50)).await;
                Ok(addr)
            }
        })
        .await;
        assert_eq!(result.unwrap(), SocketAddr::from(([10, 0, 0, 1], 1)));
        assert_eq!(attempts.load(Ordering::Relaxed), 1, "no stagger elapsed");
    }

    #[tokio::test(start_paused = true)]
    async fn all_failed_reports_the_best_candidates_error_and_races_at_most_eight() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let error = race_candidates::<SocketAddr, _, _>(addrs(12), |addr| {
            let attempts = attempts.clone();
            let best = addr == SocketAddr::from(([10, 0, 0, 1], 1));
            async move {
                attempts.fetch_add(1, Ordering::Relaxed);
                // The best candidate fails long after the others, and still owns the error.
                let after = if best { 5_000 } else { 10 };
                tokio::time::sleep(Duration::from_millis(after)).await;
                Err(EngineError::AudioDevice(addr.to_string()))
            }
        })
        .await
        .unwrap_err();
        assert_eq!(error, EngineError::AudioDevice("10.0.0.1:1".into()));
        assert_eq!(attempts.load(Ordering::Relaxed), MAX_CANDIDATES);
    }

    #[tokio::test]
    async fn no_candidates_is_unreachable() {
        let result =
            race_candidates::<SocketAddr, _, _>(Vec::new(), |addr| async move { Ok(addr) });
        assert_eq!(result.await.unwrap_err(), EngineError::Unreachable);
    }

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
