//! A session's UDP sockets: the IPv4 one on its port, and beside it, where the host has a global
//! IPv6 address, an IPv6 one on the same port (sovereign-media/SovEmu#50). A player with IPv6
//! then reaches the session straight away: there is no NAT to get through.
//!
//! str0m answers a connectivity check only when its destination is one of the session's local
//! candidates, so every datagram is tagged with the address it was sent to. The IPv4 socket
//! usually binds every address, and the OS does not say which one a datagram came in on: its
//! datagrams are tagged with the advertised address ([`advertised_addr`]). The IPv6 socket binds
//! the one address it offers, so its datagrams are tagged with that. What goes out leaves by the
//! socket of the destination's family.
//!
//! Both are waited on together, on the stream's own thread, with `ppoll` (`poll` off Linux): its
//! timeout is a high-resolution timer, where a socket's read timeout is rounded up to the
//! kernel's tick (4 ms at the usual 250 Hz) and ended every frame's wait milliseconds late.
//!
//! [`advertised_addr`]: sovemu_protocol::net::advertised_addr

use std::io::{self, ErrorKind};
use std::net::{IpAddr, Ipv6Addr, SocketAddr, SocketAddrV6, UdpSocket};
use std::time::{Duration, Instant};

use rustix::event::{PollFd, PollFlags, Timespec};
use sovemu_protocol::net::Ipv6Advertise;

/// The longest a read may block on a socket `ppoll` said was readable: Linux can say so of a
/// datagram it then drops for a bad checksum, and the read must not then hang the stream.
const READ_GUARD: Duration = Duration::from_millis(1);

/// A session's sockets.
#[derive(Debug)]
pub(crate) struct Udp {
    /// The IPv4 socket (`--udp`), and the address its datagrams are tagged with.
    primary: UdpSocket,
    advertised: SocketAddr,
    /// The IPv6 socket, and the address it offers.
    v6: Option<(UdpSocket, SocketAddr)>,
    /// Which socket is read first when both have something, taking turns so neither starves.
    v6_first: std::cell::Cell<bool>,
}

impl Udp {
    /// The session's socket alone, its datagrams tagged with `advertised`.
    pub(crate) fn new(primary: UdpSocket, advertised: SocketAddr) -> Self {
        guard_reads(&primary);
        Self { primary, advertised, v6: None, v6_first: std::cell::Cell::new(false) }
    }

    /// Adds the IPv6 socket `setting` asks for, marked with `dscp` as the first is. When there
    /// is no global address, or the port cannot be bound, the session goes on over IPv4 alone,
    /// with one line in the log.
    pub(crate) fn with_ipv6(mut self, setting: Ipv6Advertise, dscp: u8) -> io::Result<Self> {
        let bound = self.primary.local_addr()?;
        let Some(addr) = setting.resolve(bound, sovemu_protocol::net::primary_ipv6) else {
            if setting == Ipv6Advertise::Auto && bound.ip().is_unspecified() {
                tracing::info!("no global IPv6 address: sessions are offered over IPv4 only");
            }
            return Ok(self);
        };
        let IpAddr::V6(ip) = addr.ip() else { return Ok(self) };
        let socket = match bind_v6(ip, addr.port()) {
            Ok(socket) => socket,
            Err(e) => {
                tracing::info!(
                    address = %addr,
                    error = %e,
                    "IPv6 port not bound: sessions are offered over IPv4 only"
                );
                return Ok(self);
            }
        };
        crate::priority::mark(&socket, dscp);
        tracing::info!(address = %addr, bound = %socket.local_addr()?, "offered over IPv6 too");

        guard_reads(&socket);
        self.v6 = Some((socket, addr));
        Ok(self)
    }

    /// The IPv4 socket: STUN asks from it.
    pub(crate) fn primary(&self) -> &UdpSocket {
        &self.primary
    }

    /// The IPv4 address clients send to.
    pub(crate) fn advertised(&self) -> SocketAddr {
        self.advertised
    }

    /// The IPv6 address clients send to, if the session has one.
    pub(crate) fn advertised_v6(&self) -> Option<SocketAddr> {
        self.v6.as_ref().map(|(_, addr)| *addr)
    }

    /// Sends from the socket of `destination`'s family.
    pub(crate) fn send_to(&self, data: &[u8], destination: SocketAddr) -> io::Result<usize> {
        match &self.v6 {
            Some((v6, _)) if destination.is_ipv6() => v6.send_to(data, destination),
            _ => self.primary.send_to(data, destination),
        }
    }

    /// The next datagram within `wait` (none: only what has already arrived): its length in
    /// `buf`, its source, and the local address it was sent to. `None` when nothing came.
    pub(crate) fn recv(
        &self,
        buf: &mut [u8],
        wait: Duration,
    ) -> io::Result<Option<(usize, SocketAddr, SocketAddr)>> {
        let deadline = Instant::now() + wait;
        loop {
            let (v4_ready, v6_ready) = self.wait_readable(deadline)?;
            // Both have something: take turns.
            let v6_turn = v6_ready && (!v4_ready || self.v6_first.get());
            let received = match &self.v6 {
                Some((v6, local)) if v6_turn => {
                    self.v6_first.set(false);
                    read(v6, buf, *local)
                }
                _ if v4_ready => {
                    self.v6_first.set(true);
                    read(&self.primary, buf, self.advertised)
                }
                _ => return Ok(None), // the deadline passed
            };
            match received {
                Ok(datagram) => return Ok(Some(datagram)),
                // Readable but gone (a datagram dropped for its checksum): wait out the rest.
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
                Err(e) => return Err(e),
            }
        }
    }

    /// Waits until a socket is readable or `deadline` passes: which are readable, (IPv4, IPv6).
    fn wait_readable(&self, deadline: Instant) -> io::Result<(bool, bool)> {
        let readable = PollFlags::IN | PollFlags::ERR;
        loop {
            let mut fds = [
                PollFd::new(&self.primary, PollFlags::IN),
                PollFd::new(&self.primary, PollFlags::IN),
            ];
            let n = match &self.v6 {
                Some((v6, _)) => {
                    fds[1] = PollFd::new(v6, PollFlags::IN);
                    2
                }
                None => 1,
            };
            let left = deadline.saturating_duration_since(Instant::now());
            let timeout = Timespec {
                tv_sec: left.as_secs().try_into().unwrap_or(i64::MAX),
                tv_nsec: left.subsec_nanos().into(),
            };
            match rustix::event::poll(&mut fds[..n], Some(&timeout)) {
                Ok(_) => {
                    let ready = |fd: &PollFd<'_>| fd.revents().intersects(readable);
                    return Ok((ready(&fds[0]), n == 2 && ready(&fds[1])));
                }
                Err(rustix::io::Errno::INTR) => continue,
                Err(e) => return Err(e.into()),
            }
        }
    }
}

/// Bounds how long a read can block ([`READ_GUARD`]).
fn guard_reads(socket: &UdpSocket) {
    if let Err(e) = socket.set_read_timeout(Some(READ_GUARD)) {
        tracing::debug!(error = %e, "cannot bound the socket's reads");
    }
}

/// One datagram from `socket`, which was readable, tagged with `local`.
fn read(
    socket: &UdpSocket,
    buf: &mut [u8],
    local: SocketAddr,
) -> io::Result<(usize, SocketAddr, SocketAddr)> {
    let (n, source) = socket.recv_from(buf)?;
    Ok((n, source, local))
}

/// An IPv6 socket on `port`, only IPv6 (so it leaves IPv4 to the other socket): bound to `ip`
/// when it is one of the host's addresses, else to every IPv6 address (a given address another
/// box forwards here), its datagrams tagged with `ip` all the same.
fn bind_v6(ip: Ipv6Addr, port: u16) -> io::Result<UdpSocket> {
    let bind = |ip: Ipv6Addr| -> io::Result<UdpSocket> {
        let socket = socket2::Socket::new(
            socket2::Domain::IPV6,
            socket2::Type::DGRAM,
            Some(socket2::Protocol::UDP),
        )?;
        socket.set_only_v6(true)?;
        socket.bind(&SocketAddrV6::new(ip, port, 0, 0).into())?;
        Ok(socket.into())
    };
    match bind(ip) {
        Err(e) if e.kind() == ErrorKind::AddrNotAvailable => bind(Ipv6Addr::UNSPECIFIED),
        result => result,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// IPv6 loopback, where the machine has it (some containers turn IPv6 off altogether).
    fn loopback_v6() -> Option<UdpSocket> {
        UdpSocket::bind("[::1]:0").ok()
    }

    #[test]
    fn one_socket_tags_everything_with_the_advertised_address() {
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        let advertised: SocketAddr = "192.0.2.10:50000".parse().unwrap();
        let udp = Udp::new(socket.try_clone().unwrap(), advertised)
            .with_ipv6(Ipv6Advertise::Auto, 0)
            .unwrap();
        assert_eq!(udp.advertised_v6(), None, "bound to one IPv4 address: none added");

        let client = UdpSocket::bind("127.0.0.1:0").unwrap();
        client.send_to(b"hello", socket.local_addr().unwrap()).unwrap();
        let mut buf = [0u8; 64];
        let (n, source, local) = udp.recv(&mut buf, Duration::from_secs(5)).unwrap().unwrap();
        assert_eq!(
            (&buf[..n], source, local),
            (&b"hello"[..], client.local_addr().unwrap(), advertised)
        );
        assert!(udp.recv(&mut buf, Duration::from_millis(5)).unwrap().is_none(), "then nothing");
    }

    /// The two-socket path, which needs no IPv6 to test: what it waits on is two sockets.
    #[test]
    fn two_sockets_are_waited_on_together_and_take_turns() {
        let first = UdpSocket::bind("127.0.0.1:0").unwrap();
        let second = UdpSocket::bind("127.0.0.1:0").unwrap();
        let (to_first, to_second) = (first.local_addr().unwrap(), second.local_addr().unwrap());
        let advertised: SocketAddr = "192.0.2.10:50000".parse().unwrap();
        let tagged: SocketAddr = "[2001:db8::1]:50000".parse().unwrap();
        let mut udp = Udp::new(first, advertised);
        guard_reads(&second);
        udp.v6 = Some((second, tagged));

        let mut buf = [0u8; 64];
        assert!(udp.recv(&mut buf, Duration::ZERO).unwrap().is_none(), "nothing yet");
        let client = UdpSocket::bind("127.0.0.1:0").unwrap();
        // A late datagram on the second socket ends a wait on both.
        let late = client.try_clone().unwrap();
        let sender = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            late.send_to(b"late", to_second).unwrap();
        });
        let (n, _, local) = udp.recv(&mut buf, Duration::from_secs(5)).unwrap().unwrap();
        assert_eq!((&buf[..n], local), (&b"late"[..], tagged));
        sender.join().unwrap();

        // Both busy: neither starves, and each datagram keeps its socket's tag.
        for i in 0..3u8 {
            client.send_to(&[b'4', i], to_first).unwrap();
            client.send_to(&[b'6', i], to_second).unwrap();
        }
        let mut heard = Vec::new();
        while heard.len() < 6 {
            let (n, _, local) = udp.recv(&mut buf, Duration::from_secs(5)).unwrap().unwrap();
            heard.push((buf[..n].to_vec(), local));
        }
        let order: Vec<u8> = heard.iter().map(|(d, _)| d[0]).collect();
        assert!(order.windows(2).all(|w| w[0] != w[1]), "turns: {order:?}");
        assert!(
            heard.iter().all(|(d, local)| *local == if d[0] == b'4' { advertised } else { tagged })
        );
        assert!(udp.recv(&mut buf, Duration::from_millis(5)).unwrap().is_none(), "then nothing");
    }

    #[test]
    fn off_offers_no_ipv6() {
        let socket = UdpSocket::bind("0.0.0.0:0").unwrap();
        let advertised = socket.local_addr().unwrap();
        let udp = Udp::new(socket, advertised).with_ipv6(Ipv6Advertise::Off, 0).unwrap();
        assert_eq!(udp.advertised_v6(), None);
    }

    #[test]
    fn both_families_arrive_tagged_and_replies_leave_by_their_own() {
        let Some(client6) = loopback_v6() else {
            eprintln!("no IPv6 loopback here: skipped");
            return;
        };
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = socket.local_addr().unwrap().port();
        let advertised: SocketAddr = format!("192.0.2.10:{port}").parse().unwrap();
        let udp = Udp::new(socket, advertised)
            .with_ipv6(Ipv6Advertise::Address(Ipv6Addr::LOCALHOST), 34)
            .unwrap();
        let v6: SocketAddr = format!("[::1]:{port}").parse().unwrap();
        assert_eq!(udp.advertised_v6(), Some(v6), "the same port, on the given address");

        let client4 = UdpSocket::bind("127.0.0.1:0").unwrap();
        client4.send_to(b"four", ("127.0.0.1", port)).unwrap();
        client6.send_to(b"six", v6).unwrap();
        let mut buf = [0u8; 64];
        let mut heard = Vec::new();
        while heard.len() < 2 {
            let (n, source, local) = udp.recv(&mut buf, Duration::from_secs(5)).unwrap().unwrap();
            heard.push((buf[..n].to_vec(), source, local));
        }
        heard.sort();
        assert_eq!(
            heard,
            vec![
                (b"four".to_vec(), client4.local_addr().unwrap(), advertised),
                (b"six".to_vec(), client6.local_addr().unwrap(), v6),
            ]
        );

        // Each reply goes out of the socket of its destination's family.
        udp.send_to(b"to six", client6.local_addr().unwrap()).unwrap();
        udp.send_to(b"to four", client4.local_addr().unwrap()).unwrap();
        client6.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        client4.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let (n, from) = client6.recv_from(&mut buf).unwrap();
        assert_eq!((&buf[..n], from), (&b"to six"[..], v6));
        let (n, from) = client4.recv_from(&mut buf).unwrap();
        assert_eq!((&buf[..n], from.port()), (&b"to four"[..], port));
    }

    #[test]
    fn a_port_taken_on_ipv6_leaves_the_session_on_ipv4() {
        let Some(taken) = loopback_v6() else {
            eprintln!("no IPv6 loopback here: skipped");
            return;
        };
        let port = taken.local_addr().unwrap().port();
        let Ok(socket) = UdpSocket::bind(("127.0.0.1", port)) else {
            eprintln!("the IPv4 port is taken too: skipped");
            return;
        };
        let advertised = socket.local_addr().unwrap();
        let udp = Udp::new(socket, advertised)
            .with_ipv6(Ipv6Advertise::Address(Ipv6Addr::LOCALHOST), 0)
            .unwrap();
        assert_eq!(udp.advertised_v6(), None);
    }
}
