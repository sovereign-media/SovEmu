//! SovEmu's built-in TURN relay, for the players no direct path reaches: behind a symmetric or
//! carrier-grade NAT, or on a network that blocks UDP.
//!
//! It runs inside `sovemu-server`, off unless `SovEmu__Relay__Enabled` turns it on, and listens
//! on one port over UDP and TCP (3478 by default). [`turn-server-proto`] speaks the protocol; this
//! crate is its sockets and its limits:
//!
//! - **Credentials are minted, never configured** ([`mint`]): the server hands each signed-in
//!   player short-lived ones in `clientConfig.iceServers`, derived from a key only it holds. The
//!   relay works the password out from the username, so it keeps no table, and refuses a username
//!   whose expiry has passed.
//! - **It relays only to this server's game sessions** ([`Peers`]): the advertised address on the
//!   session ports, reached over loopback. Its relay sockets listen only there, so nothing else
//!   on the network can be reached through it, and it is no use as an open proxy.
//! - **UDP allocations only.** Clients may reach it over TCP; what it relays is UDP (TURN-TCP,
//!   RFC 6062, is refused).
//! - **Allocations are capped** ([`Config::max_allocations`]).
//!
//! [`turn-server-proto`]: https://docs.rs/turn-server-proto

mod credentials;
mod peers;
mod relay;

pub use credentials::{Credentials, mint};
pub use peers::Peers;
pub use relay::{Relay, Stats};

use std::net::SocketAddr;

/// The realm the relay asks clients to sign in to.
pub const REALM: &str = "sovemu";

/// How the relay runs.
#[derive(Clone)]
pub struct Config {
    /// The address it listens on, over UDP and TCP alike.
    pub listen: SocketAddr,
    /// Where it may relay to.
    pub peers: Peers,
    /// The key credentials are derived from (see [`mint`]).
    pub secret: Vec<u8>,
    /// Allocations at once, across every player and transport.
    pub max_allocations: usize,
}

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("listen", &self.listen)
            .field("peers", &self.peers)
            .field("secret", &"<redacted>")
            .field("max_allocations", &self.max_allocations)
            .finish()
    }
}

/// What can stop the relay from starting.
#[derive(Debug)]
pub enum Error {
    /// Its port could not be opened (in use, or not allowed).
    Bind { addr: SocketAddr, transport: &'static str, source: std::io::Error },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bind { addr, transport, .. } => {
                write!(f, "opening the relay on {addr}/{transport}")
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Bind { source, .. } => Some(source),
        }
    }
}
