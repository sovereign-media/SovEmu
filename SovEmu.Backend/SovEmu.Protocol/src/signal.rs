//! Session descriptions, as JSON. The shape is the browser's own `RTCSessionDescriptionInit`
//! (`{"type": "offer", "sdp": "..."}`), so a web client can post `pc.localDescription` as is.
//!
//! Signalling is non-trickle: the client gathers all its ICE candidates before sending the
//! offer, and the answer carries all of the worker's. One request, one response.
//!
//! Phase 1: the client `POST`s its offer to the worker's `/api/offer` and receives the answer.
//! Phase 2 moves this behind the server, which relays it to the session's worker.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SdpKind {
    Offer,
    Answer,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionDescription {
    #[serde(rename = "type")]
    pub kind: SdpKind,
    pub sdp: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_browser_json_shape() {
        let json = r#"{"type":"offer","sdp":"v=0\r\n"}"#;
        let desc: SessionDescription = serde_json::from_str(json).unwrap();
        assert_eq!(desc, SessionDescription { kind: SdpKind::Offer, sdp: "v=0\r\n".into() });
        let answer = SessionDescription { kind: SdpKind::Answer, sdp: "x".into() };
        assert_eq!(serde_json::to_string(&answer).unwrap(), r#"{"type":"answer","sdp":"x"}"#);
    }
}
