//! Where the relay may send: only to this server's own game sessions.
//!
//! A player's client knows a session's worker by its advertised address and port, so that is the
//! only peer a relayed allocation may name. The relay then reaches the worker on the machine
//! itself (loopback, when workers listen on every address), which is why its relay sockets need
//! not be reachable from the network at all, and nothing else on the LAN can be reached through
//! it.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

/// The game sessions' addresses, and how the relay reaches them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Peers {
    /// The address clients know workers at (`SovEmu__Streaming__AdvertiseAddress`, or the one
    /// worked out in its place).
    pub advertised: IpAddr,
    /// The address workers listen on (`SovEmu__Streaming__Udp__Bind`): every address, or one.
    pub bind: IpAddr,
    /// The session ports, first and last.
    pub ports: (u16, u16),
}

impl Peers {
    /// Where the relay sends to and receives from the workers, and binds its relay sockets: the
    /// loopback address when workers listen on every address, else the one they listen on.
    pub(crate) fn local(&self) -> IpAddr {
        match self.bind {
            IpAddr::V4(ip) if ip.is_unspecified() => IpAddr::V4(Ipv4Addr::LOCALHOST),
            IpAddr::V6(ip) if ip.is_unspecified() => IpAddr::V6(Ipv6Addr::LOCALHOST),
            ip => ip,
        }
    }

    fn session_port(&self, port: u16) -> bool {
        (self.ports.0..=self.ports.1).contains(&port)
    }

    /// Where a client's packet for `peer` goes, if `peer` is one of the sessions.
    pub(crate) fn route(&self, peer: SocketAddr) -> Option<SocketAddr> {
        (peer.ip() == self.advertised && self.session_port(peer.port()))
            .then(|| SocketAddr::new(self.local(), peer.port()))
    }

    /// Who a packet the relay received from `from` is from, as the client knows it, if it came
    /// from one of the sessions.
    pub(crate) fn source(&self, from: SocketAddr) -> Option<SocketAddr> {
        (from.ip() == self.local() && self.session_port(from.port()))
            .then(|| SocketAddr::new(self.advertised, from.port()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peers(bind: &str) -> Peers {
        Peers {
            advertised: "192.168.1.10".parse().unwrap(),
            bind: bind.parse().unwrap(),
            ports: (50000, 50009),
        }
    }

    #[test]
    fn only_the_sessions_are_reachable() {
        let p = peers("0.0.0.0");
        assert_eq!(
            p.route("192.168.1.10:50000".parse().unwrap()),
            Some("127.0.0.1:50000".parse().unwrap())
        );
        assert_eq!(
            p.route("192.168.1.10:50009".parse().unwrap()),
            Some("127.0.0.1:50009".parse().unwrap())
        );
        // Another port on this machine, another machine on the LAN, the machine's loopback.
        assert_eq!(p.route("192.168.1.10:8080".parse().unwrap()), None);
        assert_eq!(p.route("192.168.1.10:50010".parse().unwrap()), None);
        assert_eq!(p.route("192.168.1.11:50000".parse().unwrap()), None);
        assert_eq!(p.route("127.0.0.1:50000".parse().unwrap()), None);
    }

    #[test]
    fn replies_are_named_as_the_client_knows_the_session() {
        let p = peers("0.0.0.0");
        assert_eq!(
            p.source("127.0.0.1:50003".parse().unwrap()),
            Some("192.168.1.10:50003".parse().unwrap())
        );
        assert_eq!(p.source("127.0.0.1:9".parse().unwrap()), None);
        assert_eq!(p.source("192.168.1.10:50003".parse().unwrap()), None);
    }

    #[test]
    fn workers_on_one_address_are_reached_there() {
        let p = peers("192.168.1.10");
        assert_eq!(p.local(), "192.168.1.10".parse::<IpAddr>().unwrap());
        assert_eq!(
            p.route("192.168.1.10:50001".parse().unwrap()),
            Some("192.168.1.10:50001".parse().unwrap())
        );
        assert_eq!(peers("::").local(), "::1".parse::<IpAddr>().unwrap());
    }
}
