//! Streaming: the settings that shape every game's stream, and the admin's connection test.

use super::server::ConfigSetting;
use super::*;

/// What the Streaming page shows.
#[derive(SimpleObject)]
pub(super) struct StreamingReport {
    /// The settings that shape a game's stream, as the Configuration page shows them.
    pub(super) settings: Vec<ConfigSetting>,
    /// How long a connection test stays up, connected or not.
    pub(super) test_seconds: i32,
}

#[derive(Default)]
pub(super) struct StreamingQuery;

#[Object]
impl StreamingQuery {
    /// Admins: the settings that shape every game's stream (bitrates, codec and encoder,
    /// keyframes, fast-forward, rewind).
    async fn streaming(&self, ctx: &Context<'_>) -> Result<StreamingReport> {
        admin(ctx).await?;
        Ok(StreamingReport {
            settings: crate::streaming::settings(state(ctx)).into_iter().map(Into::into).collect(),
            test_seconds: sovemu_sessions::connection_test::TEST_SECONDS as i32,
        })
    }
}

#[derive(Default)]
pub(super) struct StreamingMutation;

#[Object]
impl StreamingMutation {
    /// Admins: tests this browser's connection to the game streams. Give an offer (SDP) with a
    /// data channel; the answer (SDP) comes from a peer on a free game port, answered as a game
    /// is, which echoes whatever the data channel carries for `testSeconds`. The browser reads
    /// the round trip and the path (direct, or through a relay) from its own end.
    async fn test_connection(&self, ctx: &Context<'_>, sdp: String) -> Result<String> {
        let user = admin(ctx).await?;
        crate::streaming::test_connection(state(ctx), &user, sdp).await.api()
    }
}
