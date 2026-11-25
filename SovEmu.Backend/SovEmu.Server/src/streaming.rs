//! The admin's Streaming page: the settings that shape every game's stream, and a connection
//! test from the admin's own browser to a game port (`sovemu_sessions::connection_test`), which
//! shows whether the UDP ports are reachable and by which path, before anyone tries to play.

use axum::http::StatusCode;
use sovemu_identity::User;
use sovemu_protocol::{SdpKind, SessionDescription};
use sovemu_sessions::Error;

use crate::AppState;
use crate::configuration::Setting;
use crate::error::{ApiError, ApiResult};

/// The settings that shape a game's stream: its bitrates, codec and encoder, keyframes,
/// fast-forward and rewind, and how long it waits for a player.
const STREAMING_SETTINGS: &[&str] = &[
    "SovEmu__Streaming__Video__Kbps",
    "SovEmu__Streaming__Video__MinKbps",
    "SovEmu__Streaming__Video__MaxKbps",
    "SovEmu__Streaming__Video__Codec",
    "SovEmu__Streaming__Video__Encoder",
    "SovEmu__Streaming__Audio__Kbps",
    "SovEmu__Streaming__Video__KeyframeInterval",
    "SovEmu__Streaming__Video__DropoutKeyframeMilliseconds",
    "SovEmu__Sessions__FastForward",
    "SovEmu__Sessions__Rewind__Seconds",
    "SovEmu__Sessions__Rewind__MaxMegabytes",
    "SovEmu__Sessions__IdleTimeoutSeconds",
    "SovEmu__Sessions__HealthSeconds",
];

/// The Streaming page's settings, as the Configuration page shows them.
pub(crate) fn settings(state: &AppState) -> Vec<Setting> {
    crate::configuration::settings_where(&state.config, |name| STREAMING_SETTINGS.contains(&name))
}

/// Answers the admin's browser's offer (SDP) with a connection-test worker on a free game port;
/// returns its answer (SDP).
pub(crate) async fn test_connection(
    state: &AppState,
    user: &User,
    sdp: String,
) -> ApiResult<String> {
    let offer = SessionDescription { kind: SdpKind::Offer, sdp };
    match state.workers.test_connection(offer).await {
        Ok(answer) => {
            tracing::info!(admin = %user.username, "connection test started");
            Ok(answer.sdp)
        }
        Err(e) => {
            tracing::warn!(admin = %user.username, error = %e, "connection test could not start");
            Err(match e {
                Error::NoPort(_) => ApiError::new(StatusCode::SERVICE_UNAVAILABLE, e.to_string()),
                Error::Worker(message) => ApiError::bad_request(message),
                e => ApiError::new(StatusCode::BAD_GATEWAY, e.to_string()),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_streaming_setting_exists() {
        let config = <crate::Config as clap::Parser>::parse_from(["sovemu-server"]);
        let names: Vec<String> =
            crate::configuration::settings(&config).into_iter().map(|s| s.name).collect();
        for name in super::STREAMING_SETTINGS {
            assert!(names.iter().any(|n| n == name), "{name} is not a setting");
        }
    }
}
