//! SovEmu asks the home router to forward its ports, so players away from home reach the game
//! sessions and the built-in relay with nothing set up on the router.
//!
//! Two protocols, both tried at once when the server starts:
//!
//! - **NAT-PMP** (RFC 6886, [`natpmp`]): asked of the default gateway. Apple's routers and
//!   miniupnpd (OpenWrt, pfSense, OPNsense) answer it. Preferred when it answers: one packet each
//!   way, and the router says how long it keeps each mapping.
//! - **UPnP IGD** ([`upnp`]): a multicast search, then SOAP over HTTP. Most other home routers.
//!
//! [`PortMapper`] runs in the background: it finds the router, asks it to forward each port in
//! [`Config::ports`] to this machine, renews each mapping at half its lifetime, searches again
//! when the router stops answering, and removes the mappings when it is stopped. It never fails:
//! a network with no router that answers (a container on a bridge network, a router with UPnP
//! off) is logged and left alone. [`PortMapper::status`] says what happened.

mod natpmp;
mod route;
mod upnp;

use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio_util::sync::CancellationToken;

/// A port's transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Transport {
    Udp,
    Tcp,
}

impl Transport {
    /// `udp` or `tcp`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Udp => "udp",
            Self::Tcp => "tcp",
        }
    }
}

/// How the router was asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    NatPmp,
    Upnp,
}

impl Protocol {
    /// `NAT-PMP` or `UPnP`.
    pub fn name(self) -> &'static str {
        match self {
            Self::NatPmp => "NAT-PMP",
            Self::Upnp => "UPnP",
        }
    }
}

/// A port to forward from the router to this machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Wanted {
    pub transport: Transport,
    pub port: u16,
    /// The router must forward the same port number: clients are told this port (the relay's).
    /// Otherwise any external port will do, since the session offers whatever the router gave.
    pub same_port: bool,
}

/// Where to look for the router. [`Default`] is the real network; tests point both at
/// loopback.
#[derive(Debug, Clone)]
pub struct Discovery {
    /// The NAT-PMP router (its address and port). `None`: the default gateway, on port 5351.
    pub nat_pmp: Option<SocketAddr>,
    /// Where UPnP's search goes: SSDP's multicast group, 239.255.255.250:1900.
    pub ssdp: SocketAddr,
    /// How long the UPnP search gathers answers.
    pub wait: Duration,
}

impl Default for Discovery {
    fn default() -> Self {
        Self { nat_pmp: None, ssdp: upnp::SSDP, wait: Duration::from_secs(3) }
    }
}

/// What to forward, and where to look.
#[derive(Debug, Clone)]
pub struct Config {
    pub ports: Vec<Wanted>,
    pub discovery: Discovery,
}

/// Where the port mapper is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum State {
    /// Looking for a router.
    #[default]
    Searching,
    /// No router answered UPnP or NAT-PMP. Searched again every ten minutes.
    NoRouter,
    /// A router answered; [`Status::ports`] says what it forwards.
    Active,
    /// Stopped, and the mappings removed.
    Stopped,
}

/// A wanted port, and what the router did with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortStatus {
    pub transport: Transport,
    pub internal: u16,
    /// The port the router forwards from, once it does.
    pub external: Option<u16>,
    /// Why it doesn't, when the router refused.
    pub error: Option<String>,
}

/// What the port mapper found and did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Status {
    pub state: State,
    pub protocol: Option<Protocol>,
    /// The router's address on the LAN.
    pub router: Option<IpAddr>,
    /// This machine's address, as the router sees it: where it forwards to.
    pub local_ip: Option<IpAddr>,
    /// The router's own address on the internet, as it reports it.
    pub external_ip: Option<IpAddr>,
    pub ports: Vec<PortStatus>,
}

impl Status {
    /// Where players on the internet reach `internal` (a [`Transport::Udp`] port): the
    /// router's external address and the port it forwards. `None` unless the router forwards it
    /// and its address is an internet one (see [`is_internet_address`]).
    pub fn public_addr(&self, transport: Transport, internal: u16) -> Option<SocketAddr> {
        let ip = self.external_ip.filter(|ip| is_internet_address(*ip))?;
        let port =
            self.ports.iter().find(|p| p.transport == transport && p.internal == internal)?;
        Some(SocketAddr::new(ip, port.external?))
    }
}

/// Whether `ip` can be an address on the internet. A router whose own address is private (RFC
/// 1918), the carriers' shared range (100.64.0.0/10, carrier-grade NAT), link-local or loopback
/// has another NAT in front of it, so what it forwards can't be reached from the internet.
pub fn is_internet_address(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, ..] = v4.octets();
            let shared = a == 100 && (64..128).contains(&b);
            !(v4.is_private()
                || shared
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast())
        }
        IpAddr::V6(v6) => {
            let unique_local = v6.segments()[0] & 0xfe00 == 0xfc00;
            let link_local = v6.segments()[0] & 0xffc0 == 0xfe80;
            !(v6.is_loopback() || v6.is_unspecified() || unique_local || link_local)
        }
    }
}

/// What asking the router can end in.
#[derive(Debug)]
pub(crate) enum Error {
    Io(std::io::Error),
    /// Nobody answered.
    Timeout,
    /// No router answered the search.
    NotFound,
    /// The router said no.
    Refused {
        code: u16,
        reason: String,
    },
    /// Another machine has that external port.
    Conflict,
    /// The router forwards another port than the one that had to be the same.
    OtherPort(u16),
    Http(String),
    BadResponse(&'static str),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "{e}"),
            Self::Timeout => f.write_str("the router did not answer"),
            Self::NotFound => f.write_str("no router answered"),
            Self::Refused { code, reason } if reason.is_empty() => write!(f, "refused ({code})"),
            Self::Refused { code, reason } => write!(f, "refused: {reason} ({code})"),
            Self::Conflict => f.write_str("another device on the network has this port"),
            Self::OtherPort(port) => write!(f, "the router would only forward port {port}"),
            Self::Http(e) => write!(f, "{e}"),
            Self::BadResponse(what) => f.write_str(what),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl Error {
    /// The router is gone (or never was), as opposed to having said no.
    fn unreachable(&self) -> bool {
        matches!(self, Self::Io(_) | Self::Timeout | Self::NotFound | Self::Http(_))
    }
}

/// A mapping the router made: the external port and how long it keeps it, in seconds (0: until
/// it is deleted or the router restarts).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Granted {
    pub(crate) external: u16,
    pub(crate) lifetime: u32,
}

/// A router that forwards ports: NAT-PMP's, UPnP's, or a test's.
pub(crate) trait Gateway: Send + Sync + 'static {
    fn protocol(&self) -> Protocol;
    fn router(&self) -> IpAddr;
    fn local_ip(&self) -> IpAddr;
    fn external_ip(&self) -> impl Future<Output = Result<IpAddr, Error>> + Send;
    /// Forwards `external` (a suggestion, for NAT-PMP) to `internal` on this machine for
    /// `lifetime` seconds.
    fn map(
        &self,
        transport: Transport,
        internal: u16,
        external: u16,
        lifetime: u32,
    ) -> impl Future<Output = Result<Granted, Error>> + Send;
    fn unmap(
        &self,
        transport: Transport,
        internal: u16,
        external: u16,
    ) -> impl Future<Output = Result<(), Error>> + Send;
}

/// Finds the router.
pub(crate) trait Discover: Send + Sync + 'static {
    type Gateway: Gateway;
    fn find(&self) -> impl Future<Output = Option<Self::Gateway>> + Send;
}

/// The address this machine reaches `router` from: the one the router forwards to.
/// Connecting a UDP socket sends nothing.
pub(crate) fn local_ip_towards(router: IpAddr) -> Result<IpAddr, Error> {
    let any: IpAddr = if router.is_ipv4() {
        std::net::Ipv4Addr::UNSPECIFIED.into()
    } else {
        std::net::Ipv6Addr::UNSPECIFIED.into()
    };
    let socket = std::net::UdpSocket::bind((any, 0))?;
    socket.connect((router, 9))?;
    Ok(socket.local_addr()?.ip())
}

/// The real network's routers.
#[derive(Debug)]
enum AnyGateway {
    NatPmp(natpmp::NatPmp),
    Upnp(upnp::Upnp),
}

impl Gateway for AnyGateway {
    fn protocol(&self) -> Protocol {
        match self {
            Self::NatPmp(g) => g.protocol(),
            Self::Upnp(g) => g.protocol(),
        }
    }

    fn router(&self) -> IpAddr {
        match self {
            Self::NatPmp(g) => g.router(),
            Self::Upnp(g) => g.router(),
        }
    }

    fn local_ip(&self) -> IpAddr {
        match self {
            Self::NatPmp(g) => g.local_ip(),
            Self::Upnp(g) => g.local_ip(),
        }
    }

    async fn external_ip(&self) -> Result<IpAddr, Error> {
        match self {
            Self::NatPmp(g) => g.external_ip().await,
            Self::Upnp(g) => g.external_ip().await,
        }
    }

    async fn map(
        &self,
        transport: Transport,
        internal: u16,
        external: u16,
        lifetime: u32,
    ) -> Result<Granted, Error> {
        match self {
            Self::NatPmp(g) => g.map(transport, internal, external, lifetime).await,
            Self::Upnp(g) => g.map(transport, internal, external, lifetime).await,
        }
    }

    async fn unmap(&self, transport: Transport, internal: u16, external: u16) -> Result<(), Error> {
        match self {
            Self::NatPmp(g) => g.unmap(transport, internal, external).await,
            Self::Upnp(g) => g.unmap(transport, internal, external).await,
        }
    }
}

/// Asks NAT-PMP and UPnP at once, preferring NAT-PMP.
#[derive(Debug)]
struct Network(Discovery);

impl Discover for Network {
    type Gateway = AnyGateway;

    async fn find(&self) -> Option<AnyGateway> {
        let nat_pmp = async {
            let router = self.0.nat_pmp.or_else(|| {
                route::default_gateway().map(|ip| SocketAddr::new(ip.into(), natpmp::PORT))
            });
            let Some(router) = router else {
                tracing::debug!("no default gateway known: NAT-PMP not tried");
                return None;
            };
            natpmp::NatPmp::find(router)
                .await
                .inspect_err(|e| tracing::debug!(router = %router, error = %e, "no NAT-PMP"))
                .ok()
        };
        let upnp = async {
            upnp::Upnp::find(self.0.ssdp, self.0.wait)
                .await
                .inspect_err(|e| tracing::debug!(error = %e, "no UPnP gateway"))
                .ok()
        };
        let (nat_pmp, upnp) = tokio::join!(nat_pmp, upnp);
        nat_pmp.map(AnyGateway::NatPmp).or(upnp.map(AnyGateway::Upnp))
    }
}

/// How often things happen.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Timing {
    /// The lifetime asked for, in seconds: RFC 6886's recommended two hours.
    pub(crate) lease: u32,
    /// The longest between renewals: a permanent mapping is refreshed this often, in case the
    /// router restarted and forgot it.
    pub(crate) refresh: Duration,
    /// The shortest between renewals, however short the router's lifetime.
    pub(crate) min_renew: Duration,
    /// How long after finding no router it looks again.
    pub(crate) search_again: Duration,
    /// How long removing the mappings may take, when stopping.
    pub(crate) unmap_within: Duration,
}

const TIMING: Timing = Timing {
    lease: 7200,
    refresh: Duration::from_secs(30 * 60),
    min_renew: Duration::from_secs(60),
    search_again: Duration::from_secs(10 * 60),
    unmap_within: Duration::from_secs(5),
};

/// Asks the router to forward ports, in the background, until stopped.
#[derive(Debug)]
pub struct PortMapper {
    status: Arc<Mutex<Status>>,
    cancel: CancellationToken,
    task: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

/// Told the status each time it changes.
type OnChange = Box<dyn Fn(&Status) + Send + Sync>;

impl PortMapper {
    /// Starts asking (on the current Tokio runtime). `on_change` is called each time the
    /// status changes: the router found, a port forwarded or refused, the address changed.
    pub fn start(config: Config, on_change: impl Fn(&Status) + Send + Sync + 'static) -> Self {
        Self::start_with(Network(config.discovery), config.ports, TIMING, Box::new(on_change))
    }

    fn start_with<D: Discover>(
        discover: D,
        ports: Vec<Wanted>,
        timing: Timing,
        on_change: OnChange,
    ) -> Self {
        let status = Arc::new(Mutex::new(Status { ports: unmapped(&ports), ..Status::default() }));
        let cancel = CancellationToken::new();
        let task = tokio::spawn(run(
            discover,
            ports,
            timing,
            Publisher { status: status.clone(), on_change },
            cancel.clone(),
        ));
        Self { status, cancel, task: Mutex::new(Some(task)) }
    }

    /// What it has found and done so far.
    pub fn status(&self) -> Status {
        self.status.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Removes the mappings and stops. Bounded: a router that doesn't answer gets a few seconds,
    /// then its mappings are left to expire.
    pub async fn stop(&self) {
        self.cancel.cancel();
        let task = self.task.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(task) = task {
            let abort = task.abort_handle();
            if tokio::time::timeout(TIMING.unmap_within + Duration::from_secs(1), task)
                .await
                .is_err()
            {
                abort.abort();
            }
        }
    }
}

/// Every wanted port, none forwarded.
fn unmapped(ports: &[Wanted]) -> Vec<PortStatus> {
    ports
        .iter()
        .map(|w| PortStatus {
            transport: w.transport,
            internal: w.port,
            external: None,
            error: None,
        })
        .collect()
}

/// Keeps the status and tells of changes.
struct Publisher {
    status: Arc<Mutex<Status>>,
    on_change: OnChange,
}

impl Publisher {
    fn publish(&self, status: Status) {
        let mut current = self.status.lock().unwrap_or_else(|e| e.into_inner());
        if *current != status {
            *current = status.clone();
            drop(current);
            (self.on_change)(&status);
        }
    }
}

/// A mapping held: what the router granted, and when.
#[derive(Debug, Clone, Copy)]
struct Lease {
    granted: Granted,
    at: Instant,
}

/// Alternatives to a port another device has: a thousand up, then two and three thousand.
fn alternatives(port: u16) -> impl Iterator<Item = u16> {
    (1..=3u32).map(move |k| {
        let p = u32::from(port) + k * 1000;
        // Wrap into the registered and dynamic ports (1024–65535).
        (if p > 65535 { p - 65535 + 1023 } else { p }) as u16
    })
}

/// Asks for `wanted`, suggesting the external port it had before.
async fn map_one<G: Gateway>(
    gateway: &G,
    wanted: &Wanted,
    previous: Option<u16>,
    lease: u32,
) -> Result<Granted, Error> {
    let (transport, port) = (wanted.transport, wanted.port);
    match gateway.map(transport, port, previous.unwrap_or(port), lease).await {
        Ok(granted) if wanted.same_port && granted.external != port => {
            let _ = gateway.unmap(transport, port, granted.external).await;
            Err(Error::OtherPort(granted.external))
        }
        Err(Error::Conflict) if !wanted.same_port => {
            for external in alternatives(port) {
                match gateway.map(transport, port, external, lease).await {
                    Err(Error::Conflict) => continue,
                    other => return other,
                }
            }
            Err(Error::Conflict)
        }
        other => other,
    }
}

/// The port mapper's life: search, map, renew, and on cancel remove.
async fn run<D: Discover>(
    discover: D,
    wanted: Vec<Wanted>,
    timing: Timing,
    publisher: Publisher,
    cancel: CancellationToken,
) {
    let ports: Vec<String> =
        wanted.iter().map(|w| format!("{}/{}", w.port, w.transport.name())).collect();
    tracing::info!(ports = %ports.join(","), "asking the router to forward ports (UPnP, NAT-PMP)");
    let mut said_no_router = false;
    loop {
        let found = tokio::select! {
            () = cancel.cancelled() => break,
            found = discover.find() => found,
        };
        let Some(gateway) = found else {
            let message = "no router answered UPnP or NAT-PMP: forward ports by hand if players \
                           away from home can't connect";
            if said_no_router {
                tracing::debug!("{message}");
            } else {
                tracing::info!("{message}");
                said_no_router = true;
            }
            publisher.publish(Status {
                state: State::NoRouter,
                ports: unmapped(&wanted),
                ..Status::default()
            });
            tokio::select! {
                () = cancel.cancelled() => break,
                () = tokio::time::sleep(timing.search_again) => continue,
            }
        };
        said_no_router = false;
        if keep(&gateway, &wanted, timing, &publisher, &cancel).await {
            publisher.publish(Status {
                state: State::Searching,
                ports: unmapped(&wanted),
                ..Status::default()
            });
            continue;
        }
        break;
    }
    let mut stopped = publisher.status.lock().unwrap_or_else(|e| e.into_inner()).clone();
    stopped.state = State::Stopped;
    for p in &mut stopped.ports {
        p.external = None;
    }
    publisher.publish(stopped);
}

/// Keeps `wanted` forwarded on `gateway` until cancelled (false, once the mappings are removed)
/// or the router stops answering (true: search again).
async fn keep<G: Gateway>(
    gateway: &G,
    wanted: &[Wanted],
    timing: Timing,
    publisher: &Publisher,
    cancel: &CancellationToken,
) -> bool {
    let protocol = gateway.protocol().name();
    let mut external = gateway.external_ip().await.ok();
    tracing::info!(
        protocol,
        router = %gateway.router(),
        local = %gateway.local_ip(),
        external = %external.map_or_else(|| "unknown".to_owned(), |ip| ip.to_string()),
        "router found"
    );
    warn_if_private(external);
    let mut leases: Vec<Option<Lease>> = vec![None; wanted.len()];
    let mut errors: Vec<Option<String>> = vec![None; wanted.len()];
    loop {
        for (i, w) in wanted.iter().enumerate() {
            let previous = leases[i].map(|l| l.granted.external);
            let result = tokio::select! {
                () = cancel.cancelled() => break,
                result = map_one(gateway, w, previous, timing.lease) => result,
            };
            match result {
                Ok(granted) => {
                    if previous != Some(granted.external) {
                        tracing::info!(
                            protocol,
                            transport = w.transport.name(),
                            internal = w.port,
                            external = granted.external,
                            lifetime_secs = granted.lifetime,
                            "router forwards port"
                        );
                    }
                    leases[i] = Some(Lease { granted, at: Instant::now() });
                    errors[i] = None;
                }
                Err(e) => {
                    let message = e.to_string();
                    if errors[i].as_ref() != Some(&message) {
                        tracing::warn!(
                            protocol,
                            transport = w.transport.name(),
                            internal = w.port,
                            error = %message,
                            forward_by_hand = w.same_port,
                            "router refused to forward port"
                        );
                    }
                    leases[i] = None;
                    errors[i] = Some(message);
                }
            }
        }
        if cancel.is_cancelled() {
            remove(gateway, wanted, &leases, timing).await;
            return false;
        }
        publisher.publish(Status {
            state: State::Active,
            protocol: Some(gateway.protocol()),
            router: Some(gateway.router()),
            local_ip: Some(gateway.local_ip()),
            external_ip: external,
            ports: wanted
                .iter()
                .zip(leases.iter().zip(&errors))
                .map(|(w, (lease, error))| PortStatus {
                    transport: w.transport,
                    internal: w.port,
                    external: lease.map(|l| l.granted.external),
                    error: error.clone(),
                })
                .collect(),
        });
        let wait = renew_in(&leases, timing);
        tokio::select! {
            () = cancel.cancelled() => {
                remove(gateway, wanted, &leases, timing).await;
                return false;
            }
            () = tokio::time::sleep(wait) => {}
        }
        match gateway.external_ip().await {
            Ok(ip) if Some(ip) != external => {
                tracing::info!(protocol, external = %ip, "the router's internet address changed");
                warn_if_private(Some(ip));
                external = Some(ip);
            }
            Ok(_) => {}
            Err(e) if e.unreachable() => {
                tracing::warn!(protocol, router = %gateway.router(), error = %e, "the router stopped answering: searching again");
                return true;
            }
            Err(e) => tracing::debug!(protocol, error = %e, "the router didn't give its address"),
        }
    }
}

fn warn_if_private(external: Option<IpAddr>) {
    if let Some(ip) = external.filter(|ip| !is_internet_address(*ip)) {
        tracing::warn!(
            external = %ip,
            "the router's own internet address is a private one: another router, or the internet \
             provider's carrier-grade NAT, is in front of it, so ports it forwards can't be reached \
             from the internet"
        );
    }
}

/// When to renew: at half the shortest lifetime granted, between [`Timing::min_renew`] and
/// [`Timing::refresh`] (refused ports are asked again then too).
fn renew_in(leases: &[Option<Lease>], timing: Timing) -> Duration {
    leases
        .iter()
        .flatten()
        .filter(|l| l.granted.lifetime != 0)
        .map(|l| {
            let due = l.at + Duration::from_secs(u64::from(l.granted.lifetime) / 2);
            due.saturating_duration_since(Instant::now())
        })
        .min()
        .unwrap_or(timing.refresh)
        .clamp(timing.min_renew, timing.refresh)
}

/// Removes every mapping held, within [`Timing::unmap_within`].
async fn remove<G: Gateway>(
    gateway: &G,
    wanted: &[Wanted],
    leases: &[Option<Lease>],
    timing: Timing,
) {
    let held: Vec<(&Wanted, Granted)> =
        wanted.iter().zip(leases).filter_map(|(w, l)| Some((w, l.as_ref()?.granted))).collect();
    if held.is_empty() {
        return;
    }
    let removing = async {
        let mut removed = 0;
        for (w, granted) in &held {
            match gateway.unmap(w.transport, w.port, granted.external).await {
                Ok(()) => removed += 1,
                Err(e) => tracing::debug!(port = w.port, error = %e, "forward not removed"),
            }
        }
        removed
    };
    match tokio::time::timeout(timing.unmap_within, removing).await {
        Ok(removed) => {
            tracing::info!(removed, held = held.len(), "removed the router's port forwards")
        }
        Err(_) => tracing::warn!("the router didn't answer: its port forwards expire on their own"),
    }
}

#[cfg(test)]
mod tests;
