//! The built-in TURN relay (`sovemu-relay`), when `SovEmu__Relay__Enabled` turns it on: started
//! with the server, and handed to each signed-in player in `clientConfig.iceServers` with
//! credentials of their own.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};
use axum::http::HeaderMap;

use crate::Config;

/// The running relay, and the key its credentials are derived from.
#[derive(Debug)]
pub(crate) struct RelayState {
    relay: sovemu_relay::Relay,
    secret: Vec<u8>,
    port: u16,
    host: Option<String>,
    lifetime: Duration,
}

impl RelayState {
    /// The port the relay was given (the configured one, unless that was 0).
    #[cfg(test)]
    pub(crate) fn port(&self) -> u16 {
        self.port
    }

    pub(crate) fn stats(&self) -> Arc<sovemu_relay::Stats> {
        self.relay.stats()
    }
}

/// Starts the relay, if it is turned on. Its key is generated on first run and kept in
/// `server.json`, like the server's other keys: nothing to configure.
pub(crate) async fn start(config: &Config) -> Result<Option<Arc<RelayState>>> {
    if !config.relay {
        return Ok(None);
    }
    let secret =
        sovemu_assets::load_secret(None, &config.data_dir, "relayKey", "relay credential key")?
            .into_bytes();
    let bound = SocketAddr::new(config.udp_bind, config.udp_ports.0);
    let peers = sovemu_relay::Peers {
        advertised: sovemu_protocol::net::advertised_addr(config.advertise_ip, bound).ip(),
        bind: config.udp_bind,
        ports: config.udp_ports,
    };
    let listen = SocketAddr::new(std::net::Ipv4Addr::UNSPECIFIED.into(), config.relay_port);
    let relay = sovemu_relay::Relay::start(sovemu_relay::Config {
        listen,
        peers,
        secret: secret.clone(),
        max_allocations: usize::try_from(config.relay_max_allocations).unwrap_or(usize::MAX),
    })
    .await
    .context("starting the relay (SovEmu__Relay__Port)")?;
    Ok(Some(Arc::new(RelayState {
        // The port it was given (the configured one, unless that was 0).
        port: relay.udp_addr().port(),
        relay,
        secret,
        host: config.relay_host.clone().filter(|h| !h.trim().is_empty()),
        lifetime: Duration::from_secs(config.relay_credential_hours * 3600),
    })))
}

/// The `RTCIceServer`s a player is given: the STUN servers (`SovEmu__Streaming__StunServers`),
/// then the built-in relay with their own credentials, when it runs.
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
/// reached the server (unless `SovEmu__Relay__Host` says where).
pub(crate) fn ice_server(
    relay: &RelayState,
    user: i64,
    headers: &HeaderMap,
    now: SystemTime,
) -> Option<serde_json::Value> {
    let host = match &relay.host {
        Some(h) => h.trim().to_owned(),
        None => request_host(headers)?,
    };
    let c = sovemu_relay::mint(&relay.secret, &user.to_string(), relay.lifetime, now);
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
