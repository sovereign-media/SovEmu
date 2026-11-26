//! The relay's sockets, driving `turn-server-proto`'s two servers (one for clients over UDP, one
//! over TCP) from a single task.
//!
//! Every socket has a reader task that only forwards what it reads to that task, as an [`Event`],
//! so the servers' state has one owner and needs no lock. Writes happen on the task itself,
//! except to TCP clients, which each have a writer task with a short queue (a slow client loses
//! packets rather than holding up everyone else's).

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::SystemTime;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tokio_util::task::AbortOnDropHandle;
use turn_server_proto::api::{
    SocketAllocateError, TcpConnectError, TurnServerApi, TurnServerPollRet,
};
use turn_server_proto::server::TurnServer;
use turn_server_proto::stun::Instant;
use turn_server_proto::stun::agent::Transmit;
use turn_server_proto::types::stun::attribute::Username;
use turn_server_proto::types::stun::message::Message;
use turn_server_proto::types::tcp::{IncomingTcp, StoredTcp, TurnTcpBuffer};
use turn_server_proto::types::{AddressFamily, TransportType};

use crate::{Config, Error, Peers, REALM, credentials};

/// The most a datagram can be. TURN messages carrying WebRTC media are well under 1500 bytes.
const DATAGRAM: usize = 65_536;
/// Packets waiting for a TCP client's writer before newer ones are dropped.
const TCP_QUEUE: usize = 256;
/// Packets waiting for the relay's task before readers wait (and the OS drops UDP).
const EVENTS: usize = 1024;

/// What the relay is doing, for the server's metrics.
#[derive(Debug, Default)]
pub struct Stats {
    allocations: AtomicUsize,
    bytes: AtomicU64,
}

impl Stats {
    /// Relayed allocations open now (a relayed player holds one or two).
    pub fn allocations(&self) -> usize {
        self.allocations.load(Ordering::Relaxed)
    }

    /// Bytes relayed between clients and game sessions, both ways, since the relay started.
    pub fn bytes(&self) -> u64 {
        self.bytes.load(Ordering::Relaxed)
    }
}

/// The running relay. Dropping it stops it, and closes every allocation.
#[derive(Debug)]
pub struct Relay {
    udp: SocketAddr,
    tcp: SocketAddr,
    stats: Arc<Stats>,
    cancel: CancellationToken,
    _task: AbortOnDropHandle<()>,
}

impl Relay {
    /// Opens the relay's port over UDP and TCP and starts relaying.
    pub async fn start(config: Config) -> Result<Self, Error> {
        let udp = UdpSocket::bind(config.listen).await.map_err(|source| Error::Bind {
            addr: config.listen,
            transport: "udp",
            source,
        })?;
        let udp_addr = udp.local_addr().map_err(|source| Error::Bind {
            addr: config.listen,
            transport: "udp",
            source,
        })?;
        // The same port over TCP (when the configured one was 0, the one UDP was given).
        let tcp_at = SocketAddr::new(config.listen.ip(), udp_addr.port());
        let tcp = TcpListener::bind(tcp_at).await.map_err(|source| Error::Bind {
            addr: tcp_at,
            transport: "tcp",
            source,
        })?;
        let tcp_addr = tcp.local_addr().map_err(|source| Error::Bind {
            addr: tcp_at,
            transport: "tcp",
            source,
        })?;

        let stats = Arc::new(Stats::default());
        let cancel = CancellationToken::new();
        let (events, rx) = mpsc::channel(EVENTS);
        let udp = Arc::new(udp);
        tokio::spawn(read_udp(udp.clone(), None, events.clone(), cancel.clone()));
        tokio::spawn(accept_tcp(tcp, config.max_allocations * 2, events.clone(), cancel.clone()));
        let state = State {
            udp_server: TurnServer::new(TransportType::Udp, udp_addr, REALM.to_owned()),
            tcp_server: TurnServer::new(TransportType::Tcp, tcp_addr, REALM.to_owned()),
            udp_addr,
            tcp_addr,
            udp,
            peers: config.peers,
            secret: config.secret,
            max_allocations: config.max_allocations,
            known: HashSet::new(),
            tcp_clients: HashMap::new(),
            allocations: HashMap::new(),
            events,
            stats: stats.clone(),
            cancel: cancel.clone(),
            base: std::time::Instant::now(),
        };
        let task = AbortOnDropHandle::new(tokio::spawn(state.run(rx)));
        tracing::info!(udp = %udp_addr, tcp = %tcp_addr, "relay listening");
        Ok(Self { udp: udp_addr, tcp: tcp_addr, stats, cancel, _task: task })
    }

    /// The address it listens on over UDP.
    pub fn udp_addr(&self) -> SocketAddr {
        self.udp
    }

    /// The address it listens on over TCP.
    pub fn tcp_addr(&self) -> SocketAddr {
        self.tcp
    }

    pub fn stats(&self) -> Arc<Stats> {
        self.stats.clone()
    }
}

impl Drop for Relay {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

#[derive(Debug)]
enum Event {
    /// A datagram on the relay's own port, from a client.
    Udp { from: SocketAddr, data: Vec<u8> },
    /// A client connected over TCP.
    TcpOpen { from: SocketAddr, writer: mpsc::Sender<Vec<u8>>, cancel: CancellationToken },
    /// Bytes from a TCP client.
    Tcp { from: SocketAddr, data: Vec<u8> },
    /// A TCP client went away.
    TcpClosed { from: SocketAddr },
    /// A datagram from a game session to an allocation, `from` named as the client knows it.
    Peer { relayed: SocketAddr, from: SocketAddr, data: Vec<u8> },
}

#[derive(Debug)]
struct TcpClient {
    writer: mpsc::Sender<Vec<u8>>,
    cancel: CancellationToken,
    /// A copy of the server's framing, only to read the usernames in what the client sends.
    framing: TurnTcpBuffer,
}

#[derive(Debug)]
struct Allocation {
    socket: Arc<UdpSocket>,
    /// The server (client transport) the allocation belongs to.
    transport: TransportType,
    cancel: CancellationToken,
}

#[derive(Debug)]
struct State {
    udp_server: TurnServer,
    tcp_server: TurnServer,
    udp_addr: SocketAddr,
    tcp_addr: SocketAddr,
    udp: Arc<UdpSocket>,
    peers: Peers,
    secret: Vec<u8>,
    max_allocations: usize,
    /// Usernames already given to the servers (by whether it is the TCP one): their passwords
    /// are added the first time a client uses them.
    known: HashSet<(bool, String)>,
    tcp_clients: HashMap<SocketAddr, TcpClient>,
    /// By relayed address, as clients are told it (the advertised address and the socket's port).
    allocations: HashMap<SocketAddr, Allocation>,
    events: mpsc::Sender<Event>,
    stats: Arc<Stats>,
    cancel: CancellationToken,
    base: std::time::Instant,
}

impl State {
    fn now(&self) -> Instant {
        Instant::from_std(self.base)
    }

    async fn run(mut self, mut rx: mpsc::Receiver<Event>) {
        loop {
            let wake = self.drive().await;
            let wake = tokio::time::Instant::from_std(wake.to_std(self.base));
            tokio::select! {
                () = self.cancel.cancelled() => break,
                event = rx.recv() => match event {
                    Some(event) => self.handle(event).await,
                    None => break,
                },
                () = tokio::time::sleep_until(wake) => {}
            }
        }
        for (_, a) in self.allocations.drain() {
            a.cancel.cancel();
        }
        for (_, c) in self.tcp_clients.drain() {
            c.cancel.cancel();
        }
        tracing::info!("relay stopped");
    }

    fn server(&mut self, transport: TransportType) -> &mut TurnServer {
        match transport {
            TransportType::Udp => &mut self.udp_server,
            TransportType::Tcp => &mut self.tcp_server,
        }
    }

    fn listen_addr(&self, transport: TransportType) -> SocketAddr {
        match transport {
            TransportType::Udp => self.udp_addr,
            TransportType::Tcp => self.tcp_addr,
        }
    }

    async fn handle(&mut self, event: Event) {
        let now = self.now();
        match event {
            Event::Udp { from, data } => {
                // STUN messages start with two zero bits; channel data (0x40-0x7f) carries none.
                if data.first().is_some_and(|b| *b < 0x40)
                    && !self.authorize(TransportType::Udp, &data)
                {
                    tracing::debug!(client = %from, "relay: expired or malformed credentials");
                    return;
                }
                let to = self.udp_addr;
                let out = self
                    .udp_server
                    .recv(Transmit::new(&data[..], TransportType::Udp, from, to), now);
                if let Some(out) = out.map(|t| t.build()) {
                    self.send(out).await;
                }
            }
            Event::TcpOpen { from, writer, cancel } => {
                self.tcp_clients
                    .insert(from, TcpClient { writer, cancel, framing: TurnTcpBuffer::new() });
            }
            Event::Tcp { from, data } => {
                if !self.authorize_tcp(from, &data) {
                    tracing::debug!(client = %from, "relay: expired or malformed credentials, closing");
                    self.close_tcp(from, now).await;
                    return;
                }
                let to = self.tcp_addr;
                let out = self
                    .tcp_server
                    .recv(Transmit::new(&data[..], TransportType::Tcp, from, to), now);
                if let Some(out) = out.map(|t| t.build()) {
                    self.send(out).await;
                }
            }
            Event::TcpClosed { from } => self.close_tcp(from, now).await,
            Event::Peer { relayed, from, data } => {
                let Some(transport) = self.allocations.get(&relayed).map(|a| a.transport) else {
                    return;
                };
                self.stats.bytes.fetch_add(data.len() as u64, Ordering::Relaxed);
                let out = self
                    .server(transport)
                    .recv(Transmit::new(&data[..], TransportType::Udp, from, relayed), now);
                if let Some(out) = out.map(|t| t.build()) {
                    self.send(out).await;
                }
            }
        }
    }

    /// Ends a TCP client's connection, and tells the TCP server it has gone.
    async fn close_tcp(&mut self, client: SocketAddr, now: Instant) {
        if let Some(c) = self.tcp_clients.remove(&client) {
            c.cancel.cancel();
            let to = self.tcp_addr;
            let out =
                self.tcp_server.recv(Transmit::new(&[][..], TransportType::Tcp, client, to), now);
            if let Some(out) = out.map(|t| t.build()) {
                self.send(out).await;
            }
        }
    }

    /// Whether a client's STUN message may go on to the server: one with no username (the first
    /// allocate, a binding request) does, and so does one naming credentials this server minted
    /// that have not expired, whose password the server is given here the first time.
    ///
    /// A client signing in with RFC 8489's anonymous USERHASH instead of a USERNAME is refused
    /// by the server (no password is known for it until its username has been seen): browsers
    /// and the apps' WebRTC send a USERNAME.
    fn authorize(&mut self, transport: TransportType, data: &[u8]) -> bool {
        let Ok(msg) = Message::from_bytes(data) else {
            // Not ours to judge: the server answers it (or not) as the protocol says.
            return true;
        };
        let Ok(username) = msg.attribute::<Username>() else {
            return true;
        };
        let username = username.username();
        if !credentials::current(username, SystemTime::now()) {
            return false;
        }
        if self.known.insert((transport == TransportType::Tcp, username.to_owned())) {
            let password = credentials::password(&self.secret, username);
            self.server(transport).add_user(username.to_owned(), password);
        }
        true
    }

    /// [`authorize`](Self::authorize) for every STUN message complete in a TCP client's stream
    /// once `data` is added to it.
    fn authorize_tcp(&mut self, from: SocketAddr, data: &[u8]) -> bool {
        let Some(client) = self.tcp_clients.get_mut(&from) else {
            return false;
        };
        let mut messages = Vec::new();
        let first = client.framing.incoming_tcp(Transmit::new(
            data,
            TransportType::Tcp,
            from,
            self.tcp_addr,
        ));
        match first {
            Err(_) => return false,
            Ok(Some(IncomingTcp::CompleteMessage(t, range))) => {
                messages.push(t.data[range].to_vec())
            }
            Ok(Some(IncomingTcp::StoredMessage(m, _))) => messages.push(m),
            Ok(_) => {}
        }
        loop {
            match client.framing.poll_recv() {
                Err(_) => return false,
                Ok(Some(StoredTcp::Message(m))) => messages.push(m),
                Ok(Some(StoredTcp::Channel(_))) => {}
                Ok(None) => break,
            }
        }
        messages.iter().all(|m| self.authorize(TransportType::Tcp, m))
    }

    /// Runs both servers until neither has anything to do now: opens and closes what they ask
    /// for and sends what they have queued. Returns when to call it again.
    async fn drive(&mut self) -> Instant {
        let mut wake = self.now() + std::time::Duration::from_secs(3600);
        for transport in [TransportType::Udp, TransportType::Tcp] {
            // After a drain that sent nothing, one more look: a TCP client's buffered message,
            // handled while draining, can leave work (an allocation) without sending anything.
            let mut looked_again = false;
            loop {
                let now = self.now();
                match self.server(transport).poll(now) {
                    TurnServerPollRet::WaitUntil(at) => {
                        wake = wake.min(at);
                        let mut sent = false;
                        while let Some(t) = self.server(transport).poll_transmit(now) {
                            self.send(t).await;
                            sent = true;
                        }
                        if sent {
                            looked_again = false;
                        } else if looked_again {
                            break;
                        } else {
                            looked_again = true;
                        }
                    }
                    TurnServerPollRet::AllocateSocket {
                        transport,
                        listen_addr,
                        client_addr,
                        allocation_transport,
                        family,
                    } => {
                        let result = self
                            .allocate(transport, client_addr, allocation_transport, family)
                            .await;
                        self.server(transport).allocated_socket(
                            transport,
                            listen_addr,
                            client_addr,
                            allocation_transport,
                            family,
                            result,
                            now,
                        );
                    }
                    TurnServerPollRet::SocketClose { listen_addr, .. } => {
                        if let Some(a) = self.allocations.remove(&listen_addr) {
                            a.cancel.cancel();
                            self.stats.allocations.store(self.allocations.len(), Ordering::Relaxed);
                            tracing::info!(relayed = %listen_addr, "relay allocation closed");
                        }
                    }
                    TurnServerPollRet::TcpConnect {
                        relayed_addr,
                        peer_addr,
                        listen_addr,
                        client_addr,
                    } => {
                        // TURN-TCP is refused at allocation, so no allocation can ask this.
                        self.server(transport).tcp_connected(
                            relayed_addr,
                            peer_addr,
                            listen_addr,
                            client_addr,
                            Err(TcpConnectError::Forbidden),
                            now,
                        );
                    }
                    TurnServerPollRet::TcpClose { local_addr, remote_addr } => {
                        if local_addr == self.tcp_addr
                            && let Some(c) = self.tcp_clients.remove(&remote_addr)
                        {
                            c.cancel.cancel();
                        }
                    }
                }
            }
        }
        wake
    }

    /// Opens a relay socket for a client's allocation, beside the game sessions.
    async fn allocate(
        &mut self,
        transport: TransportType,
        client: SocketAddr,
        allocation_transport: TransportType,
        family: AddressFamily,
    ) -> Result<SocketAddr, SocketAllocateError> {
        let advertised = self.peers.advertised;
        let ours = if advertised.is_ipv4() { AddressFamily::IPV4 } else { AddressFamily::IPV6 };
        if family != ours {
            return Err(SocketAllocateError::AddressFamilyNotSupported);
        }
        if allocation_transport != TransportType::Udp
            || self.allocations.len() >= self.max_allocations
        {
            tracing::warn!(
                client = %client,
                open = self.allocations.len(),
                "relay allocation refused (TCP, or at SovEmu__Relay__MaxAllocations)"
            );
            return Err(SocketAllocateError::InsufficientCapacity);
        }
        let socket =
            UdpSocket::bind(SocketAddr::new(self.peers.local(), 0)).await.map_err(|e| {
                tracing::warn!(error = %e, "opening a relay socket");
                SocketAllocateError::InsufficientCapacity
            })?;
        let port =
            socket.local_addr().map_err(|_| SocketAllocateError::InsufficientCapacity)?.port();
        let relayed = SocketAddr::new(advertised, port);
        let socket = Arc::new(socket);
        let cancel = self.cancel.child_token();
        tokio::spawn(read_udp(
            socket.clone(),
            Some((relayed, self.peers)),
            self.events.clone(),
            cancel.clone(),
        ));
        self.allocations.insert(relayed, Allocation { socket, transport, cancel });
        self.stats.allocations.store(self.allocations.len(), Ordering::Relaxed);
        tracing::info!(client = %client, transport = %transport, relayed = %relayed, "relay allocation opened");
        Ok(relayed)
    }

    /// Sends what a server produced: to a client (over UDP or its TCP connection), or from an
    /// allocation to a game session, and only to one.
    async fn send(&mut self, t: Transmit<Vec<u8>>) {
        if t.from == self.listen_addr(t.transport) {
            match t.transport {
                TransportType::Udp => {
                    if let Err(e) = self.udp.send_to(&t.data, t.to).await {
                        tracing::debug!(error = %e, client = %t.to, "relay: sending to a client");
                    }
                }
                TransportType::Tcp => {
                    if let Some(c) = self.tcp_clients.get(&t.to) {
                        // Full: the client is not keeping up; this packet is lost, as on UDP.
                        let _ = c.writer.try_send(t.data);
                    }
                }
            }
            return;
        }
        let Some(a) = self.allocations.get(&t.from) else {
            return;
        };
        let Some(to) = self.peers.route(t.to) else {
            tracing::debug!(peer = %t.to, "relay: refused, not a game session");
            return;
        };
        self.stats.bytes.fetch_add(t.data.len() as u64, Ordering::Relaxed);
        if let Err(e) = a.socket.send_to(&t.data, to).await {
            tracing::debug!(error = %e, peer = %to, "relay: sending to a game session");
        }
    }
}

/// Reads datagrams from a socket until cancelled: the relay's own (`relayed` none, from clients),
/// or an allocation's (from game sessions, anything else dropped).
async fn read_udp(
    socket: Arc<UdpSocket>,
    relayed: Option<(SocketAddr, Peers)>,
    events: mpsc::Sender<Event>,
    cancel: CancellationToken,
) {
    let mut buf = vec![0; DATAGRAM];
    loop {
        let (n, from) = tokio::select! {
            () = cancel.cancelled() => return,
            r = socket.recv_from(&mut buf) => match r {
                Ok(r) => r,
                // An ICMP error from an earlier send (a client gone): not this socket's end.
                Err(e) if e.kind() == std::io::ErrorKind::ConnectionReset => continue,
                Err(e) => {
                    tracing::warn!(error = %e, "relay: reading a socket");
                    return;
                }
            },
        };
        let data = buf[..n].to_vec();
        let event = match relayed {
            None => Event::Udp { from, data },
            Some((relayed, peers)) => match peers.source(from) {
                Some(from) => Event::Peer { relayed, from, data },
                None => continue,
            },
        };
        if events.send(event).await.is_err() {
            return;
        }
    }
}

/// Accepts TCP clients until cancelled, up to `limit` at once.
async fn accept_tcp(
    listener: TcpListener,
    limit: usize,
    events: mpsc::Sender<Event>,
    cancel: CancellationToken,
) {
    let open = Arc::new(AtomicUsize::new(0));
    loop {
        let (stream, from) = tokio::select! {
            () = cancel.cancelled() => return,
            r = listener.accept() => match r {
                Ok(r) => r,
                Err(e) => {
                    tracing::debug!(error = %e, "relay: accepting a TCP client");
                    continue;
                }
            },
        };
        if open.load(Ordering::Relaxed) >= limit {
            tracing::warn!(client = %from, "relay: too many TCP clients, refused");
            continue;
        }
        let _ = stream.set_nodelay(true);
        let (writer, rx) = mpsc::channel(TCP_QUEUE);
        let conn = cancel.child_token();
        if events.send(Event::TcpOpen { from, writer, cancel: conn.clone() }).await.is_err() {
            return;
        }
        open.fetch_add(1, Ordering::Relaxed);
        let (events, open) = (events.clone(), open.clone());
        tokio::spawn(async move {
            tcp_client(stream, from, rx, events.clone(), conn).await;
            open.fetch_sub(1, Ordering::Relaxed);
            let _ = events.send(Event::TcpClosed { from }).await;
        });
    }
}

/// One TCP client: forwards what it sends, writes what the relay queues, until either side ends.
async fn tcp_client(
    stream: TcpStream,
    from: SocketAddr,
    mut rx: mpsc::Receiver<Vec<u8>>,
    events: mpsc::Sender<Event>,
    cancel: CancellationToken,
) {
    let (mut read, mut write) = stream.into_split();
    let mut buf = vec![0; DATAGRAM];
    loop {
        tokio::select! {
            () = cancel.cancelled() => break,
            n = read.read(&mut buf) => match n {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if events.send(Event::Tcp { from, data: buf[..n].to_vec() }).await.is_err() {
                        break;
                    }
                }
            },
            out = rx.recv() => match out {
                Some(data) => {
                    if write.write_all(&data).await.is_err() {
                        break;
                    }
                }
                None => break,
            },
        }
    }
    let _ = write.shutdown().await;
}
