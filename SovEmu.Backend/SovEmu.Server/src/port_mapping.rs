//! Asking the home router to forward the session ports and the relay's port to this server
//! (`sovemu-port-mapping`: UPnP or NAT-PMP), so players away from home reach it with nothing set
//! up on the router. On unless `SovEmu__Network__PortMapping` is off.
//!
//! What the router forwards to each session port goes to the workers ([`ForwardedPorts`]): a
//! session offers that public address beside the one STUN sees, which is what gets through when
//! STUN's doesn't (a router that gives each destination its own port) or when the router chose
//! another external port.

use std::sync::Arc;

use sovemu_port_mapping::{Discovery, PortMapper, Transport, Wanted};
use sovemu_sessions::workers::ForwardedPorts;

use crate::Config;

/// Starts asking, in the background (it never holds up startup or fails it). `relay_port` is the
/// built-in relay's, when it runs: forwarded over UDP and TCP, and only as the same port, since
/// players are told that port.
pub(crate) fn start(
    config: &Config,
    relay_port: Option<u16>,
    forwarded: ForwardedPorts,
) -> Option<Arc<PortMapper>> {
    if !config.port_mapping {
        tracing::info!(
            "not asking the router to forward ports: SovEmu__Network__PortMapping is off"
        );
        return None;
    }
    // Tests never ask the network's router for anything.
    if cfg!(test) {
        return None;
    }
    if config.udp_bind.is_loopback() {
        tracing::info!(bind = %config.udp_bind, "not asking the router to forward ports: sessions listen on loopback only");
        return None;
    }
    let ports = wanted(config.udp_ports, relay_port);
    let (first, last) = config.udp_ports;
    let mapper = PortMapper::start(
        sovemu_port_mapping::Config { ports, discovery: Discovery::default() },
        move |status| {
            forwarded.set(
                (first..=last)
                    .filter_map(|port| Some((port, status.public_addr(Transport::Udp, port)?)))
                    .collect(),
            );
        },
    );
    Some(Arc::new(mapper))
}

/// Every session port over UDP (any external port will do: the session offers what the router
/// gave), then the relay's over UDP and TCP (the same port, or none).
fn wanted(sessions: (u16, u16), relay_port: Option<u16>) -> Vec<Wanted> {
    let mut ports: Vec<Wanted> = (sessions.0..=sessions.1)
        .map(|port| Wanted { transport: Transport::Udp, port, same_port: false })
        .collect();
    if let Some(port) = relay_port {
        ports.extend([Transport::Udp, Transport::Tcp].map(|transport| Wanted {
            transport,
            port,
            same_port: true,
        }));
    }
    ports
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_session_ports_and_the_relay_s_are_wanted() {
        let ports = wanted((50000, 50002), Some(3478));
        let described: Vec<(u16, &str, bool)> =
            ports.iter().map(|w| (w.port, w.transport.name(), w.same_port)).collect();
        assert_eq!(
            described,
            [
                (50000, "udp", false),
                (50001, "udp", false),
                (50002, "udp", false),
                (3478, "udp", true),
                (3478, "tcp", true)
            ]
        );
        assert_eq!(wanted((50000, 50000), None).len(), 1, "no relay, no relay port");
    }

    #[test]
    fn off_and_in_tests_it_asks_nothing() {
        use clap::Parser;
        let off = Config::parse_from(["sovemu-server", "--port-mapping", "false"]);
        assert!(!off.port_mapping);
        assert!(start(&off, Some(3478), ForwardedPorts::default()).is_none());
        let on = Config::parse_from(["sovemu-server"]);
        assert!(on.port_mapping, "on by default");
        assert!(start(&on, Some(3478), ForwardedPorts::default()).is_none(), "never in tests");
    }
}
