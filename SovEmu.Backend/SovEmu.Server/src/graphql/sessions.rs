//! Game sessions: starting one and following its start, the WebRTC offer, saves and states, discs, and what clients report.

use super::*;

/// Where a 3D system renders on this server.
/// What went wrong on a client (`reportClientEvents`).
#[derive(Enum, Clone, Copy, PartialEq, Eq)]
pub(super) enum ClientEventKind {
    /// WebRTC could not connect.
    ConnectionFailed,
    /// A connected stream dropped.
    ConnectionLost,
    /// No new picture for a while during play.
    StreamStalled,
    /// A controller the client has no layout for.
    ControllerUnknown,
    /// A screen or the app failed.
    ScreenError,
    Other,
}

/// One thing that went wrong on a client.
#[derive(InputObject)]
pub(super) struct ClientEventInput {
    pub(super) kind: ClientEventKind,
    /// What happened, in a line (kept to 500 characters).
    pub(super) message: String,
    /// The game session it happened in, if any.
    pub(super) session_id: Option<ID>,
    /// When, in seconds since the Unix epoch on the client's clock.
    pub(super) at: Option<f64>,
}

impl From<ClientEventInput> for crate::client_events::Event {
    fn from(e: ClientEventInput) -> Self {
        use crate::client_events::Kind;
        let kind = match e.kind {
            ClientEventKind::ConnectionFailed => Kind::ConnectionFailed,
            ClientEventKind::ConnectionLost => Kind::ConnectionLost,
            ClientEventKind::StreamStalled => Kind::StreamStalled,
            ClientEventKind::ControllerUnknown => Kind::ControllerUnknown,
            ClientEventKind::ScreenError => Kind::ScreenError,
            ClientEventKind::Other => Kind::Other,
        };
        Self { kind, message: e.message, session_id: e.session_id.map(|id| id.0), at: e.at }
    }
}

/// A running game: one worker streaming over WebRTC.
pub(super) struct SessionNode(pub(super) sovemu_sessions::workers::SessionInfo);

/// A game being played, as the admin's sessions page shows it.
pub(super) struct ActiveSessionNode(pub(super) crate::api::ActiveSession);

#[Object(name = "ActiveSession")]
impl ActiveSessionNode {
    async fn id(&self) -> ID {
        ID::from(&self.0.info.id)
    }
    async fn user_id(&self) -> i64 {
        self.0.user_id
    }
    /// The player; null if the account has been deleted since.
    async fn username(&self) -> Option<&str> {
        self.0.username.as_deref()
    }
    /// The app and device playing it, as the client named itself (null until it connects, or
    /// for a client that predates naming itself).
    async fn client(&self) -> Option<&str> {
        self.0.live.client.as_deref()
    }
    async fn game_id(&self) -> i64 {
        self.0.info.game_id
    }
    async fn title(&self) -> &str {
        &self.0.info.title
    }
    async fn system(&self) -> &str {
        &self.0.info.system
    }
    /// The core's name.
    async fn core(&self) -> &str {
        &self.0.info.core
    }
    /// When it started, as a Unix time in seconds.
    async fn started_at(&self) -> f64 {
        self.0.started_at
    }
    /// A client is streaming it (else it waits, for a client to come back or its idle timeout).
    async fn connected(&self) -> bool {
        self.0.info.connected
    }
    /// How the stream is made; null until the first frame is encoded.
    async fn pipeline(&self) -> Option<StreamPipeline> {
        self.0.live.pipeline.as_ref().map(StreamPipeline::from)
    }
    /// The worker's last health interval (every 10 s while a client is connected).
    async fn health(&self) -> Option<StreamHealth> {
        self.0.live.health.as_ref().map(StreamHealth::from)
    }
    /// The client's own view of the stream, as it last reported it (once a second while playing).
    async fn client_stats(&self) -> Option<ClientStreamStats> {
        self.0.live.client_stats.as_ref().map(|(s, _)| ClientStreamStats::from(s))
    }
}

/// How a session's stream is made: what draws the picture, the video codec and encoder (and
/// any encoder it fell back from), the audio, and run-ahead.
#[derive(SimpleObject)]
pub(super) struct StreamPipeline {
    /// The OpenGL renderer a 3D core draws with; null for a core that draws on the CPU.
    pub(super) renderer: Option<String>,
    /// The renderer is the software one (llvmpipe), not a GPU.
    pub(super) renderer_software: bool,
    /// `h264` or `vp8`.
    pub(super) video_codec: String,
    /// `vaapi`, `nvenc`, `x264` or `libvpx`.
    pub(super) video_encoder: String,
    /// The encoder runs on a GPU.
    pub(super) video_hardware: bool,
    /// Encoders given up before this one, in order, with why.
    pub(super) fallbacks: Vec<EncoderFallback>,
    pub(super) width: i32,
    pub(super) height: i32,
    pub(super) fps: f64,
    /// `opus`.
    pub(super) audio_codec: String,
    /// The core's own sample rate, resampled to 48 kHz.
    pub(super) audio_source_hz: f64,
    pub(super) audio_kbps: i32,
    /// Frames the picture runs ahead of the game (0: off).
    pub(super) run_ahead: i32,
}

impl From<&sovemu_protocol::Pipeline> for StreamPipeline {
    fn from(p: &sovemu_protocol::Pipeline) -> Self {
        Self {
            renderer: p.renderer.clone(),
            renderer_software: p.renderer_software,
            video_codec: p.video.codec.clone(),
            video_encoder: p.video.encoder.clone(),
            video_hardware: p.video.hardware,
            fallbacks: p
                .video
                .fallbacks
                .iter()
                .map(|f| EncoderFallback { encoder: f.encoder.clone(), reason: f.reason.clone() })
                .collect(),
            width: p.video.width as i32,
            height: p.video.height as i32,
            fps: p.video.fps,
            audio_codec: p.audio.codec.clone(),
            audio_source_hz: p.audio.source_hz,
            audio_kbps: p.audio.kbps as i32,
            run_ahead: p.run_ahead as i32,
        }
    }
}

/// An encoder a session gave up, and why.
#[derive(SimpleObject)]
pub(super) struct EncoderFallback {
    pub(super) encoder: String,
    pub(super) reason: String,
}

/// A session's last `session health` interval.
#[derive(SimpleObject)]
pub(super) struct StreamHealth {
    /// Frames shown per second.
    pub(super) fps: f64,
    /// The share of frames that took longer than a frame period, 0 to 1.
    pub(super) overran: f64,
    /// The share of the interval the game was paused (the player's menu open), 0 to 1.
    pub(super) paused: f64,
    pub(super) tick_mean_ms: f64,
    pub(super) encode_mean_ms: f64,
    /// Inside the core, per frame it ran (run-ahead's included).
    pub(super) core_mean_ms: f64,
    /// Reading a GPU frame back, per frame the core ran (0 for cores that draw on the CPU).
    pub(super) readback_mean_ms: f64,
    /// What the encoder produced, and what it aimed for, kbit/s.
    pub(super) video_kbps: f64,
    pub(super) video_kbps_target: i32,
    /// The send-bandwidth estimate, kbit/s.
    pub(super) estimate_kbps: Option<f64>,
}

impl From<&sovemu_protocol::ipc::SessionHealth> for StreamHealth {
    fn from(h: &sovemu_protocol::ipc::SessionHealth) -> Self {
        Self {
            fps: h.fps,
            overran: h.overran,
            paused: h.paused,
            tick_mean_ms: h.tick_mean_ms,
            encode_mean_ms: h.encode_mean_ms,
            core_mean_ms: h.core_mean_ms,
            readback_mean_ms: h.readback_mean_ms,
            video_kbps: h.video_kbps,
            video_kbps_target: h.video_kbps_target as i32,
            estimate_kbps: h.estimate_kbps.map(|k| k as f64),
        }
    }
}

/// What a player's stats panel shows (times in ms, rates in kbit/s); null where the client has
/// no value.
#[derive(SimpleObject)]
pub(super) struct ClientStreamStats {
    /// Button to pixel, as the client estimates it.
    pub(super) estimate_ms: Option<f64>,
    /// Button to frame, as the client measures it (the display's own lag comes on top).
    pub(super) input_to_frame_ms: Option<f64>,
    pub(super) input_rtt_ms: Option<f64>,
    pub(super) net_rtt_ms: Option<f64>,
    pub(super) jitter_ms: Option<f64>,
    pub(super) decode_ms: Option<f64>,
    pub(super) fps: Option<f64>,
    pub(super) kbps: Option<f64>,
    /// Packets lost since the stream began.
    pub(super) packets_lost: Option<f64>,
    /// The player's video decoder.
    pub(super) decoder: Option<String>,
    /// Whether that decoder runs in low-latency mode; null where the client cannot tell.
    pub(super) low_latency_decode: Option<bool>,
}

impl From<&sovemu_protocol::ClientStats> for ClientStreamStats {
    fn from(s: &sovemu_protocol::ClientStats) -> Self {
        Self {
            estimate_ms: s.estimate_ms,
            input_to_frame_ms: s.input_to_frame_ms,
            input_rtt_ms: s.input_rtt_p50_ms,
            net_rtt_ms: s.net_rtt_ms,
            jitter_ms: s.jitter_ms,
            decode_ms: s.decode_ms,
            fps: s.fps,
            kbps: s.kbps,
            packets_lost: s.packets_lost.map(|n| n as f64),
            decoder: s.decoder.clone(),
            low_latency_decode: s.low_latency_decode,
        }
    }
}

#[Object(name = "GameSession")]
impl SessionNode {
    async fn id(&self) -> ID {
        ID::from(&self.0.id)
    }
    async fn game_id(&self) -> i64 {
        self.0.game_id
    }
    async fn system(&self) -> &str {
        &self.0.system
    }
    async fn title(&self) -> &str {
        &self.0.title
    }
    /// The core's name.
    async fn core(&self) -> &str {
        &self.0.core
    }
    async fn width(&self) -> i32 {
        self.0.width as i32
    }
    async fn height(&self) -> i32 {
        self.0.height as i32
    }
    async fn fps(&self) -> f64 {
        self.0.fps
    }
    /// A client is streaming it.
    async fn connected(&self) -> bool {
        self.0.connected
    }
    /// How many discs the game has (1 for most games).
    async fn discs(&self) -> i32 {
        self.0.discs as i32
    }
    /// The disc in the tray, from 1.
    async fn disc(&self) -> i32 {
        self.0.disc as i32
    }
}

/// What a game start is doing.
#[derive(Enum, Clone, Copy, PartialEq, Eq)]
pub(super) enum StartPhase {
    /// Saving and stopping the player's previous session.
    Stopping,
    /// Starting the emulator's process.
    Launching,
    /// Opening the emulator.
    Core,
    /// Reading the game.
    Content,
    Ready,
    Failed,
}

/// How far a game start has got, for a loading bar.
#[derive(SimpleObject)]
pub(super) struct StartStatus {
    pub(super) phase: StartPhase,
    /// Bytes read so far in this phase, where known.
    pub(super) read: Option<f64>,
    /// Bytes this phase will read, when known.
    pub(super) total: Option<f64>,
    /// Since the start began.
    pub(super) elapsed_ms: f64,
    /// Since this phase began.
    pub(super) phase_ms: f64,
    pub(super) error: Option<String>,
}

impl From<sovemu_sessions::starts::StartStatus> for StartStatus {
    fn from(s: sovemu_sessions::starts::StartStatus) -> Self {
        use sovemu_sessions::starts::Phase;
        Self {
            phase: match s.phase {
                Phase::Stopping => StartPhase::Stopping,
                Phase::Launching => StartPhase::Launching,
                Phase::Core => StartPhase::Core,
                Phase::Content => StartPhase::Content,
                Phase::Ready => StartPhase::Ready,
                Phase::Failed => StartPhase::Failed,
            },
            read: s.read.map(|n| n as f64),
            total: s.total.map(|n| n as f64),
            elapsed_ms: s.elapsed_ms as f64,
            phase_ms: s.phase_ms as f64,
            error: s.error,
        }
    }
}

#[derive(Default)]
pub(super) struct SessionsQuery;

#[Object]
impl SessionsQuery {
    /// This user's running game, if any.
    async fn current_session(&self, ctx: &Context<'_>) -> Result<Option<SessionNode>> {
        let user = user(ctx).await?;
        Ok(crate::api::current_session(state(ctx), &user).map(SessionNode))
    }

    /// How far the start `startId` (given to `startSession`) has got. Poll it while
    /// `startSession` waits.
    async fn start_progress(&self, ctx: &Context<'_>, start_id: String) -> Result<StartStatus> {
        let user = user(ctx).await?;
        crate::api::start_progress(state(ctx), &user, &start_id).api().map(Into::into)
    }

    /// Admins: every game being played now, oldest first, with how each stream is made.
    async fn active_sessions(&self, ctx: &Context<'_>) -> Result<Vec<ActiveSessionNode>> {
        admin(ctx).await?;
        let sessions = crate::api::active_sessions(state(ctx)).await.api()?;
        Ok(sessions.into_iter().map(ActiveSessionNode).collect())
    }
}

#[derive(Default)]
pub(super) struct SessionsMutation;

#[Object]
impl SessionsMutation {
    /// Signed-in users: what went wrong on this client (a connection that failed, a stalled
    /// picture, a screen that crashed), for the server's log beside the session's own lines.
    /// `client` names it and its version (`android 0.1.0 (MiTV-AFMU1, Android 11)`). At most
    /// 20 events a call and 60 a minute per user; the rest are dropped. Returns how many were
    /// logged (0 when `SOVEMU_CLIENT_EVENTS` is off). Nothing is stored.
    async fn report_client_events(
        &self,
        ctx: &Context<'_>,
        client: String,
        events: Vec<ClientEventInput>,
    ) -> Result<i32> {
        let user = user(ctx).await?;
        let state = state(ctx);
        if !state.config.client_events {
            return Ok(0);
        }
        let events: Vec<crate::client_events::Event> = events.into_iter().map(Into::into).collect();
        let logged =
            state.client_events.report(user.id, &client, &events, std::time::Instant::now());
        Ok(logged as i32)
    }

    /// Starts a game (ending this user's running one first) and returns once it is ready to
    /// stream. Give a `startId` (8–64 letters, digits, `-` or `_`) to follow its progress with
    /// `startProgress` meanwhile.
    async fn start_session(
        &self,
        ctx: &Context<'_>,
        game_id: i64,
        start_id: Option<String>,
    ) -> Result<SessionNode> {
        let user = user(ctx).await?;
        crate::api::start_session(state(ctx), &user, game_id, start_id.as_deref())
            .await
            .api()
            .map(SessionNode)
    }

    /// Ends a session, the player's own or (admins) anyone's; the worker writes the battery save
    /// and the auto save first.
    async fn stop_session(&self, ctx: &Context<'_>, id: ID) -> Result<bool> {
        let user = user(ctx).await?;
        crate::api::stop_session(state(ctx), &user, &id).await.api()?;
        Ok(true)
    }

    /// Answers a client's WebRTC offer (SDP) for a session with the worker's answer (SDP).
    /// `client` names the app and device (`web`, `android 1.4.0 (Mi TV)`), as its stats do,
    /// for the admin's sessions page.
    async fn session_offer(
        &self,
        ctx: &Context<'_>,
        id: ID,
        sdp: String,
        client: Option<String>,
    ) -> Result<String> {
        let user = user(ctx).await?;
        crate::api::offer(state(ctx), &user, &id, sdp, client).await.api()
    }

    /// Presses the console's reset button.
    async fn reset_session(&self, ctx: &Context<'_>, id: ID) -> Result<bool> {
        let user = user(ctx).await?;
        crate::api::reset(state(ctx), &user, &id).await.api()?;
        Ok(true)
    }

    /// Changes a multi-disc game's disc (from 1), as when the game asks for the next one.
    async fn set_disc(&self, ctx: &Context<'_>, session_id: ID, disc: u32) -> Result<bool> {
        let user = user(ctx).await?;
        crate::api::set_disc(state(ctx), &user, &session_id, disc).await.api()?;
        Ok(true)
    }

    /// Saves the session's game to a slot (1–9), with a screenshot.
    async fn save_state(&self, ctx: &Context<'_>, session_id: ID, slot: u32) -> Result<bool> {
        let user = user(ctx).await?;
        crate::api::save_state(state(ctx), &user, &session_id, slot).await.api()?;
        Ok(true)
    }

    /// Loads a slot (0 is the auto save) into the session's game.
    async fn load_state(&self, ctx: &Context<'_>, session_id: ID, slot: u32) -> Result<bool> {
        let user = user(ctx).await?;
        crate::api::load_state(state(ctx), &user, &session_id, slot).await.api()?;
        Ok(true)
    }
}
