//! Who a request came from. The connection's own address, unless it comes from a reverse proxy
//! listed in `SovEmu__Network__TrustedProxyNetwork`: then the proxy's `X-Forwarded-For` says who it forwarded
//! for (the last address in it that is not itself a trusted proxy). A forwarded header from
//! anyone else is ignored, since it could say anything, and the first one is logged, once, as
//! a likely missing setting.
//!
//! The address throttles sign-ins per client (`SovEmu__Authentication__Lockout__MaxFailuresPerAddress`) and goes on
//! each request's span (`client.address`).

use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};

use axum::extract::{ConnectInfo, Request, State};
use axum::middleware::Next;
use axum::response::Response;

use crate::AppState;

/// An address range: `10.0.0.0/8`, `192.168.1.10` (one address), `::1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Network {
    addr: IpAddr,
    prefix: u8,
}

impl std::str::FromStr for Network {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        let (addr, prefix) = match s.split_once('/') {
            Some((addr, prefix)) => (addr, Some(prefix)),
            None => (s, None),
        };
        let addr: IpAddr = addr.parse().map_err(|_| format!("{s:?} is not an address or range"))?;
        let max = if addr.is_ipv4() { 32 } else { 128 };
        let prefix = match prefix {
            Some(p) => p
                .parse::<u8>()
                .ok()
                .filter(|p| *p <= max)
                .ok_or_else(|| format!("{s:?}: bad prefix"))?,
            None => max,
        };
        Ok(Self { addr, prefix })
    }
}

impl std::fmt::Display for Network {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.addr, self.prefix)
    }
}

impl Network {
    pub fn contains(&self, ip: IpAddr) -> bool {
        // An IPv4 client reached over IPv6 (::ffff:a.b.c.d) is still that IPv4 client.
        let ip = match ip {
            IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(ip, IpAddr::V4),
            v4 => v4,
        };
        match (self.addr, ip) {
            (IpAddr::V4(net), IpAddr::V4(ip)) => {
                let mask =
                    if self.prefix == 0 { 0 } else { u32::MAX << (32 - u32::from(self.prefix)) };
                u32::from(net) & mask == u32::from(ip) & mask
            }
            (IpAddr::V6(net), IpAddr::V6(ip)) => {
                let mask =
                    if self.prefix == 0 { 0 } else { u128::MAX << (128 - u32::from(self.prefix)) };
                u128::from(net) & mask == u128::from(ip) & mask
            }
            _ => false,
        }
    }
}

/// The client's address, if known (tests and other in-process callers have none). The type is
/// `sovemu_identity`'s, whose sign-in throttle reads it.
pub(crate) use sovemu_identity::ClientIp;

/// The client of a request that arrived from `peer` with `forwarded_for`, given the trusted
/// proxies. Walks `X-Forwarded-For` from its end (the hop nearest us), past trusted proxies.
pub(crate) fn resolve(
    peer: Option<IpAddr>,
    forwarded_for: Option<&str>,
    trusted: &[Network],
) -> Option<IpAddr> {
    let peer = peer?;
    let is_trusted = |ip: IpAddr| trusted.iter().any(|n| n.contains(ip));
    if !is_trusted(peer) {
        return Some(peer);
    }
    let Some(header) = forwarded_for else { return Some(peer) };
    let mut client = peer;
    for hop in header.rsplit(',') {
        let Ok(ip) = hop.trim().parse::<IpAddr>() else { break };
        client = ip;
        if !is_trusted(ip) {
            break;
        }
    }
    Some(client)
}

/// Works out each request's client, for the handlers (`ClientIp`) and its span.
pub(crate) async fn remember(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    static WARNED: AtomicBool = AtomicBool::new(false);
    let peer = request.extensions().get::<ConnectInfo<SocketAddr>>().map(|c| c.0.ip());
    let forwarded = request.headers().get("x-forwarded-for").and_then(|v| v.to_str().ok());
    let trusted = &state.config.trusted_proxies;
    if let (Some(peer), Some(_)) = (peer, forwarded)
        && !trusted.iter().any(|n| n.contains(peer))
        && !WARNED.swap(true, Ordering::Relaxed)
    {
        tracing::warn!(
            peer = %peer,
            "X-Forwarded-For from a peer not in SovEmu__Network__TrustedProxyNetwork: ignored (if this is your \
             reverse proxy, list it there, or every client counts as the proxy)"
        );
    }
    let client = resolve(peer, forwarded, trusted);
    request.extensions_mut().insert(ClientIp(client));
    next.run(request).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nets(list: &[&str]) -> Vec<Network> {
        list.iter().map(|s| s.parse().unwrap()).collect()
    }

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn ranges_contain_their_addresses() {
        let lan: Network = "192.168.1.0/24".parse().unwrap();
        assert!(lan.contains(ip("192.168.1.77")));
        assert!(!lan.contains(ip("192.168.2.1")));
        assert!(lan.contains(ip("::ffff:192.168.1.5")), "IPv4 over IPv6");
        let one: Network = "10.0.0.5".parse().unwrap();
        assert!(one.contains(ip("10.0.0.5")) && !one.contains(ip("10.0.0.6")));
        let v6: Network = "fd00::/8".parse().unwrap();
        assert!(v6.contains(ip("fd12::1")) && !v6.contains(ip("fe80::1")));
        assert!("10.0.0.0/33".parse::<Network>().is_err());
        assert!("proxy".parse::<Network>().is_err());
    }

    #[test]
    fn a_trusted_proxy_says_who_it_forwarded_for() {
        let trusted = nets(&["10.0.0.0/8"]);
        // Direct: the peer, whatever it claims.
        assert_eq!(
            resolve(Some(ip("203.0.113.9")), Some("1.2.3.4"), &trusted),
            Some(ip("203.0.113.9"))
        );
        // Through the proxy: the last hop that is not a proxy.
        assert_eq!(
            resolve(Some(ip("10.0.0.2")), Some("198.51.100.7"), &trusted),
            Some(ip("198.51.100.7"))
        );
        assert_eq!(
            resolve(Some(ip("10.0.0.2")), Some("6.6.6.6, 198.51.100.7, 10.0.0.3"), &trusted),
            Some(ip("198.51.100.7")),
            "a client cannot prepend its own claim"
        );
        // From the proxy itself, no header: the proxy.
        assert_eq!(resolve(Some(ip("10.0.0.2")), None, &trusted), Some(ip("10.0.0.2")));
        assert_eq!(resolve(None, Some("1.2.3.4"), &trusted), None);
        // Nothing trusted (the default): always the peer.
        assert_eq!(resolve(Some(ip("10.0.0.2")), Some("1.2.3.4"), &[]), Some(ip("10.0.0.2")));
    }
}
