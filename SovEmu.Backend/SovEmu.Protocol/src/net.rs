//! The addresses clients send a session's media to, worked out the same way by the worker
//! (which advertises them) and the server (whose admin Network page shows them): one IPv4
//! address, and an IPv6 one where the host has a global address.

use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket};
use std::str::FromStr;

/// The one IPv4 address clients send media to: `--advertise-ip` if given, else the bound
/// address, else (for a wildcard bind) the machine's primary address.
///
/// Exactly one, because str0m's ICE answers a connectivity check only when the packet's
/// destination is one of its local candidates, and with a wildcard bind the OS does not tell us
/// which local address a datagram arrived on. So every datagram the IPv4 socket receives is
/// tagged with this address. In a container that is the host's address
/// (`SovEmu__Streaming__AdvertiseAddress`), which the container could not discover on its own.
/// IPv6 has a socket of its own ([`advertised_v6`]), so its datagrams are told apart by family.
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

/// The IPv6 address the OS would use to reach the internet, if it is a global one. As
/// [`primary_ip`]: 2001:db8::1 is reserved for documentation, and connecting sends nothing. A
/// host with no IPv6 route (a container on a bridge network) has none.
pub fn primary_ipv6() -> Option<Ipv6Addr> {
    let socket = UdpSocket::bind((Ipv6Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect((Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1), 9)).ok()?;
    match socket.local_addr().ok()?.ip() {
        IpAddr::V6(ip) if is_global_v6(ip) => Some(ip),
        _ => None,
    }
}

/// A global unicast IPv6 address (2000::/3, the internet's), not the documentation prefix: one
/// a player elsewhere can send to. Unique local (fc00::/7) and link-local (fe80::/10) addresses
/// reach only the server's own network, which IPv4 already reaches.
pub fn is_global_v6(ip: Ipv6Addr) -> bool {
    let [first, second, ..] = ip.segments();
    first & 0xe000 == 0x2000 && !(first == 0x2001 && second == 0x0db8)
}

/// `SovEmu__Streaming__AdvertiseAddressV6`: whether, and at what address, a session is offered
/// over IPv6 as well as IPv4.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Ipv6Advertise {
    /// The host's global IPv6 address ([`primary_ipv6`]), when the session ports bind every
    /// address (`0.0.0.0`) and it has one.
    #[default]
    Auto,
    /// IPv4 only.
    Off,
    /// This address: bound if it is the host's own, else (behind a NAT that forwards it, as
    /// Docker's IPv6 can) every IPv6 address is bound and datagrams are tagged with it.
    Address(Ipv6Addr),
}

impl Ipv6Advertise {
    /// The IPv6 address and port to offer beside the IPv4 socket `bound`, if any. `detect` is
    /// [`primary_ipv6`] (a parameter for tests).
    pub fn resolve(
        self,
        bound: SocketAddr,
        detect: impl FnOnce() -> Option<Ipv6Addr>,
    ) -> Option<SocketAddr> {
        let ip = match self {
            Self::Off => None,
            Self::Address(ip) => Some(ip),
            // A socket bound to one IPv4 address (loopback, one interface) was kept to it on
            // purpose: nothing is added unasked. An IPv6 bind already is the IPv6 socket.
            Self::Auto => (bound.ip() == IpAddr::V4(Ipv4Addr::UNSPECIFIED)).then(detect).flatten(),
        }?;
        bound.is_ipv4().then(|| SocketAddr::new(IpAddr::V6(ip), bound.port()))
    }
}

/// [`Ipv6Advertise`] is `auto`, `off` or an IPv6 address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BadIpv6Advertise(String);

impl fmt::Display for BadIpv6Advertise {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "expected auto, off or an IPv6 address, got {:?}", self.0)
    }
}

impl std::error::Error for BadIpv6Advertise {}

impl FromStr for Ipv6Advertise {
    type Err = BadIpv6Advertise;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        match s.to_ascii_lowercase().as_str() {
            "" | "auto" => Ok(Self::Auto),
            "off" | "none" => Ok(Self::Off),
            _ => s
                .trim_start_matches('[')
                .trim_end_matches(']')
                .parse()
                .map(Self::Address)
                .map_err(|_| BadIpv6Advertise(s.to_owned())),
        }
    }
}

impl fmt::Display for Ipv6Advertise {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Auto => f.write_str("auto"),
            Self::Off => f.write_str("off"),
            Self::Address(ip) => write!(f, "{ip}"),
        }
    }
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

    #[test]
    fn only_global_unicast_ipv6_is_offered_on_its_own() {
        for global in ["2a02:8071:5e1:1:2c3:4dff:fe5e:6f70", "2600::1", "3fff::1"] {
            assert!(is_global_v6(global.parse().unwrap()), "{global}");
        }
        for local in
            ["::1", "::", "fd12:3456::1", "fe80::1", "ff02::1", "2001:db8::1", "::ffff:192.0.2.1"]
        {
            assert!(!is_global_v6(local.parse().unwrap()), "{local}");
        }
    }

    #[test]
    fn ipv6_is_detected_only_beside_a_wildcard_ipv4_bind() {
        let found: Ipv6Addr = "2600::5".parse().unwrap();
        let detect = || Some(found);
        let wildcard: SocketAddr = "0.0.0.0:50000".parse().unwrap();
        let expected = Some(SocketAddr::new(IpAddr::V6(found), 50000));
        assert_eq!(Ipv6Advertise::Auto.resolve(wildcard, detect), expected);
        // None found: IPv4 alone, quietly.
        assert_eq!(Ipv6Advertise::Auto.resolve(wildcard, || None), None);
        // Bound to one IPv4 address (loopback in tests): nothing added unasked.
        let one: SocketAddr = "127.0.0.1:50000".parse().unwrap();
        assert_eq!(Ipv6Advertise::Auto.resolve(one, detect), None);
        assert_eq!(Ipv6Advertise::Off.resolve(wildcard, detect), None);
    }

    #[test]
    fn a_given_ipv6_address_is_offered_on_the_session_port() {
        let ip: Ipv6Addr = "::1".parse().unwrap();
        let one: SocketAddr = "127.0.0.1:50003".parse().unwrap();
        let resolved = Ipv6Advertise::Address(ip).resolve(one, || panic!("not detected"));
        assert_eq!(resolved, Some(SocketAddr::new(IpAddr::V6(ip), 50003)));
        // A session bound to IPv6 already has no second socket.
        let v6: SocketAddr = "[::]:50003".parse().unwrap();
        assert_eq!(Ipv6Advertise::Address(ip).resolve(v6, || None), None);
    }

    #[test]
    fn the_setting_reads_auto_off_or_an_address() {
        assert_eq!("".parse(), Ok(Ipv6Advertise::Auto));
        assert_eq!(" Auto ".parse(), Ok(Ipv6Advertise::Auto));
        assert_eq!("off".parse(), Ok(Ipv6Advertise::Off));
        let ip: Ipv6Addr = "2600::5".parse().unwrap();
        assert_eq!("2600::5".parse(), Ok(Ipv6Advertise::Address(ip)));
        assert_eq!("[2600::5]".parse(), Ok(Ipv6Advertise::Address(ip)));
        assert!("192.168.1.10".parse::<Ipv6Advertise>().is_err(), "IPv4 is the other setting");
        assert!("yes".parse::<Ipv6Advertise>().is_err());
        for setting in [Ipv6Advertise::Auto, Ipv6Advertise::Off, Ipv6Advertise::Address(ip)] {
            assert_eq!(setting.to_string().parse(), Ok(setting), "round trips as an argument");
        }
    }
}
