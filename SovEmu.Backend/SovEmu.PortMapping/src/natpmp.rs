//! NAT-PMP (RFC 6886): one UDP packet to the router's port 5351 and one back. Apple's routers
//! speak it, and so does miniupnpd (OpenWrt, pfSense, OPNsense), which also answers it for PCP
//! (RFC 6887) clients' sake.
//!
//! The router forwards to the address the request came from, so the request goes out from the
//! address the router sees this server at.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use tokio::net::UdpSocket;

use crate::{Error, Granted, Protocol, Transport};

/// The port NAT-PMP routers listen on.
pub(crate) const PORT: u16 = 5351;

/// The waits between tries: RFC 6886 starts at 250 ms and doubles. It allows nine tries (a
/// minute in all); four is enough on a home network, and a router that doesn't answer in four
/// seconds isn't one that speaks NAT-PMP. Tests wait less.
const TRIES: &[Duration] = if cfg!(test) {
    &[Duration::from_millis(50), Duration::from_millis(100)]
} else {
    &[
        Duration::from_millis(250),
        Duration::from_millis(500),
        Duration::from_millis(1000),
        Duration::from_millis(2000),
    ]
};

const VERSION: u8 = 0;
const OP_EXTERNAL_ADDRESS: u8 = 0;
/// A response's opcode is the request's plus 128.
const RESPONSE: u8 = 128;

fn opcode(transport: Transport) -> u8 {
    match transport {
        Transport::Udp => 1,
        Transport::Tcp => 2,
    }
}

/// The external address request.
fn external_address_request() -> [u8; 2] {
    [VERSION, OP_EXTERNAL_ADDRESS]
}

/// A mapping request: `lifetime` 0 (and `external` 0) removes the mapping.
fn map_request(transport: Transport, internal: u16, external: u16, lifetime: u32) -> [u8; 12] {
    let mut m = [0u8; 12];
    m[0] = VERSION;
    m[1] = opcode(transport);
    // Two reserved bytes, zero.
    m[4..6].copy_from_slice(&internal.to_be_bytes());
    m[6..8].copy_from_slice(&external.to_be_bytes());
    m[8..12].copy_from_slice(&lifetime.to_be_bytes());
    m
}

/// A response's result code, checked against the request's opcode: `Ok` with the rest of the
/// packet (after the result code and the router's epoch), or what the router said.
fn result(data: &[u8], op: u8) -> Result<&[u8], Error> {
    if data.len() < 8 || data[0] != VERSION || data[1] != RESPONSE + op {
        return Err(Error::BadResponse("not a NAT-PMP answer to this request"));
    }
    match u16::from_be_bytes([data[2], data[3]]) {
        0 => Ok(&data[8..]),
        code => Err(Error::Refused { code, reason: result_reason(code).to_owned() }),
    }
}

/// RFC 6886's result codes, in words.
fn result_reason(code: u16) -> &'static str {
    match code {
        1 => "unsupported version",
        2 => "not authorised (NAT-PMP is turned off on the router)",
        3 => "network failure (the router has no internet address yet)",
        4 => "out of resources",
        5 => "unsupported request",
        _ => "refused",
    }
}

/// The external address response's address.
fn parse_external_address(data: &[u8]) -> Result<Ipv4Addr, Error> {
    let rest = result(data, OP_EXTERNAL_ADDRESS)?;
    let b: [u8; 4] =
        rest.get(..4).and_then(|b| b.try_into().ok()).ok_or(Error::BadResponse("short answer"))?;
    Ok(Ipv4Addr::from(b))
}

/// A mapping response for `internal`: the external port and lifetime granted.
fn parse_map(data: &[u8], transport: Transport, internal: u16) -> Result<Granted, Error> {
    let rest = result(data, opcode(transport))?;
    if rest.len() < 8 {
        return Err(Error::BadResponse("short answer"));
    }
    if u16::from_be_bytes([rest[0], rest[1]]) != internal {
        return Err(Error::BadResponse("an answer about another port"));
    }
    Ok(Granted {
        external: u16::from_be_bytes([rest[2], rest[3]]),
        lifetime: u32::from_be_bytes([rest[4], rest[5], rest[6], rest[7]]),
    })
}

/// A router that answers NAT-PMP.
#[derive(Debug)]
pub(crate) struct NatPmp {
    router: SocketAddr,
    local: IpAddr,
}

impl NatPmp {
    /// Asks `router` (its address and NAT-PMP port) for its external address: a router that
    /// answers is one that speaks NAT-PMP.
    pub(crate) async fn find(router: SocketAddr) -> Result<Self, Error> {
        let local = crate::local_ip_towards(router.ip())?;
        let reply = exchange(router, local, &external_address_request()).await?;
        parse_external_address(&reply)?;
        Ok(Self { router, local })
    }
}

impl crate::Gateway for NatPmp {
    fn protocol(&self) -> Protocol {
        Protocol::NatPmp
    }

    fn router(&self) -> IpAddr {
        self.router.ip()
    }

    fn local_ip(&self) -> IpAddr {
        self.local
    }

    async fn external_ip(&self) -> Result<IpAddr, Error> {
        let reply = exchange(self.router, self.local, &external_address_request()).await?;
        Ok(parse_external_address(&reply)?.into())
    }

    async fn map(
        &self,
        transport: Transport,
        internal: u16,
        external: u16,
        lifetime: u32,
    ) -> Result<Granted, Error> {
        // NAT-PMP has no permanent mappings: 0 would delete it.
        let lifetime = lifetime.max(1);
        let request = map_request(transport, internal, external, lifetime);
        let reply = exchange(self.router, self.local, &request).await?;
        let granted = parse_map(&reply, transport, internal)?;
        if granted.lifetime == 0 {
            return Err(Error::BadResponse("the router gave the mapping no time"));
        }
        Ok(granted)
    }

    async fn unmap(
        &self,
        transport: Transport,
        internal: u16,
        _external: u16,
    ) -> Result<(), Error> {
        let reply =
            exchange(self.router, self.local, &map_request(transport, internal, 0, 0)).await?;
        parse_map(&reply, transport, internal).map(drop)
    }
}

/// Sends `request` to `router` from `local`, trying again as RFC 6886 says, and returns the
/// first answer from the router.
async fn exchange(router: SocketAddr, local: IpAddr, request: &[u8]) -> Result<Vec<u8>, Error> {
    let socket = UdpSocket::bind(SocketAddr::new(local, 0)).await?;
    let mut buf = [0u8; 64];
    for wait in TRIES {
        socket.send_to(request, router).await?;
        let deadline = tokio::time::Instant::now() + *wait;
        loop {
            match tokio::time::timeout_at(deadline, socket.recv_from(&mut buf)).await {
                Err(_) => break,
                // An ICMP "port unreachable" from the last send: nobody listens there.
                Ok(Err(e)) if e.kind() == std::io::ErrorKind::ConnectionRefused => {
                    return Err(Error::Timeout);
                }
                Ok(Err(e)) => return Err(e.into()),
                // Only the router's answer counts.
                Ok(Ok((n, from))) if from == router => return Ok(buf[..n].to_vec()),
                Ok(Ok(_)) => {}
            }
        }
    }
    Err(Error::Timeout)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::Gateway;

    #[test]
    fn requests_are_rfc_6886_s() {
        assert_eq!(external_address_request(), [0, 0]);
        assert_eq!(
            map_request(Transport::Udp, 50000, 50000, 7200),
            [0, 1, 0, 0, 0xc3, 0x50, 0xc3, 0x50, 0, 0, 0x1c, 0x20]
        );
        assert_eq!(
            map_request(Transport::Tcp, 3478, 0, 0),
            [0, 2, 0, 0, 0x0d, 0x96, 0, 0, 0, 0, 0, 0]
        );
    }

    #[test]
    fn answers_are_read() {
        let external = [0, 128, 0, 0, 0, 0, 0, 9, 203, 0, 113, 7];
        assert_eq!(parse_external_address(&external).unwrap(), Ipv4Addr::new(203, 0, 113, 7));
        let mapped = [0, 129, 0, 0, 0, 0, 0, 9, 0xc3, 0x50, 0xc3, 0x51, 0, 0, 0x0e, 0x10];
        let granted = parse_map(&mapped, Transport::Udp, 50000).unwrap();
        assert_eq!((granted.external, granted.lifetime), (50001, 3600));
    }

    #[test]
    fn refusals_and_strays_are_errors() {
        let refused = [0, 129, 0, 2, 0, 0, 0, 9, 0xc3, 0x50, 0, 0, 0, 0, 0, 0];
        assert!(matches!(
            parse_map(&refused, Transport::Udp, 50000),
            Err(Error::Refused { code: 2, .. })
        ));
        // A TCP answer to a UDP request, another port's answer, and a short one.
        let tcp = [0, 130, 0, 0, 0, 0, 0, 9, 0xc3, 0x50, 0xc3, 0x50, 0, 0, 0x0e, 0x10];
        assert!(parse_map(&tcp, Transport::Udp, 50000).is_err());
        assert!(parse_map(&tcp, Transport::Tcp, 50001).is_err());
        assert!(parse_external_address(&[0, 128, 0, 0]).is_err());
    }

    /// A router on loopback that answers like a NAT-PMP one: external address 203.0.113.7,
    /// every mapping granted at the port asked for plus `shift`, for an hour at most.
    pub(crate) async fn fake_router(shift: u16) -> SocketAddr {
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let addr = socket.local_addr().unwrap();
        tokio::spawn(async move {
            let mut buf = [0u8; 64];
            loop {
                let Ok((n, from)) = socket.recv_from(&mut buf).await else { return };
                let req = &buf[..n];
                let mut reply = vec![0, RESPONSE + req[1], 0, 0, 0, 0, 0, 42];
                if req[1] == OP_EXTERNAL_ADDRESS {
                    reply.extend([203, 0, 113, 7]);
                } else {
                    let external = u16::from_be_bytes([req[6], req[7]]);
                    let lifetime = u32::from_be_bytes(req[8..12].try_into().unwrap()).min(3600);
                    let granted = if lifetime == 0 { 0 } else { external + shift };
                    reply.extend(&req[4..6]);
                    reply.extend(granted.to_be_bytes());
                    reply.extend(lifetime.to_be_bytes());
                }
                let _ = socket.send_to(&reply, from).await;
            }
        });
        addr
    }

    #[tokio::test]
    async fn a_router_is_found_and_maps_ports() {
        let router = fake_router(1).await;
        let gateway = NatPmp::find(router).await.unwrap();
        assert_eq!(gateway.external_ip().await.unwrap(), IpAddr::from([203, 0, 113, 7]));
        assert_eq!(gateway.local_ip(), IpAddr::from([127, 0, 0, 1]));
        let granted = gateway.map(Transport::Udp, 50000, 50000, 7200).await.unwrap();
        assert_eq!((granted.external, granted.lifetime), (50001, 3600), "the router's choice");
        gateway.unmap(Transport::Udp, 50000, 50001).await.unwrap();
    }

    #[tokio::test]
    async fn nobody_answering_is_a_timeout() {
        // A bound socket that never answers.
        let silent = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let started = std::time::Instant::now();
        let err = NatPmp::find(silent.local_addr().unwrap()).await.unwrap_err();
        assert!(matches!(err, Error::Timeout), "{err}");
        assert!(started.elapsed() >= Duration::from_millis(150), "every try was made");
    }
}
