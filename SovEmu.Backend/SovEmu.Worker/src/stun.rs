//! The session's address on the internet, asked of STUN servers (RFC 8489 Binding requests) from
//! the session's own socket: the address and port the home router maps that socket to. Offered
//! as a server-reflexive candidate, it lets a player away from home reach the session with no
//! port forwarded, once both sides have sent each other a connectivity check (the worker runs
//! full ICE for that).
//!
//! The requests go out when the session starts and again for each new client (a router's
//! mapping can change while nothing is sent), and the answers arrive on the same socket as the
//! game's traffic: [`Reflexive::handle`] takes them out before the peer sees them.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, ToSocketAddrs, UdpSocket};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const MAGIC: u32 = 0x2112_A442;
const BINDING_REQUEST: u16 = 0x0001;
const BINDING_SUCCESS: u16 = 0x0101;
const MAPPED_ADDRESS: u16 = 0x0001;
const XOR_MAPPED_ADDRESS: u16 = 0x0020;

/// How long a new client's answer waits for the servers, at most.
const WAIT: Duration = Duration::from_millis(1000);
/// Ask again when nothing came back in this long (UDP loses packets).
const RETRY: Duration = Duration::from_millis(300);
/// Answers older than this are asked for again before they're offered.
const STALE: Duration = Duration::from_secs(20);
/// How long looking the servers' names up may take, when the session starts.
const RESOLVE: Duration = Duration::from_secs(2);

/// A Binding request with `id` as its transaction id.
fn request(id: &[u8; 12]) -> [u8; 20] {
    let mut m = [0u8; 20];
    m[0..2].copy_from_slice(&BINDING_REQUEST.to_be_bytes());
    // Length 0: no attributes.
    m[4..8].copy_from_slice(&MAGIC.to_be_bytes());
    m[8..20].copy_from_slice(id);
    m
}

/// A Binding success response's transaction id and mapped address (XOR-MAPPED-ADDRESS, else the
/// old MAPPED-ADDRESS); `None` for anything else.
fn response(data: &[u8]) -> Option<([u8; 12], SocketAddr)> {
    if data.len() < 20
        || u16::from_be_bytes([data[0], data[1]]) != BINDING_SUCCESS
        || u32::from_be_bytes(data[4..8].try_into().ok()?) != MAGIC
    {
        return None;
    }
    let id: [u8; 12] = data[8..20].try_into().ok()?;
    let len = usize::from(u16::from_be_bytes([data[2], data[3]]));
    let attrs = data.get(20..20 + len)?;
    let (mut xor, mut plain) = (None, None);
    let mut at = 0;
    while at + 4 <= attrs.len() {
        let kind = u16::from_be_bytes([attrs[at], attrs[at + 1]]);
        let size = usize::from(u16::from_be_bytes([attrs[at + 2], attrs[at + 3]]));
        let value = attrs.get(at + 4..at + 4 + size)?;
        match kind {
            XOR_MAPPED_ADDRESS => xor = address(value, Some(&id)),
            MAPPED_ADDRESS => plain = address(value, None),
            _ => {}
        }
        // Attributes are padded to four bytes.
        at += 4 + size.div_ceil(4) * 4;
    }
    Some((id, xor.or(plain)?))
}

/// A (XOR-)MAPPED-ADDRESS value; `id` given, it is XOR'd with the cookie and transaction id.
fn address(value: &[u8], id: Option<&[u8; 12]>) -> Option<SocketAddr> {
    let family = *value.get(1)?;
    let mut port = u16::from_be_bytes([*value.get(2)?, *value.get(3)?]);
    let mut key = [0u8; 16];
    key[..4].copy_from_slice(&MAGIC.to_be_bytes());
    if let Some(id) = id {
        key[4..].copy_from_slice(id);
        port ^= (MAGIC >> 16) as u16;
    }
    let ip = match family {
        0x01 => {
            let mut b: [u8; 4] = value.get(4..8)?.try_into().ok()?;
            if id.is_some() {
                b.iter_mut().zip(&key).for_each(|(b, k)| *b ^= k);
            }
            IpAddr::V4(Ipv4Addr::from(b))
        }
        0x02 => {
            let mut b: [u8; 16] = value.get(4..20)?.try_into().ok()?;
            if id.is_some() {
                b.iter_mut().zip(&key).for_each(|(b, k)| *b ^= k);
            }
            IpAddr::V6(Ipv6Addr::from(b))
        }
        _ => return None,
    };
    Some(SocketAddr::new(ip, port))
}

/// The session's server-reflexive address, as the STUN servers last saw it.
#[derive(Debug)]
pub(crate) struct Reflexive {
    /// The servers, by address (their names looked up once).
    servers: Vec<SocketAddr>,
    /// Requests asked and not yet answered: transaction id, and when.
    pending: Vec<[u8; 12]>,
    asked: Option<Instant>,
    sent: Option<Instant>,
    /// Every address heard back for the current ask: more than one means the router gives each
    /// destination its own port (a symmetric NAT), which no reflexive address gets through.
    heard: Vec<SocketAddr>,
    found: Option<(SocketAddr, Instant)>,
    warned: bool,
}

impl Reflexive {
    /// For `servers` (`host:port`), only those of `socket`'s address family. Looking their names
    /// up gives up after a couple of seconds: a session starts without them, never late.
    pub(crate) fn new(servers: &[String], socket: &UdpSocket) -> Self {
        let v6 = socket.local_addr().is_ok_and(|a| a.is_ipv6());
        let names = servers.to_vec();
        let (tx, rx) = mpsc::channel();
        let _ = std::thread::Builder::new().name("stun-resolve".into()).spawn(move || {
            let found: Vec<SocketAddr> = names
                .iter()
                .filter_map(|name| match name.to_socket_addrs() {
                    Ok(mut addrs) => addrs.find(|a| a.is_ipv6() == v6),
                    Err(e) => {
                        tracing::warn!(server = %name, error = %e, "STUN server not found");
                        None
                    }
                })
                .collect();
            let _ = tx.send(found);
        });
        let servers = if servers.is_empty() {
            Vec::new()
        } else {
            rx.recv_timeout(RESOLVE).unwrap_or_else(|_| {
                tracing::warn!("looking up the STUN servers took too long: none this session");
                Vec::new()
            })
        };
        Self {
            servers,
            pending: Vec::new(),
            asked: None,
            sent: None,
            heard: Vec::new(),
            found: None,
            warned: false,
        }
    }

    /// Asks every server again.
    pub(crate) fn ask(&mut self, socket: &UdpSocket) {
        if self.servers.is_empty() {
            return;
        }
        self.pending.clear();
        self.heard.clear();
        self.asked = Some(Instant::now());
        self.send(socket);
    }

    fn send(&mut self, socket: &UdpSocket) {
        for server in &self.servers {
            let mut id = [0u8; 12];
            if getrandom::fill(&mut id).is_err() {
                continue;
            }
            if let Err(e) = socket.send_to(&request(&id), server) {
                tracing::debug!(server = %server, error = %e, "STUN request not sent");
                continue;
            }
            self.pending.push(id);
        }
        self.sent = Some(Instant::now());
    }

    /// Takes a datagram that answers one of this session's requests (true), recording the
    /// address; anything else is left for the peer (false).
    pub(crate) fn handle(&mut self, data: &[u8]) -> bool {
        let Some((id, addr)) = response(data) else { return false };
        let Some(i) = self.pending.iter().position(|p| *p == id) else { return false };
        self.pending.swap_remove(i);
        if !self.heard.contains(&addr) {
            self.heard.push(addr);
        }
        if self.heard.len() > 1 && !self.warned {
            self.warned = true;
            tracing::warn!(
                addresses = ?self.heard,
                "the router maps this session to a different port for each destination (a \
                 symmetric NAT): players away from home need the built-in relay (port 3478 forwarded)"
            );
        }
        if self.found.is_none_or(|(old, _)| old != addr) {
            tracing::info!(address = %addr, "public address from STUN");
        }
        self.found = Some((addr, Instant::now()));
        true
    }

    /// The address to offer a new client: asked again first if the last answer is old, and
    /// waited for (up to a second from the ask) if it hasn't come yet. Datagrams that arrive
    /// meanwhile and aren't answers are dropped: a client being replaced loses a moment.
    pub(crate) fn current(&mut self, udp: &crate::udp::Udp) -> Option<SocketAddr> {
        if self.servers.is_empty() {
            return None;
        }
        let socket = udp.primary();
        let fresh = self.found.is_some_and(|(_, at)| at.elapsed() < STALE);
        if !fresh && self.asked.is_none_or(|at| at.elapsed() >= WAIT) {
            self.ask(socket);
        }
        let until = self.asked.map_or_else(Instant::now, |at| at + WAIT);
        let mut buf = [0u8; 1500];
        // Every server answered, or one did and the answer is current.
        while !self.pending.is_empty()
            && !(self.found.is_some_and(|(_, at)| self.asked.is_some_and(|a| at >= a)))
        {
            let now = Instant::now();
            if now >= until {
                break;
            }
            if self.sent.is_some_and(|at| now - at >= RETRY) {
                self.send(socket);
            }
            let wait = until.min(self.sent.map_or(until, |at| at + RETRY)) - now;
            match udp.recv(&mut buf, wait) {
                Ok(Some((n, _, _))) => {
                    self.handle(&buf[..n]);
                }
                Ok(None) => {}
                Err(_) => break,
            }
        }
        let found = self.found.map(|(addr, _)| addr);
        if found.is_none() {
            tracing::info!("no answer from the STUN servers: offering the local address only");
        }
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 5769, 2.2: a sample IPv4 response (192.0.2.1:32853), with SOFTWARE before it and
    /// MESSAGE-INTEGRITY and FINGERPRINT after.
    const RFC5769_IPV4: [u8; 80] = [
        0x01, 0x01, 0x00, 0x3c, 0x21, 0x12, 0xa4, 0x42, 0xb7, 0xe7, 0xa7, 0x01, 0xbc, 0x34, 0xd6,
        0x86, 0xfa, 0x87, 0xdf, 0xae, 0x80, 0x22, 0x00, 0x0b, 0x74, 0x65, 0x73, 0x74, 0x20, 0x76,
        0x65, 0x63, 0x74, 0x6f, 0x72, 0x20, 0x00, 0x20, 0x00, 0x08, 0x00, 0x01, 0xa1, 0x47, 0xe1,
        0x12, 0xa6, 0x43, 0x00, 0x08, 0x00, 0x14, 0x2b, 0x91, 0xf5, 0x99, 0xfd, 0x9e, 0x90, 0xc3,
        0x8c, 0x74, 0x89, 0xf9, 0x2a, 0xf9, 0xba, 0x53, 0xf0, 0x6b, 0xe7, 0xd7, 0x80, 0x28, 0x00,
        0x04, 0xc0, 0x7d, 0x4c, 0x96,
    ];

    /// RFC 5769, 2.3: the same, IPv6 (2001:db8:1234:5678:11:2233:4455:6677, port 32853).
    const RFC5769_IPV6: [u8; 92] = [
        0x01, 0x01, 0x00, 0x48, 0x21, 0x12, 0xa4, 0x42, 0xb7, 0xe7, 0xa7, 0x01, 0xbc, 0x34, 0xd6,
        0x86, 0xfa, 0x87, 0xdf, 0xae, 0x80, 0x22, 0x00, 0x0b, 0x74, 0x65, 0x73, 0x74, 0x20, 0x76,
        0x65, 0x63, 0x74, 0x6f, 0x72, 0x20, 0x00, 0x20, 0x00, 0x14, 0x00, 0x02, 0xa1, 0x47, 0x01,
        0x13, 0xa9, 0xfa, 0xa5, 0xd3, 0xf1, 0x79, 0xbc, 0x25, 0xf4, 0xb5, 0xbe, 0xd2, 0xb9, 0xd9,
        0x00, 0x08, 0x00, 0x14, 0xa3, 0x82, 0x95, 0x4e, 0x4b, 0xe6, 0x7b, 0xf1, 0x17, 0x84, 0xc9,
        0x7c, 0x82, 0x92, 0xc2, 0x75, 0xbf, 0xe3, 0xed, 0x41, 0x80, 0x28, 0x00, 0x04, 0xc8, 0xfb,
        0x0b, 0x4c,
    ];

    const ID: [u8; 12] = [0xb7, 0xe7, 0xa7, 0x01, 0xbc, 0x34, 0xd6, 0x86, 0xfa, 0x87, 0xdf, 0xae];

    #[test]
    fn reads_rfc_5769s_responses() {
        assert_eq!(response(&RFC5769_IPV4), Some((ID, "192.0.2.1:32853".parse().unwrap())));
        assert_eq!(
            response(&RFC5769_IPV6),
            Some((ID, "[2001:db8:1234:5678:11:2233:4455:6677]:32853".parse().unwrap()))
        );
    }

    #[test]
    fn ignores_what_is_not_a_binding_success() {
        let mut request = RFC5769_IPV4;
        request[1] = 0x01 - 0x01; // a request, not a response
        assert_eq!(response(&request), None);
        assert_eq!(response(&RFC5769_IPV4[..30]), None, "cut short");
        assert_eq!(response(&[0x80, 0x60, 0, 0]), None, "RTP");
        assert_eq!(response(&[]), None);
    }

    #[test]
    fn a_request_is_a_bare_binding_request() {
        let r = request(&ID);
        assert_eq!(&r[..8], &[0x00, 0x01, 0x00, 0x00, 0x21, 0x12, 0xa4, 0x42]);
        assert_eq!(&r[8..], &ID);
    }

    /// A session's socket alone.
    fn udp(bind: &str) -> crate::udp::Udp {
        let socket = UdpSocket::bind(bind).unwrap();
        let local = socket.local_addr().unwrap();
        crate::udp::Udp::new(socket, local)
    }

    /// A STUN server on loopback that answers every request with `mapped`.
    fn server(mapped: SocketAddr) -> SocketAddr {
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        let addr = socket.local_addr().unwrap();
        std::thread::spawn(move || {
            let mut buf = [0u8; 1500];
            while let Ok((n, from)) = socket.recv_from(&mut buf) {
                if n < 20 {
                    continue;
                }
                let mut reply = RFC5769_IPV4[..48].to_vec();
                reply[2..4].copy_from_slice(&28u16.to_be_bytes());
                reply[8..20].copy_from_slice(&buf[8..20]);
                // XOR-MAPPED-ADDRESS for `mapped` under this transaction's id.
                let SocketAddr::V4(m) = mapped else { unreachable!() };
                let port = m.port() ^ (MAGIC >> 16) as u16;
                reply[42..44].copy_from_slice(&port.to_be_bytes());
                let ip: [u8; 4] = m.ip().octets();
                let key = MAGIC.to_be_bytes();
                for i in 0..4 {
                    reply[44 + i] = ip[i] ^ key[i];
                }
                let _ = socket.send_to(&reply, from);
            }
        });
        addr
    }

    #[test]
    fn learns_the_address_from_a_server_and_leaves_other_traffic() {
        let mapped: SocketAddr = "203.0.113.9:41000".parse().unwrap();
        let stun = server(mapped);
        let udp = udp("127.0.0.1:0");
        let mut r = Reflexive::new(&[stun.to_string()], udp.primary());
        assert_eq!(r.current(&udp), Some(mapped));
        assert!(!r.handle(&RFC5769_IPV4), "an answer to a request it never sent");
        assert!(!r.handle(&[0x80, 0x60, 0, 0]), "RTP is the peer's");
    }

    #[test]
    fn no_servers_or_no_answer_offers_nothing() {
        let udp = udp("127.0.0.1:0");
        assert_eq!(Reflexive::new(&[], udp.primary()).current(&udp), None);
        // A port nothing answers on.
        let silent = UdpSocket::bind("127.0.0.1:0").unwrap();
        let mut r = Reflexive::new(&[silent.local_addr().unwrap().to_string()], udp.primary());
        let started = Instant::now();
        assert_eq!(r.current(&udp), None);
        assert!(started.elapsed() < WAIT * 3, "gives up after about a second");
    }
}
