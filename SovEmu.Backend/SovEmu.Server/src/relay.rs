//! The built-in TURN relay (`sovemu-relay`): always on, with nothing to configure. It is started
//! with the server and handed to each signed-in player in `clientConfig.iceServers` with
//! credentials of their own, at the host the player reached the server at.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use anyhow::Result;
use axum::http::HeaderMap;

use crate::Config;

/// The relay's port, over UDP and TCP: TURN's own, so it is the one to forward. Tests take any
/// free port, as they run side by side.
const PORT: u16 = if cfg!(test) { 0 } else { 3478 };
/// How long a player's credentials last (rounded up to the hour). A player's client asks for
/// new ones each time it starts a game.
const LIFETIME: Duration = Duration::from_secs(24 * 3600);
/// Allocations at once for each session port: a relayed player holds one or two, so this
/// leaves room for a client reconnecting before its old allocations expire.
const ALLOCATIONS_PER_SESSION: usize = 4;

/// The running relay, and the key its credentials are derived from.
#[derive(Debug)]
pub(crate) struct RelayState {
    relay: sovemu_relay::Relay,
    secret: Vec<u8>,
    port: u16,
}

impl RelayState {
    /// The port the relay was given.
    #[cfg(test)]
    pub(crate) fn port(&self) -> u16 {
        self.port
    }

    pub(crate) fn stats(&self) -> Arc<sovemu_relay::Stats> {
        self.relay.stats()
    }
}

/// Starts the relay. Its key is generated on first run and kept in `server.json`, like the
/// server's other keys. When its port can't be opened (another TURN server has it), the server
/// runs without it, and says so: players away from home then need a direct path.
pub(crate) async fn start(config: &Config) -> Result<Option<Arc<RelayState>>> {
    let secret =
        sovemu_assets::load_secret(None, &config.data_dir, "relayKey", "relay credential key")?
            .into_bytes();
    let bound = SocketAddr::new(config.udp_bind, config.udp_ports.0);
    let peers = sovemu_relay::Peers {
        advertised: sovemu_protocol::net::advertised_addr(config.advertise_ip, bound).ip(),
        bind: config.udp_bind,
        ports: config.udp_ports,
    };
    let sessions = usize::from(config.udp_ports.1 - config.udp_ports.0) + 1;
    let started = sovemu_relay::Relay::start(sovemu_relay::Config {
        listen: SocketAddr::new(std::net::Ipv4Addr::UNSPECIFIED.into(), PORT),
        peers,
        secret: secret.clone(),
        max_allocations: sessions * ALLOCATIONS_PER_SESSION,
    })
    .await;
    match started {
        Ok(relay) => {
            Ok(Some(Arc::new(RelayState { port: relay.udp_addr().port(), relay, secret })))
        }
        Err(e) => {
            tracing::warn!(
                error = %e,
                port = PORT,
                "the built-in relay could not start: players no direct path reaches can't play"
            );
            Ok(None)
        }
    }
}

/// The `RTCIceServer`s a player is given: the STUN servers, then the built-in relay with their
/// own credentials, when it runs.
pub(crate) fn ice_servers(
    config: &Config,
    relay: Option<&RelayState>,
    user: i64,
    headers: &HeaderMap,
    now: SystemTime,
) -> Vec<serde_json::Value> {
    let mut servers = Vec::new();
    let stun = config.stun_urls();
    if !stun.is_empty() {
        servers.push(serde_json::json!({ "urls": stun }));
    }
    servers.extend(relay.and_then(|r| ice_server(r, user, headers, now)));
    servers
}

/// The relay as an `RTCIceServer` for `user`, over UDP and TCP, reached where the request
/// reached the server.
pub(crate) fn ice_server(
    relay: &RelayState,
    user: i64,
    headers: &HeaderMap,
    now: SystemTime,
) -> Option<serde_json::Value> {
    let host = request_host(headers)?;
    let c = sovemu_relay::mint(&relay.secret, &user.to_string(), LIFETIME, now);
    let port = relay.port;
    Some(serde_json::json!({
        "urls": [
            format!("turn:{host}:{port}?transport=udp"),
            format!("turn:{host}:{port}?transport=tcp"),
        ],
        "username": c.username,
        "credential": c.password,
    }))
}

/// The host (without its port) of the request's `Host` header. An IPv6 address keeps its
/// brackets, as a URL needs them.
fn request_host(headers: &HeaderMap) -> Option<String> {
    let host = headers.get(axum::http::header::HOST)?.to_str().ok()?;
    let authority: axum::http::uri::Authority = host.parse().ok()?;
    Some(authority.host().to_owned()).filter(|h| !h.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(host: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(axum::http::header::HOST, host.parse().unwrap());
        h
    }

    #[test]
    fn the_host_is_the_requests_without_its_port() {
        assert_eq!(
            request_host(&headers("games.example.com")).as_deref(),
            Some("games.example.com")
        );
        assert_eq!(request_host(&headers("192.168.1.10:8080")).as_deref(), Some("192.168.1.10"));
        assert_eq!(request_host(&headers("[2001:db8::1]:8080")).as_deref(), Some("[2001:db8::1]"));
        assert_eq!(request_host(&HeaderMap::new()), None);
    }
}
