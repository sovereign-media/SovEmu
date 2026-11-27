//! The check of a player's connection before the stream starts (#243): where the link goes
//! ([`PathClass`]), how long to measure it, when the measurement is done ([`Probe`]), and the
//! rate the stream then starts at ([`start_kbps`]).
//!
//! While a new client is checked the game does not run and no video is sent: str0m pads the
//! link toward the stream's ceiling (`bwe().set_bitrate`) and its estimate climbs to what the
//! link carries. The stream then starts at a rate the link was seen to carry, instead of at the
//! configured one and adapting from there: on a slow link that is several cuts (each a stall)
//! saved, and on a fast one ten seconds of a softer picture.

use std::net::IpAddr;
use std::time::{Duration, Instant};

use crate::adapt::Limits;

/// The share of the estimate the stream may use, as `adapt.rs` takes it.
const ESTIMATE_SHARE: f64 = 0.85;

/// Over TCP to the relay, retransmissions hide loss until the queue is long: the start keeps
/// well under the estimate.
const RELAY_TCP_SHARE: f64 = 0.6;

/// The estimate has not moved by more than [`MOVE`] either way for this long: it has found the
/// link.
const STEADY: Duration = Duration::from_millis(500);
const MOVE: f64 = 0.05;

/// Where a client's link goes, from its selected candidate pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PathClass {
    /// The same network: the client's own private address reached the worker.
    Lan,
    /// From elsewhere, straight to the server (its public address, or one STUN found).
    Direct,
    /// Through the relay, before the client has said over what.
    Relay,
    RelayUdp,
    RelayTcp,
    RelayTls,
}

impl PathClass {
    /// How the worker sees the link: the relay forwards to its sessions over loopback, so a
    /// relayed client comes from a loopback address; a private address is the server's own
    /// network, and so is a global IPv6 address in the same /64 as the session's own (`local`,
    /// the address the client sent to): one home network's prefix. Anything else is direct.
    pub(crate) fn of(remote: IpAddr, local: Option<IpAddr>) -> Self {
        if remote.is_loopback() {
            return Self::Relay;
        }
        let private = match remote {
            // Not 100.64.0.0/10: that is a carrier's NAT or a VPN, not the server's network.
            IpAddr::V4(v4) => v4.is_private() || v4.is_link_local(),
            IpAddr::V6(v6) => {
                v6.is_unique_local()
                    || v6.is_unicast_link_local()
                    || matches!(local, Some(IpAddr::V6(ours)) if same_64(v6, ours))
            }
        };
        if private { Self::Lan } else { Self::Direct }
    }

    /// Takes in what the client says of its path (`ClientStats::path`): only it can tell the
    /// relay's transports apart, as the relay forwards UDP whichever the client used. A client
    /// on loopback that says it is not relayed is on the server's own machine. What the worker
    /// saw for itself stands otherwise.
    pub(crate) fn with_client(self, client: Option<&str>) -> Self {
        match (self, client) {
            (Self::Relay | Self::RelayUdp | Self::RelayTcp | Self::RelayTls, Some(said)) => {
                match said {
                    "relay-udp" => Self::RelayUdp,
                    "relay-tcp" => Self::RelayTcp,
                    "relay-tls" => Self::RelayTls,
                    "direct" => Self::Lan,
                    _ => self,
                }
            }
            _ => self,
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Lan => "lan",
            Self::Direct => "direct",
            Self::Relay => "relay",
            Self::RelayUdp => "relay-udp",
            Self::RelayTcp => "relay-tcp",
            Self::RelayTls => "relay-tls",
        }
    }

    /// Over TCP (or TLS, also TCP) to the relay.
    fn over_tcp(self) -> bool {
        matches!(self, Self::RelayTcp | Self::RelayTls)
    }
}

/// Two IPv6 addresses on the same network: the first 64 bits, the prefix a router hands out.
fn same_64(a: std::net::Ipv6Addr, b: std::net::Ipv6Addr) -> bool {
    a.segments()[..4] == b.segments()[..4]
}

/// Why a check ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Stop {
    /// The estimate reached the most the stream may use.
    Ceiling,
    /// It stopped rising.
    Steady,
    /// The client lost packets, or its round trip rose: the link is full.
    Trouble,
    /// Out of time.
    Budget,
    /// The link cannot be probed (no transport-wide feedback).
    Unavailable,
}

impl Stop {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Ceiling => "ceiling",
            Self::Steady => "steady",
            Self::Trouble => "trouble",
            Self::Budget => "budget",
            Self::Unavailable => "unavailable",
        }
    }
}

/// One check of a client's link: estimates go in, and it says when it is done.
#[derive(Debug, Clone)]
pub(crate) struct Probe {
    started: Instant,
    budget: Duration,
    /// The most the stream may use, video and audio, kbit/s.
    ceiling_kbps: u64,
    /// The latest estimate, kbit/s, and when it last moved by more than [`MOVE`]: a probe
    /// that overshoots, or one started high on the same network, falls back to the link.
    estimate_kbps: Option<u64>,
    moved_at: Option<Instant>,
    stopped: Option<(Stop, Instant)>,
}

impl Probe {
    pub(crate) fn new(budget: Duration, ceiling_kbps: u64, now: Instant) -> Self {
        Self {
            started: now,
            budget,
            ceiling_kbps,
            estimate_kbps: None,
            moved_at: None,
            stopped: None,
        }
    }

    /// A check is wanted at all: a budget of 0 turns it off.
    pub(crate) fn wanted(budget: Duration) -> bool {
        !budget.is_zero()
    }

    /// A shorter budget, once the path is known to be the same network.
    pub(crate) fn shorten(&mut self, budget: Duration) {
        self.budget = self.budget.min(budget);
    }

    /// A send-bandwidth estimate, bit/s.
    pub(crate) fn estimate(&mut self, bps: u64, now: Instant) {
        let kbps = bps / 1000;
        let moved = self
            .estimate_kbps
            .is_none_or(|last| (kbps as f64 - last as f64).abs() > last as f64 * MOVE);
        if moved {
            self.moved_at = Some(now);
        }
        self.estimate_kbps = Some(kbps);
        if kbps >= self.ceiling_kbps {
            self.stop(Stop::Ceiling, now);
        }
    }

    /// The client is losing packets or its round trip rose: the link is full.
    pub(crate) fn trouble(&mut self, now: Instant) {
        self.stop(Stop::Trouble, now);
    }

    /// str0m cannot probe this link.
    pub(crate) fn unavailable(&mut self, now: Instant) {
        self.stop(Stop::Unavailable, now);
    }

    fn stop(&mut self, why: Stop, now: Instant) {
        self.stopped.get_or_insert((why, now));
    }

    /// Whether the check is over, and why: on a stop already seen, out of time, or once the
    /// estimate has held for [`STEADY`]. A steady estimate counts only from half the budget
    /// on: the first estimate is the estimator's own starting guess, which the probe takes a
    /// moment to move.
    pub(crate) fn done(&mut self, now: Instant) -> Option<Stop> {
        if let Some((why, _)) = self.stopped {
            return Some(why);
        }
        let elapsed = now.duration_since(self.started);
        if elapsed >= self.budget {
            self.stop(Stop::Budget, now);
        } else if elapsed >= self.budget / 2
            && self.moved_at.is_some_and(|at| now.duration_since(at) >= STEADY)
        {
            self.stop(Stop::Steady, now);
        }
        self.stopped.map(|(why, _)| why)
    }

    pub(crate) fn elapsed(&self, now: Instant) -> Duration {
        let end = self.stopped.map_or(now, |(_, at)| at);
        end.duration_since(self.started)
    }

    pub(crate) fn budget(&self) -> Duration {
        self.budget
    }

    /// The latest estimate, kbit/s.
    pub(crate) fn estimate_kbps(&self) -> Option<u64> {
        self.estimate_kbps
    }
}

/// The video rate a stream starts at from a measured estimate, kbit/s: what `adapt.rs` would
/// leave for video from it (85%, less the audio), within the stream's floor and ceiling. Over
/// TCP to the relay it starts lower still: retransmissions there hide loss until the delay is
/// already long.
pub(crate) fn start_kbps(
    estimate_kbps: u64,
    audio_kbps: u32,
    limits: Limits,
    path: PathClass,
) -> u32 {
    let share = if path.over_tcp() { ESTIMATE_SHARE * RELAY_TCP_SHARE } else { ESTIMATE_SHARE };
    let usable = (estimate_kbps as f64 * share) as i64 - i64::from(audio_kbps);
    (usable.max(0) as u32).clamp(limits.min_kbps, limits.max_kbps)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIMITS: Limits = Limits { min_kbps: 300, start_kbps: 5000, max_kbps: 12_000 };

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn classes_a_path_by_where_the_client_comes_from() {
        // The relay hands its traffic to sessions over loopback: that is how a relayed client
        // is known without asking it.
        let of = |remote: &str| PathClass::of(remote.parse().unwrap(), None);
        assert_eq!(of("127.0.0.1"), PathClass::Relay);
        assert_eq!(of("::1"), PathClass::Relay);
        assert_eq!(of("192.168.2.79"), PathClass::Lan);
        assert_eq!(of("10.0.2.16"), PathClass::Lan);
        assert_eq!(of("fd12::5"), PathClass::Lan);
        assert_eq!(of("fe80::1c2b:3aff:fe4d:5e6f"), PathClass::Lan);
        assert_eq!(of("99.69.84.25"), PathClass::Direct);
        assert_eq!(of("2a02:8071:5e1:1::7"), PathClass::Direct);
        // Over IPv6 a player at home sends to the session's global address from one of the
        // same network: the same /64.
        let ours = Some("2a02:8071:5e1:1:2c3:4dff:fe5e:6f70".parse().unwrap());
        let from = |remote: &str| PathClass::of(remote.parse().unwrap(), ours);
        assert_eq!(from("2a02:8071:5e1:1::7"), PathClass::Lan);
        assert_eq!(from("2a02:8071:5e1:2::7"), PathClass::Direct, "another /64");
        assert_eq!(from("2600::7"), PathClass::Direct);
        assert_eq!(from("99.69.84.25"), PathClass::Direct);
    }

    #[test]
    fn only_the_client_tells_the_relays_transports_apart() {
        assert_eq!(PathClass::Relay.with_client(Some("relay-tcp")), PathClass::RelayTcp);
        assert_eq!(PathClass::Relay.with_client(Some("relay-udp")), PathClass::RelayUdp);
        assert_eq!(PathClass::Relay.with_client(None), PathClass::Relay);
        // A relayed client always knows it is: one on loopback that is not is on the server's
        // own machine.
        assert_eq!(PathClass::Relay.with_client(Some("direct")), PathClass::Lan);
        // A client that thinks it is relayed is not believed over what the worker saw: a
        // stale report from before ICE changed its pair says nothing of this one.
        assert_eq!(PathClass::Direct.with_client(Some("relay-tcp")), PathClass::Direct);
    }

    #[test]
    fn stops_at_the_ceiling_at_once() {
        let t = Instant::now();
        let mut probe = Probe::new(ms(2000), 12_128, t);
        probe.estimate(5_000_000, t + ms(50));
        assert_eq!(probe.done(t + ms(60)), None);
        probe.estimate(12_500_000, t + ms(300));
        assert_eq!(probe.done(t + ms(310)), Some(Stop::Ceiling));
        assert_eq!(probe.elapsed(t + ms(900)), ms(300), "timed to the stop, not to now");
    }

    #[test]
    fn stops_when_the_estimate_settles_but_not_on_the_estimators_first_guess() {
        let t = Instant::now();
        let mut probe = Probe::new(ms(2000), 12_128, t);
        // The starting guess, unmoved for 600 ms: too early to call it the link.
        probe.estimate(6_000_000, t);
        assert_eq!(probe.done(t + ms(600)), None, "steady only from half the budget");
        // It climbs, then holds.
        probe.estimate(8_000_000, t + ms(700));
        probe.estimate(8_100_000, t + ms(900));
        assert_eq!(probe.done(t + ms(1100)), None);
        assert_eq!(probe.done(t + ms(1250)), Some(Stop::Steady));
        assert_eq!(probe.estimate_kbps(), Some(8100));
    }

    #[test]
    fn keeps_the_latest_estimate_and_stops_on_trouble_or_time() {
        // Started high (the same network), the estimate falls to what the link carries: the
        // fall is the answer, not the guess it fell from.
        let t = Instant::now();
        let mut probe = Probe::new(ms(2000), 12_128, t);
        probe.estimate(10_900_000, t);
        probe.estimate(1_600_000, t + ms(400));
        assert_eq!(probe.estimate_kbps(), Some(1600));
        assert_eq!(probe.done(t + ms(1000)), Some(Stop::Steady), "held since 400 ms");
        let mut probe = Probe::new(ms(2000), 12_128, t);
        probe.estimate(9_000_000, t + ms(100));
        probe.trouble(t + ms(250));
        assert_eq!(probe.done(t + ms(260)), Some(Stop::Trouble));

        let mut idle = Probe::new(ms(2000), 12_128, t);
        assert_eq!(idle.done(t + ms(2000)), Some(Stop::Budget));
    }

    #[test]
    fn a_shorter_budget_for_the_same_network() {
        let t = Instant::now();
        let mut probe = Probe::new(ms(2000), 12_128, t);
        probe.shorten(ms(750));
        assert_eq!(probe.done(t + ms(750)), Some(Stop::Budget));
        assert!(!Probe::wanted(Duration::ZERO), "0 turns the check off");
    }

    #[test]
    fn starts_at_what_the_adaptation_would_leave_for_video() {
        // 1500 kbit/s measured with 128 for audio: 1147 for video, under the 1275 a limited
        // link of 1500 can take (#243's test).
        assert_eq!(start_kbps(1500, 128, LIMITS, PathClass::Direct), 1147);
        assert_eq!(start_kbps(40_000, 128, LIMITS, PathClass::Lan), 12_000, "the ceiling");
        assert_eq!(start_kbps(200, 128, LIMITS, PathClass::Direct), 300, "the floor");
        // Over TCP to the relay, well under the estimate.
        assert_eq!(start_kbps(10_000, 128, LIMITS, PathClass::RelayTcp), 4972);
    }
}
