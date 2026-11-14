//! Server ↔ worker messages: one JSON object per line on the worker's stdin and stdout.
//!
//! The server starts `sovemu-worker session …` per game session. Requests carry an `id` the
//! worker echoes in its reply, so several can be in flight. Events (`ready`, `client_*`) carry
//! none. The worker exits when its stdin closes, so a crashed server never leaves orphans.

use serde::{Deserialize, Serialize};

use crate::SessionDescription;

/// Server → worker.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    /// A client's SDP offer; answered with [`Reply::Answer`].
    Offer { id: u64, offer: SessionDescription },
    /// Write a save state to `slot` (and a screenshot beside it).
    SaveState { id: u64, slot: u32 },
    /// Restore the save state in `slot`.
    LoadState { id: u64, slot: u32 },
    /// Press the console's reset button.
    Reset { id: u64 },
    /// Replace the active cheats with these codes (format depends on the core: Game Genie,
    /// Pro Action Replay, …). Empty clears them.
    SetCheats { id: u64, codes: Vec<String> },
    /// Flush battery saves and exit.
    Shutdown { id: u64 },
}

impl Request {
    pub fn id(&self) -> u64 {
        match self {
            Self::Offer { id, .. }
            | Self::SaveState { id, .. }
            | Self::LoadState { id, .. }
            | Self::Reset { id }
            | Self::SetCheats { id, .. }
            | Self::Shutdown { id } => *id,
        }
    }
}

/// Worker → server.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Reply {
    /// The core is loaded and the worker is listening for WebRTC on `udp_port`.
    Ready {
        udp_port: u16,
        core: String,
        width: u32,
        height: u32,
        fps: f64,
    },
    /// The answer to [`Request::Offer`].
    Answer {
        id: u64,
        answer: SessionDescription,
    },
    /// A request succeeded with nothing else to say.
    Done {
        id: u64,
    },
    /// A request failed.
    Error {
        id: u64,
        message: String,
    },
    ClientConnected,
    ClientDisconnected,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SdpKind;

    #[test]
    fn json_shapes() {
        let offer = Request::Offer {
            id: 7,
            offer: SessionDescription { kind: SdpKind::Offer, sdp: "v=0".into() },
        };
        assert_eq!(
            serde_json::to_string(&offer).unwrap(),
            r#"{"type":"offer","id":7,"offer":{"type":"offer","sdp":"v=0"}}"#
        );
        assert_eq!(offer.id(), 7);
        let save: Request =
            serde_json::from_str(r#"{"type":"save_state","id":2,"slot":1}"#).unwrap();
        assert_eq!(save, Request::SaveState { id: 2, slot: 1 });
        assert_eq!(
            serde_json::to_string(&Reply::ClientConnected).unwrap(),
            r#"{"type":"client_connected"}"#
        );
        let ready: Reply = serde_json::from_str(
            r#"{"type":"ready","udp_port":50001,"core":"FCEUmm","width":256,"height":224,"fps":60.1}"#,
        )
        .unwrap();
        assert!(matches!(ready, Reply::Ready { udp_port: 50001, .. }));
    }
}
