//! The port mapper's life against a fake router: what it maps, renews, gives up on, searches
//! again for and removes. No real network: the fake is a trait seam, and the one test of the
//! real discovery talks to a NAT-PMP router on loopback.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use super::*;

/// A router whose behaviour each test sets.
#[derive(Debug)]
struct FakeRouter {
    /// It answers the search.
    present: AtomicBool,
    /// It answers anything at all.
    answering: AtomicBool,
    external_ip: Mutex<IpAddr>,
    /// External ports another device has.
    taken: Vec<u16>,
    /// Forwards a different port than asked, for this internal one.
    shifted: Option<u16>,
    /// The lifetime it grants, at most.
    lifetime: u32,
    /// What it forwards now: (transport, external) → internal.
    forwards: Mutex<HashMap<(Transport, u16), u16>>,
    /// Map requests per internal port.
    asked: Mutex<HashMap<u16, usize>>,
    searches: AtomicUsize,
}

impl FakeRouter {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            present: AtomicBool::new(true),
            answering: AtomicBool::new(true),
            external_ip: Mutex::new(IpAddr::from([203, 0, 113, 7])),
            taken: Vec::new(),
            shifted: None,
            lifetime: 7200,
            forwards: Mutex::default(),
            asked: Mutex::default(),
            searches: AtomicUsize::new(0),
        })
    }

    fn forwards(&self) -> HashMap<(Transport, u16), u16> {
        self.forwards.lock().unwrap().clone()
    }

    fn asked(&self, port: u16) -> usize {
        self.asked.lock().unwrap().get(&port).copied().unwrap_or(0)
    }

    fn check(&self) -> Result<(), Error> {
        if self.answering.load(Ordering::SeqCst) { Ok(()) } else { Err(Error::Timeout) }
    }
}

#[derive(Debug)]
struct FakeGateway(Arc<FakeRouter>);

impl Gateway for FakeGateway {
    fn protocol(&self) -> Protocol {
        Protocol::NatPmp
    }

    fn router(&self) -> IpAddr {
        IpAddr::from([192, 168, 1, 1])
    }

    fn local_ip(&self) -> IpAddr {
        IpAddr::from([192, 168, 1, 10])
    }

    async fn external_ip(&self) -> Result<IpAddr, Error> {
        self.0.check()?;
        Ok(*self.0.external_ip.lock().unwrap())
    }

    async fn map(
        &self,
        transport: Transport,
        internal: u16,
        external: u16,
        lifetime: u32,
    ) -> Result<Granted, Error> {
        self.0.check()?;
        *self.0.asked.lock().unwrap().entry(internal).or_default() += 1;
        if self.0.taken.contains(&external) {
            return Err(Error::Conflict);
        }
        let external = if self.0.shifted == Some(internal) { external + 1 } else { external };
        self.0.forwards.lock().unwrap().insert((transport, external), internal);
        Ok(Granted { external, lifetime: lifetime.min(self.0.lifetime) })
    }

    async fn unmap(
        &self,
        transport: Transport,
        _internal: u16,
        external: u16,
    ) -> Result<(), Error> {
        self.0.check()?;
        self.0.forwards.lock().unwrap().remove(&(transport, external));
        Ok(())
    }
}

#[derive(Debug)]
struct FakeDiscover(Arc<FakeRouter>);

impl Discover for FakeDiscover {
    type Gateway = FakeGateway;

    async fn find(&self) -> Option<FakeGateway> {
        self.0.searches.fetch_add(1, Ordering::SeqCst);
        (self.0.present.load(Ordering::SeqCst) && self.0.answering.load(Ordering::SeqCst))
            .then(|| FakeGateway(self.0.clone()))
    }
}

const FAST: Timing = Timing {
    lease: 7200,
    refresh: Duration::from_millis(200),
    min_renew: Duration::from_millis(20),
    search_again: Duration::from_millis(50),
    unmap_within: Duration::from_secs(2),
};

fn udp(port: u16) -> Wanted {
    Wanted { transport: Transport::Udp, port, same_port: false }
}

fn relay(transport: Transport) -> Wanted {
    Wanted { transport, port: 3478, same_port: true }
}

/// The mapper, and how many times it has told of a change.
fn start(
    router: &Arc<FakeRouter>,
    ports: Vec<Wanted>,
    timing: Timing,
) -> (PortMapper, Arc<AtomicUsize>) {
    let changes = Arc::new(AtomicUsize::new(0));
    let counted = changes.clone();
    let mapper = PortMapper::start_with(
        FakeDiscover(router.clone()),
        ports,
        timing,
        Box::new(move |_| {
            counted.fetch_add(1, Ordering::SeqCst);
        }),
    );
    (mapper, changes)
}

/// Waits (up to ten seconds) for `done`.
async fn until(mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < deadline, "timed out");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test]
async fn every_port_is_forwarded_and_removed_on_stop() {
    let router = FakeRouter::new();
    let ports = vec![udp(50000), udp(50001), relay(Transport::Udp), relay(Transport::Tcp)];
    let (mapper, changes) = start(&router, ports, TIMING);
    until(|| mapper.status().state == State::Active).await;

    let status = mapper.status();
    assert_eq!(status.protocol, Some(Protocol::NatPmp));
    assert_eq!(status.local_ip, Some(IpAddr::from([192, 168, 1, 10])));
    assert!(status.ports.iter().all(|p| p.external == Some(p.internal) && p.error.is_none()));
    assert_eq!(
        status.public_addr(Transport::Udp, 50001),
        Some("203.0.113.7:50001".parse().unwrap())
    );
    assert_eq!(status.public_addr(Transport::Udp, 50002), None, "not a wanted port");
    assert_eq!(router.forwards().len(), 4);
    assert!(changes.load(Ordering::SeqCst) >= 1, "told");

    mapper.stop().await;
    assert!(router.forwards().is_empty(), "every forward removed: {:?}", router.forwards());
    let status = mapper.status();
    assert_eq!(status.state, State::Stopped);
    assert!(status.ports.iter().all(|p| p.external.is_none()));
}

#[tokio::test]
async fn a_taken_port_gets_another_unless_it_must_be_the_same() {
    let mut router = FakeRouter::new();
    Arc::get_mut(&mut router).unwrap().taken = vec![50000, 3478];
    let (mapper, _) = start(&router, vec![udp(50000), relay(Transport::Udp)], TIMING);
    until(|| mapper.status().state == State::Active).await;

    let status = mapper.status();
    assert_eq!(status.ports[0].external, Some(51000), "the next one free");
    assert_eq!(status.ports[1].external, None);
    assert!(status.ports[1].error.as_deref().unwrap().contains("another device"), "{status:?}");
    mapper.stop().await;
}

#[tokio::test]
async fn a_router_that_forwards_another_port_for_the_relay_is_refused() {
    let mut router = FakeRouter::new();
    Arc::get_mut(&mut router).unwrap().shifted = Some(3478);
    let (mapper, _) = start(&router, vec![relay(Transport::Tcp)], TIMING);
    until(|| mapper.status().state == State::Active).await;

    let port = &mapper.status().ports[0];
    assert_eq!(port.external, None);
    assert_eq!(port.error.as_deref(), Some("the router would only forward port 3479"));
    assert!(router.forwards().is_empty(), "the wrong forward was taken back");
    mapper.stop().await;
}

#[tokio::test]
async fn mappings_are_renewed_at_half_their_lifetime() {
    let mut router = FakeRouter::new();
    // Granted for 0 seconds' half: renewed as soon as the floor allows.
    Arc::get_mut(&mut router).unwrap().lifetime = 1;
    let (mapper, _) = start(&router, vec![udp(50000)], FAST);
    until(|| router.asked(50000) >= 3).await;
    assert_eq!(mapper.status().ports[0].external, Some(50000), "the same port each time");
    mapper.stop().await;
}

#[tokio::test]
async fn with_no_router_it_searches_again() {
    let router = FakeRouter::new();
    router.present.store(false, Ordering::SeqCst);
    let (mapper, _) = start(&router, vec![udp(50000)], FAST);
    until(|| mapper.status().state == State::NoRouter).await;
    assert_eq!(mapper.status().ports[0].external, None);

    // UPnP turned on later, or the router came back from a restart.
    router.present.store(true, Ordering::SeqCst);
    until(|| mapper.status().state == State::Active).await;
    assert!(router.searches.load(Ordering::SeqCst) >= 2);
    mapper.stop().await;
}

#[tokio::test]
async fn a_router_that_stops_answering_is_searched_for_again() {
    let router = FakeRouter::new();
    let (mapper, _) = start(&router, vec![udp(50000)], FAST);
    until(|| mapper.status().state == State::Active).await;

    router.answering.store(false, Ordering::SeqCst);
    until(|| mapper.status().state == State::NoRouter).await;
    router.answering.store(true, Ordering::SeqCst);
    until(|| mapper.status().state == State::Active).await;
    mapper.stop().await;
}

#[tokio::test]
async fn a_private_external_address_is_never_offered() {
    let router = FakeRouter::new();
    *router.external_ip.lock().unwrap() = IpAddr::from([100, 64, 12, 34]);
    let (mapper, _) = start(&router, vec![udp(50000)], TIMING);
    until(|| mapper.status().state == State::Active).await;

    let status = mapper.status();
    assert_eq!(status.ports[0].external, Some(50000), "forwarded all the same");
    assert_eq!(status.public_addr(Transport::Udp, 50000), None, "but not reachable");
    mapper.stop().await;
}

#[tokio::test]
async fn a_changed_external_address_is_picked_up() {
    let router = FakeRouter::new();
    let (mapper, _) = start(&router, vec![udp(50000)], FAST);
    until(|| mapper.status().state == State::Active).await;
    *router.external_ip.lock().unwrap() = IpAddr::from([198, 51, 100, 9]);
    until(|| mapper.status().external_ip == Some(IpAddr::from([198, 51, 100, 9]))).await;
    mapper.stop().await;
}

#[tokio::test]
async fn stopping_while_searching_is_prompt() {
    let router = FakeRouter::new();
    router.present.store(false, Ordering::SeqCst);
    let (mapper, _) = start(&router, vec![udp(50000)], TIMING);
    until(|| mapper.status().state == State::NoRouter).await;
    let started = Instant::now();
    mapper.stop().await;
    assert!(started.elapsed() < Duration::from_secs(1));
    assert_eq!(mapper.status().state, State::Stopped);
}

#[test]
fn alternatives_stay_in_range() {
    assert_eq!(alternatives(50000).collect::<Vec<_>>(), [51000, 52000, 53000]);
    assert!(alternatives(65000).all(|p| p >= 1024));
}

#[test]
fn renewal_is_at_half_the_shortest_lifetime_within_bounds() {
    let lease =
        |lifetime| Some(Lease { granted: Granted { external: 1, lifetime }, at: Instant::now() });
    let renew = |leases: &[Option<Lease>]| renew_in(leases, TIMING);
    assert_eq!(renew(&[]), TIMING.refresh, "nothing held: try again in a while");
    assert_eq!(renew(&[lease(0)]), TIMING.refresh, "permanent: refreshed");
    let half_hour = renew(&[lease(7200), lease(3600)]);
    assert!(half_hour <= Duration::from_secs(1800) && half_hour > Duration::from_secs(1790));
    assert_eq!(renew(&[lease(10)]), TIMING.min_renew);
}

#[test]
fn internet_addresses() {
    for ip in ["203.0.113.7", "8.8.8.8", "2001:db8::1", "2606:4700::1111"] {
        assert!(is_internet_address(ip.parse().unwrap()), "{ip}");
    }
    for ip in [
        "192.168.1.1",
        "10.0.0.1",
        "172.16.0.1",
        "100.64.0.1",
        "100.127.255.1",
        "169.254.1.1",
        "127.0.0.1",
        "0.0.0.0",
        "fd00::1",
        "fe80::1",
        "::1",
    ] {
        assert!(!is_internet_address(ip.parse().unwrap()), "{ip}");
    }
}

#[tokio::test]
async fn the_real_discovery_finds_a_nat_pmp_router() {
    let router = natpmp::tests::fake_router(0).await;
    // UPnP's search goes to a socket that never answers.
    let silent = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let discovery = Discovery {
        nat_pmp: Some(router),
        ssdp: silent.local_addr().unwrap(),
        wait: Duration::from_millis(100),
    };
    let mapper = PortMapper::start(Config { ports: vec![udp(50000)], discovery }, |_| {});
    until(|| mapper.status().state == State::Active).await;
    let status = mapper.status();
    assert_eq!(status.protocol, Some(Protocol::NatPmp));
    assert_eq!(status.external_ip, Some(IpAddr::from([203, 0, 113, 7])));
    assert_eq!(status.ports[0].external, Some(50000));
    mapper.stop().await;
    assert_eq!(mapper.status().state, State::Stopped);
}
