//! The relay against a real TURN client (`turn-client-proto`), relaying to a stand-in game
//! session on loopback: what a browser on a far network does through it.

use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sovemu_relay::{Config, Peers, Relay, mint};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, UdpSocket};
use turn_client_proto::api::{TurnConfig, TurnEvent, TurnPollRet, TurnRecvRet};
use turn_client_proto::prelude::*;
use turn_client_proto::stun::Instant;
use turn_client_proto::stun::agent::Transmit;
use turn_client_proto::stun::auth::Feature;
use turn_client_proto::tcp::TurnClientTcp;
use turn_client_proto::types::{TransportType, TurnCredentials};
use turn_client_proto::udp::TurnClientUdp;

/// The address clients know the sessions at. Never sent to: the relay reaches them on loopback.
const ADVERTISED: &str = "192.0.2.10";
const SECRET: &[u8] = b"test relay key";
/// Long enough for a loaded CI machine; every wait ends as soon as what it waits for happens.
const PATIENCE: Duration = Duration::from_secs(20);

fn advertised(port: u16) -> SocketAddr {
    SocketAddr::new(ADVERTISED.parse().unwrap(), port)
}

/// A stand-in game session on loopback, and a relay that may reach it (and only it).
async fn setup(max_allocations: usize) -> (Relay, UdpSocket) {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_test_writer()
        .try_init();
    let session = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let port = session.local_addr().unwrap().port();
    let relay = Relay::start(Config {
        listen: "127.0.0.1:0".parse().unwrap(),
        peers: Peers {
            advertised: ADVERTISED.parse().unwrap(),
            bind: "0.0.0.0".parse().unwrap(),
            ports: (port, port),
        },
        secret: SECRET.to_vec(),
        max_allocations,
    })
    .await
    .unwrap();
    (relay, session)
}

fn credentials() -> TurnCredentials {
    let c = mint(SECRET, "7", Duration::from_secs(3600), SystemTime::now());
    TurnCredentials::new(&c.username, &c.password)
}

fn config(credentials: TurnCredentials) -> TurnConfig {
    let mut config = TurnConfig::new(credentials);
    // As browsers and the apps' WebRTC do: a USERNAME, not RFC 8489's anonymous USERHASH (which
    // the relay cannot work a password out from).
    config.set_anonymous_username(Feature::Disabled);
    // A refused request fails in about a second, not the RFC's 39.5.
    config.set_request_retransmits(
        Duration::from_millis(100),
        Duration::from_millis(200),
        3,
        Duration::from_millis(500),
    );
    config
}

#[derive(Debug)]
enum Socket {
    Udp(UdpSocket),
    Tcp(TcpStream),
}

#[derive(Debug)]
enum Turn {
    Udp(TurnClientUdp),
    Tcp(TurnClientTcp),
}

macro_rules! turn {
    ($self:expr, $c:ident => $e:expr) => {
        match &mut $self.turn {
            Turn::Udp($c) => $e,
            Turn::Tcp($c) => $e,
        }
    };
}

/// A TURN client, driven over a real socket.
#[derive(Debug)]
struct Client {
    turn: Turn,
    socket: Socket,
    local: SocketAddr,
    server: SocketAddr,
    base: std::time::Instant,
    /// What peers sent, as (peer, data).
    received: Vec<(SocketAddr, Vec<u8>)>,
    /// Whether [`until`](Self::until) returns as soon as a peer's data arrives.
    want_data: bool,
}

impl Client {
    async fn udp(relay: &Relay, credentials: TurnCredentials) -> Self {
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let local = socket.local_addr().unwrap();
        let server = relay.udp_addr();
        let turn = Turn::Udp(TurnClientUdp::allocate(local, server, config(credentials)));
        Self::new(turn, Socket::Udp(socket), local, server)
    }

    async fn tcp(relay: &Relay, credentials: TurnCredentials) -> Self {
        let server = relay.tcp_addr();
        let socket = TcpStream::connect(server).await.unwrap();
        let local = socket.local_addr().unwrap();
        let turn = Turn::Tcp(TurnClientTcp::allocate(local, server, config(credentials)));
        Self::new(turn, Socket::Tcp(socket), local, server)
    }

    fn new(turn: Turn, socket: Socket, local: SocketAddr, server: SocketAddr) -> Self {
        Self {
            turn,
            socket,
            local,
            server,
            base: std::time::Instant::now(),
            received: vec![],
            want_data: false,
        }
    }

    fn now(&self) -> Instant {
        Instant::from_std(self.base)
    }

    fn transport(&self) -> TransportType {
        match self.socket {
            Socket::Udp(_) => TransportType::Udp,
            Socket::Tcp(_) => TransportType::Tcp,
        }
    }

    async fn write(&mut self, data: &[u8]) {
        match &mut self.socket {
            Socket::Udp(s) => {
                s.send_to(data, self.server).await.unwrap();
            }
            Socket::Tcp(s) => s.write_all(data).await.unwrap(),
        }
    }

    /// Runs the client until `done` holds for an event, or it closes, or `PATIENCE` passes.
    async fn until(&mut self, done: impl Fn(&TurnEvent) -> bool) -> Option<TurnEvent> {
        let give_up = tokio::time::Instant::now() + PATIENCE;
        let mut buf = vec![0; 65_536];
        loop {
            let now = self.now();
            let wait = match turn!(self, t => t.poll(now)) {
                TurnPollRet::WaitUntil(at) => at,
                TurnPollRet::Closed => return None,
                other => panic!("unexpected {other:?}"),
            };
            while let Some(t) = turn!(self, c => c.poll_transmit(now)) {
                let data = t.data.to_vec();
                self.write(&data).await;
            }
            while let Some(event) = turn!(self, t => t.poll_event()) {
                if done(&event) {
                    return Some(event);
                }
            }
            let wake = tokio::time::Instant::from_std(wait.to_std(self.base));
            // Nothing to do for longer than we would wait: a refused client has given up.
            if wake >= give_up && !self.want_data {
                return None;
            }
            let wake = wake.min(give_up);
            if tokio::time::Instant::now() >= give_up {
                return None;
            }
            let n = tokio::select! {
                () = tokio::time::sleep_until(wake) => continue,
                n = async {
                    match &mut self.socket {
                        Socket::Udp(s) => s.recv(&mut buf).await.unwrap(),
                        Socket::Tcp(s) => s.read(&mut buf).await.unwrap(),
                    }
                } => n,
            };
            let (transport, from, to, now) =
                (self.transport(), self.server, self.local, self.now());
            let ret = turn!(self, t => t.recv(Transmit::new(&buf[..n], transport, from, to), now));
            if let TurnRecvRet::PeerData(d) = ret {
                self.received.push((d.peer, d.data().to_vec()));
            }
            while let Some(d) = turn!(self, t => t.poll_recv(now)) {
                self.received.push((d.peer, d.data().to_vec()));
            }
            if self.want_data && !self.received.is_empty() {
                return None;
            }
        }
    }

    async fn allocate(&mut self) -> SocketAddr {
        match self
            .until(|e| {
                matches!(e, TurnEvent::AllocationCreated(..) | TurnEvent::AllocationCreateFailed(_))
            })
            .await
        {
            Some(TurnEvent::AllocationCreated(_, relayed)) => relayed,
            other => panic!("no allocation: {other:?}"),
        }
    }

    async fn permit(&mut self, ip: IpAddr) {
        let now = self.now();
        turn!(self, t => t.create_permission(TransportType::Udp, ip, now)).unwrap();
        let got = self
            .until(|e| {
                matches!(
                    e,
                    TurnEvent::PermissionCreated(..) | TurnEvent::PermissionCreateFailed(..)
                )
            })
            .await;
        assert!(matches!(got, Some(TurnEvent::PermissionCreated(..))), "{got:?}");
    }

    async fn send(&mut self, to: SocketAddr, data: &[u8]) {
        let now = self.now();
        let out = turn!(self, t => t.send_to(TransportType::Udp, to, data, now)).unwrap();
        if let Some(out) = out {
            let out = out.build();
            self.write(&out.data).await;
        }
    }

    /// Runs the client until a peer's data has arrived.
    async fn receive(&mut self) -> (SocketAddr, Vec<u8>) {
        self.want_data = true;
        self.until(|_| false).await;
        self.want_data = false;
        assert!(!self.received.is_empty(), "nothing relayed back");
        self.received.remove(0)
    }
}

async fn recv(session: &UdpSocket) -> (Vec<u8>, SocketAddr) {
    let mut buf = vec![0; 2048];
    let (n, from) = tokio::time::timeout(PATIENCE, session.recv_from(&mut buf))
        .await
        .expect("nothing reached the session")
        .unwrap();
    (buf[..n].to_vec(), from)
}

async fn relays_both_ways(mut client: Client, relay: &Relay, session: &UdpSocket) {
    let port = session.local_addr().unwrap().port();
    let relayed = client.allocate().await;
    assert_eq!(
        relayed.ip(),
        ADVERTISED.parse::<IpAddr>().unwrap(),
        "relayed as the sessions' address"
    );
    assert_eq!(relay.stats().allocations(), 1);
    client.permit(ADVERTISED.parse().unwrap()).await;

    client.send(advertised(port), b"ping").await;
    let (data, from) = recv(session).await;
    assert_eq!(data, b"ping");
    assert!(from.ip().is_loopback(), "reached over loopback: {from}");

    session.send_to(b"pong", from).await.unwrap();
    let (peer, data) = client.receive().await;
    assert_eq!((peer, data.as_slice()), (advertised(port), &b"pong"[..]));
    assert!(relay.stats().bytes() >= 8);
}

#[tokio::test(flavor = "multi_thread")]
async fn relays_a_udp_client_to_a_session_and_back() {
    let (relay, session) = setup(4).await;
    let client = Client::udp(&relay, credentials()).await;
    relays_both_ways(client, &relay, &session).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn relays_a_tcp_client_to_a_session_and_back() {
    let (relay, session) = setup(4).await;
    let client = Client::tcp(&relay, credentials()).await;
    relays_both_ways(client, &relay, &session).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn relays_only_to_the_sessions() {
    let (relay, session) = setup(4).await;
    let port = session.local_addr().unwrap().port();
    // Something else on this machine, on a port that is not a session's.
    let other = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let other_port = other.local_addr().unwrap().port();
    let mut client = Client::udp(&relay, credentials()).await;
    client.allocate().await;
    client.permit(ADVERTISED.parse().unwrap()).await;
    client.permit("127.0.0.1".parse().unwrap()).await;

    client.send(advertised(other_port), b"not a session").await;
    client.send(SocketAddr::new("127.0.0.1".parse().unwrap(), port), b"not as advertised").await;
    client.send(advertised(port), b"ok").await;
    // In order through the relay: by the time the last arrives, the others would have.
    assert_eq!(recv(&session).await.0, b"ok");
    let mut buf = [0; 64];
    assert!(other.try_recv_from(&mut buf).is_err(), "relayed to something that is not a session");
}

#[tokio::test(flavor = "multi_thread")]
async fn refuses_a_wrong_password() {
    let (relay, _session) = setup(4).await;
    let c = mint(SECRET, "7", Duration::from_secs(3600), SystemTime::now());
    let mut client = Client::udp(&relay, TurnCredentials::new(&c.username, "guessed")).await;
    let got = client
        .until(|e| {
            matches!(e, TurnEvent::AllocationCreated(..) | TurnEvent::AllocationCreateFailed(_))
        })
        .await;
    assert!(!matches!(got, Some(TurnEvent::AllocationCreated(..))), "{got:?}");
    assert_eq!(relay.stats().allocations(), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn refuses_expired_credentials() {
    let (relay, _session) = setup(4).await;
    // Minted with its own key, but long ago.
    let c = mint(SECRET, "7", Duration::from_secs(3600), UNIX_EPOCH);
    let mut client = Client::udp(&relay, TurnCredentials::new(&c.username, &c.password)).await;
    let got = client
        .until(|e| {
            matches!(e, TurnEvent::AllocationCreated(..) | TurnEvent::AllocationCreateFailed(_))
        })
        .await;
    assert!(!matches!(got, Some(TurnEvent::AllocationCreated(..))), "{got:?}");
    assert_eq!(relay.stats().allocations(), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn caps_allocations() {
    let (relay, _session) = setup(1).await;
    let mut first = Client::udp(&relay, credentials()).await;
    first.allocate().await;
    let mut second = Client::udp(&relay, credentials()).await;
    let got = second
        .until(|e| {
            matches!(e, TurnEvent::AllocationCreated(..) | TurnEvent::AllocationCreateFailed(_))
        })
        .await;
    assert!(!matches!(got, Some(TurnEvent::AllocationCreated(..))), "{got:?}");
    assert_eq!(relay.stats().allocations(), 1);
}
