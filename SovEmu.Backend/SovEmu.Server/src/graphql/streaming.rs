//! Streaming: the settings that shape every game's stream, and the admin's connection test.

use super::server::ConfigSetting;
use super::*;

/// What one frame cost an encoder at one picture size, in the startup encoder test.
#[derive(SimpleObject)]
pub(super) struct EncoderTiming {
    pub(super) width: i32,
    pub(super) height: i32,
    /// Frames timed.
    pub(super) frames: i32,
    /// Milliseconds a frame, on average: colour conversion, any upload to a GPU, and the encode.
    pub(super) mean_ms: f64,
    /// Milliseconds a frame at the 95th percentile.
    pub(super) p95_ms: f64,
}

/// A video encoder the startup test ran, exactly as a game's session would run it.
#[derive(SimpleObject)]
pub(super) struct TestedEncoder {
    /// `vp8` or `h264`.
    pub(super) codec: String,
    /// `libvpx`, `vaapi`, `nvenc` or `x264`.
    pub(super) encoder: String,
    /// It encodes on a GPU.
    pub(super) hardware: bool,
    /// CPU threads a frame is encoded on; null for a GPU's encoder.
    pub(super) threads: Option<i32>,
    /// It encoded every frame it was given, as it was given it, within its share of the time.
    pub(super) works: bool,
    /// Why it does not work: it did not open, failed, held frames back, or ran out of time.
    pub(super) error: Option<String>,
    /// A row for each picture size it finished, smallest first.
    pub(super) timings: Vec<EncoderTiming>,
}

/// The startup encoder test: every video encoder run on a moving picture and timed, once,
/// before the server takes requests, so no game starts without it.
#[derive(SimpleObject)]
pub(super) struct EncoderTest {
    pub(super) encoders: Vec<TestedEncoder>,
    /// How long the whole test took, and the longest it may (10 seconds).
    pub(super) took_ms: i32,
    pub(super) budget_ms: i32,
    /// Why the test gave no answer (the worker could not be run, or did not answer in time);
    /// null when it did.
    pub(super) note: Option<String>,
    /// When it ran (Unix seconds).
    pub(super) tested_at: Option<f64>,
}

impl From<&sovemu_sessions::encoders::EncoderTest> for EncoderTest {
    fn from(test: &sovemu_sessions::encoders::EncoderTest) -> Self {
        let encoders = test
            .encoders
            .iter()
            .map(|e| TestedEncoder {
                codec: e.codec.clone(),
                encoder: e.encoder.clone(),
                hardware: e.hardware,
                threads: e.threads.map(|t| t as i32),
                works: e.works,
                error: e.error.clone(),
                timings: e
                    .timings
                    .iter()
                    .map(|t| EncoderTiming {
                        width: t.width as i32,
                        height: t.height as i32,
                        frames: t.frames as i32,
                        mean_ms: t.mean_ms,
                        p95_ms: t.p95_ms,
                    })
                    .collect(),
            })
            .collect();
        Self {
            encoders,
            took_ms: test.took_ms.min(i32::MAX as u64) as i32,
            budget_ms: sovemu_sessions::encoders::BUDGET.as_millis() as i32,
            note: test.note.clone(),
            tested_at: test.tested_at.and_then(|at| {
                at.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs_f64())
            }),
        }
    }
}

/// What the Streaming page shows.
#[derive(SimpleObject)]
pub(super) struct StreamingReport {
    /// The settings that shape a game's stream, as the Configuration page shows them.
    pub(super) settings: Vec<ConfigSetting>,
    /// How long a connection test stays up, connected or not.
    pub(super) test_seconds: i32,
    /// What the startup encoder test found on this host.
    pub(super) encoder_test: EncoderTest,
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
            encoder_test: state(ctx).encoders.as_ref().into(),
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
