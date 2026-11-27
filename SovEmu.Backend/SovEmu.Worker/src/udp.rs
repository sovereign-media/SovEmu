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
//! With IPv4 alone the socket is read on the stream's own thread, as it always was. With both,
//! each is read on a thread of its own and the datagrams meet in one queue, since std has no way
//! to wait on two sockets at once.
//!
//! [`advertised_addr`]: sovemu_protocol::net::advertised_addr

use std::io::{self, ErrorKind};
use std::net::{IpAddr, Ipv6Addr, SocketAddr, SocketAddrV6, UdpSocket};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::Duration;

use sovemu_protocol::net::Ipv6Advertise;

/// How often a reading thread looks up to see whether the session has ended.
const READ_SLICE: Duration = Duration::from_millis(200);

/// One received datagram: its bytes, who sent it, and the local address it was sent to.
type Datagram = (Vec<u8>, SocketAddr, SocketAddr);

/// A session's sockets.
#[derive(Debug)]
pub(crate) struct Udp {
    /// The IPv4 socket (`--udp`), and the address its datagrams are tagged with.
    primary: UdpSocket,
    advertised: SocketAddr,
    /// The IPv6 socket, and the address it offers.
    v6: Option<(UdpSocket, SocketAddr)>,
    /// With two sockets, what their reading threads have received.
    incoming: Option<Receiver<Datagram>>,
    /// Tells the reading threads to stop.
    closed: Arc<AtomicBool>,
}

impl Udp {
    /// The session's socket alone, its datagrams tagged with `advertised`.
    pub(crate) fn new(primary: UdpSocket, advertised: SocketAddr) -> Self {
        Self { primary, advertised, v6: None, incoming: None, closed: Arc::default() }
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

        let (tx, rx) = mpsc::channel();
        read_on_thread("udp-v4", self.primary.try_clone()?, self.advertised, &tx, &self.closed)?;
        read_on_thread("udp-v6", socket.try_clone()?, addr, &tx, &self.closed)?;
        self.v6 = Some((socket, addr));
        self.incoming = Some(rx);
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

    /// The next datagram within `wait` (at least a millisecond): its length in `buf`, its
    /// source, and the local address it was sent to. `None` when nothing came.
    pub(crate) fn recv(
        &self,
        buf: &mut [u8],
        wait: Duration,
    ) -> io::Result<Option<(usize, SocketAddr, SocketAddr)>> {
        let wait = wait.max(Duration::from_millis(1));
        let Some(incoming) = &self.incoming else {
            self.primary.set_read_timeout(Some(wait))?;
            return match self.primary.recv_from(buf) {
                Ok((n, source)) => Ok(Some((n, source, self.advertised))),
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                    Ok(None)
                }
                Err(e) => Err(e),
            };
        };
        match incoming.recv_timeout(wait) {
            Ok((data, source, local)) => {
                let n = data.len().min(buf.len());
                buf[..n].copy_from_slice(&data[..n]);
                Ok(Some((n, source, local)))
            }
            Err(RecvTimeoutError::Timeout) => Ok(None),
            Err(RecvTimeoutError::Disconnected) => {
                Err(io::Error::other("the sockets' reading threads have stopped"))
            }
        }
    }
}

impl Drop for Udp {
    fn drop(&mut self) {
        self.closed.store(true, Ordering::Relaxed);
    }
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

/// Reads `socket` on a thread of its own, tagging its datagrams with `local`, until the session
/// ends or nobody is listening.
fn read_on_thread(
    name: &str,
    socket: UdpSocket,
    local: SocketAddr,
    tx: &Sender<Datagram>,
    closed: &Arc<AtomicBool>,
) -> io::Result<()> {
    // A read timeout is the socket's, shared by every handle to it: only this thread reads.
    socket.set_read_timeout(Some(READ_SLICE))?;
    let (tx, closed) = (tx.clone(), closed.clone());
    let span = tracing::Span::current();
    thread::Builder::new().name(name.into()).spawn(move || {
        let _span = span.enter();
        let mut buf = vec![0u8; 2048];
        while !closed.load(Ordering::Relaxed) {
            match socket.recv_from(&mut buf) {
                Ok((n, source)) => {
                    if tx.send((buf[..n].to_vec(), source, local)).is_err() {
                        break;
                    }
                }
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
                // An ICMP error from an earlier send to a client that went away.
                Err(e) => tracing::debug!(error = %e, "UDP receive failed"),
            }
        }
    })?;
    Ok(())
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
