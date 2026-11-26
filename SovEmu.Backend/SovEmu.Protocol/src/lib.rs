// EXPERIMENT (#245): a change that forces a recompile; this branch is never merged.
//! Wire formats shared by the SovEmu worker, server and clients.
//!
//! Every client (web, Android, iOS) implements these byte-for-byte, so this crate is the
//! reference: change a format here and in every client in the same release, and bump
//! [`INPUT_VERSION`] if old clients would misread it.
//!
//! - [`input`]: the binary messages on the `input` WebRTC DataChannel.
//! - [`signal`]: the JSON session descriptions exchanged to set up the peer connection.
//! - [`ipc`]: the JSON lines the server and a session's worker exchange over stdin/stdout.
//! - [`net`]: the address clients send a session's media to.

pub mod input;
pub mod ipc;
pub mod net;
pub mod signal;

pub use input::{
    Analog, AnalogPacket, AudioPath, ClientMessage, ClientStats, DecodeError, Fallback, Hotkeys,
    InputAck, InputPacket, MAX_PORTS, Pipeline, VideoPath,
};
pub use signal::{SdpKind, SessionDescription};

/// The version byte of the input channel's messages.
pub const INPUT_VERSION: u8 = 1;

/// The label of the DataChannel carrying input and acks. Clients open it unordered with no
/// retransmits: each packet carries the whole controller state, so a lost one is corrected by
/// the next.
pub const INPUT_CHANNEL: &str = "input";
