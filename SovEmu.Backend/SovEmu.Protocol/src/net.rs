//! The one address clients send a session's media to, worked out the same way by the worker
//! (which advertises it) and the server (whose admin Network page shows it).

use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};

/// The one address clients send media to: `--advertise-ip` if given, else the bound address,
/// else (for a wildcard bind) the machine's primary address.
///
/// Exactly one, because str0m's ICE answers a connectivity check only when the packet's
/// destination is one of its local candidates, and with a wildcard bind the OS does not tell us
/// which local address a datagram arrived on. So every received datagram is tagged with this
/// address. In a container that is the host's address (`SOVEMU_ADVERTISE_IP`), which the
/// container could not discover on its own.
pub fn advertised_addr(advertise: Option<IpAddr>, bound: SocketAddr) -> SocketAddr {
    let ip = advertise
        .or_else(|| (!bound.ip().is_unspecified()).then(|| bound.ip()))
        .or_else(primary_ip)
        .unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST));
    SocketAddr::new(ip, bound.port())
}

/// The address the OS would use for outbound traffic. Connecting a UDP socket sends nothing;
/// 192.0.2.1 is TEST-NET-1, reserved for documentation, so no real host is involved.
pub fn primary_ip() -> Option<IpAddr> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect((Ipv4Addr::new(192, 0, 2, 1), 9)).ok()?;
    let ip = socket.local_addr().ok()?.ip();
    (!ip.is_unspecified()).then_some(ip)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_explicit_advertise_address_wins() {
        let bound: SocketAddr = "0.0.0.0:50000".parse().unwrap();
        let ip: IpAddr = "203.0.113.7".parse().unwrap();
        assert_eq!(advertised_addr(Some(ip), bound), SocketAddr::new(ip, 50000));
    }

    #[test]
    fn a_specific_bind_is_advertised_as_is() {
        let bound: SocketAddr = "127.0.0.1:50001".parse().unwrap();
        assert_eq!(advertised_addr(None, bound), bound);
    }

    #[test]
    fn a_wildcard_bind_advertises_a_real_address() {
        let bound: SocketAddr = "0.0.0.0:50002".parse().unwrap();
        let addr = advertised_addr(None, bound);
        assert!(!addr.ip().is_unspecified());
        assert_eq!(addr.port(), 50002);
    }
}
