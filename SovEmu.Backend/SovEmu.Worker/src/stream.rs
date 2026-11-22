//! The streaming loop shared by `serve` (standalone) and `session` (driven by the server).
//!
//! Everything media-related runs on one thread: the core, the encoders and the str0m peer
//! (sans-IO) sharing one UDP socket. Each turn of the loop takes any pending command (a client's
//! offer, a control request), drives the peer until it has nothing to send, waits on the socket
//! until whichever comes first of the peer's next timer and the next frame's deadline, then runs
//! a frame if one is due. Emulation runs only while a client is connected, so the game pauses
//! when nobody is watching.

use std::io::ErrorKind;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use crate::loading::{self, Report};
use crate::sysdir::SystemDir;
use anyhow::{Context, Result, anyhow};
use sovemu_media::{AudioEncoder, H264Preference};
use sovemu_protocol::ipc::{CoreCategory, CoreOption, LoadPhase};
use sovemu_protocol::ipc::{ExitReason, Request};
use sovemu_protocol::{
    ClientMessage, ClientStats, Hotkeys, INPUT_CHANNEL, InputAck, SdpKind, SessionDescription,
};
use sovemu_retro_host::{AnalogSticks, AvInfo, Core, CoreOptions, JoypadButtons};
use str0m::bwe::{Bitrate, BweKind};
use str0m::change::SdpOffer;
use str0m::channel::ChannelId;
use str0m::format::Codec;
use str0m::media::{Frequency, MediaKind, MediaTime, Mid, Pt};
use str0m::net::{Protocol, Receive};
use str0m::rtp::Extension;
use str0m::{Candidate, Event, IceConnectionState, Input, Output, Rtc, RtcConfig};

use crate::adapt::{Adaptation, ClientSample, LimitChange, Limits};
use crate::health::{self, Health};
use crate::pacing::Pacer;
use crate::persist::{Persist, Screenshot};
use crate::rewind::Rewind;
use crate::runahead::RunAhead;
use crate::video::{CodecSetting, Video, VideoCodec, VideoSettings};
use crate::watchdog;
use sovemu_protocol::ipc::SessionHealth;
use sovemu_protocol::{AudioPath, Pipeline, VideoPath};

/// Held input lapses if the client stops refreshing it: hotkeys, buttons and off-centre sticks.
/// Every client resends what is held at least every 100 ms, so a release that never arrives (the
/// input channel stalled or closed while the video kept going, the app lost its window) lets go
/// within half a second instead of holding the button for the rest of the session.
const INPUT_LAPSE: Duration = Duration::from_millis(500);

/// Whether held input has gone unrefreshed for [`INPUT_LAPSE`]: buttons held (`held`) and last
/// sent at `buttons_at`, or sticks off-centre and last sent at `analog_at`.
fn input_lapsed(held: bool, buttons_at: Instant, analog_at: Option<Instant>, now: Instant) -> bool {
    let stale = |at: Instant| now.duration_since(at) >= INPUT_LAPSE;
    (held && stale(buttons_at)) || analog_at.is_some_and(stale)
}

/// The payload types offered for H.264 and its retransmissions (str0m answers with the
/// client's own numbers).
const H264_PT: u8 = 108;
const H264_RTX_PT: u8 = 109;
/// `42e01f`: Constrained Baseline, level 3.1.
const H264_PROFILE_LEVEL_ID: u32 = 0x42e01f;

/// The RTP header extension id offered for playout delay: one str0m's standard map leaves free.
const PLAYOUT_DELAY_EXT_ID: u8 = 5;

/// How long the loop sleeps at most when idle, so new commands are picked up promptly.
const IDLE_WAIT: Duration = Duration::from_millis(20);

/// Work for the streaming loop from its front end (HTTP in `serve`, stdin in `session`).
#[derive(Debug)]
pub(crate) enum Command {
    /// A client's offer; the new client replaces any current one.
    Offer { offer: SessionDescription, reply: Sender<Result<SessionDescription, String>> },
    /// A control request: reset, save or load a state, shut down.
    Control { request: Request, reply: Sender<Result<(), String>> },
}

/// Something the front end may want to report.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum StreamEvent {
    ClientConnected,
    ClientDisconnected,
    /// How the stream is made now (sent when it changes).
    Pipeline(Pipeline),
    /// The last health interval's numbers.
    Health(SessionHealth),
    /// What the client's stats panel shows.
    ClientStats(ClientStats),
}

/// How to start a session.
#[derive(Debug, Clone)]
pub(crate) struct SessionConfig {
    pub core: PathBuf,
    pub rom: Option<PathBuf>,
    /// The system's BIOS folder.
    pub system_dir: PathBuf,
    /// Where its files belong in the core's system directory (see [`SystemDir`]).
    pub bios_path: String,
    pub save_dir: PathBuf,
    pub variables: Vec<(String, String)>,
    /// The video bitrate a stream starts at, and the floor and ceiling it adapts between.
    pub video_kbps: u32,
    pub video_min_kbps: u32,
    pub video_max_kbps: u32,
    pub audio_kbps: u32,
    /// Where save states go (`slotN.state` + `slotN.png`); none disables them.
    pub state_dir: Option<PathBuf>,
    pub rewind_seconds: u32,
    pub rewind_max_mb: u32,
    pub fast_forward: u32,
    /// Frames the picture runs ahead of the game (0: off).
    pub run_ahead: u32,
    /// Seconds between `session health` log lines while a client is connected (0: none).
    pub health_interval: u32,
    pub keyframe_interval: u32,
    /// The GPU for OpenGL cores; `None` refuses them. Its render node also encodes H.264.
    pub gpu: Option<sovemu_retro_host::GpuChoice>,
    pub video_codec: CodecSetting,
    pub video_encoder: H264Preference,
}

/// The connected (or connecting) client.
struct Peer {
    rtc: Rtc,
    video: Option<(Mid, Pt)>,
    audio: Option<(Mid, Pt)>,
    input: Option<ChannelId>,
    connected: bool,
}

/// The emulation and encoding state, kept across clients.
pub(crate) struct Session {
    core: Core,
    fps: f64,
    video: Video,
    audio: AudioEncoder,
    rgba: Vec<u8>,
    frame: u64,
    audio_samples: u64,
    buttons: Vec<JoypadButtons>,
    last_seq: Option<u32>,
    last_analog_seq: Option<u32>,
    pending_ack: Option<(u32, u32)>,
    persist: Persist,
    rewind: Rewind,
    /// Frames emulated per tick while fast-forward is held.
    fast_forward: u32,
    run_ahead: RunAhead,
    health: Health,
    /// The last send-bandwidth estimate, for the health lines.
    estimate_kbps: Option<u64>,
    /// When the client's last stats were taken (at most every [`STATS_SPACING`], however
    /// often a client sends them), and when they were last logged.
    stats_at: Option<Instant>,
    stats_logged_at: Option<Instant>,
    /// How often they are logged; `None`: never (as the health lines).
    stats_log_interval: Option<Duration>,
    /// Video frames sent to clients.
    frames_sent: u64,
    /// The core has drawn a picture: it is up, and can take a state.
    drawn: bool,
    /// A state asked for before that, loaded once it has (its slot and bytes).
    pending_load: Option<(u32, Vec<u8>)>,
    /// Events for the front end, handed over by the loop after each turn.
    events: Vec<StreamEvent>,
    /// The pipeline last reported, and when it last went to the client (`None`: it must go
    /// now, as when the channel has just opened).
    pipeline: Option<Pipeline>,
    pipeline_to_client_at: Option<Instant>,
    /// The video bitrate, through the stream.
    adaptation: Adaptation,
    audio_kbps: u32,
    hotkeys: Hotkeys,
    hotkeys_at: Instant,
    /// When `buttons` last arrived, and when the sticks last did while off-centre (`None`:
    /// centred, nothing to lapse).
    buttons_at: Instant,
    analog_at: Option<Instant>,
    /// Frames sent to the client, for RTP timestamps: wall-clock paced even when fast-forward
    /// runs several emulated frames per tick.
    ticks: u64,
    /// Size of the frame in `rgba`, for save-state screenshots.
    shot_size: (u32, u32),
    finished: bool,
    /// The core's system directory (a merge it may be reading from); dropped last, after the
    /// core.
    _system_dir: SystemDir,
}

impl Session {
    /// Loads the core and content and sets up the encoders.
    pub(crate) fn open(config: &SessionConfig, report: Report<'_>) -> Result<Self> {
        // Most systems need no BIOS, so a missing folder is no reason to refuse the game.
        if let Err(e) = std::fs::create_dir_all(&config.system_dir) {
            tracing::debug!(path = %config.system_dir.display(), error = %e, "cannot create the system folder");
        }
        std::fs::create_dir_all(&config.save_dir)?;
        let bios = config
            .system_dir
            .canonicalize()
            .or_else(|_| std::path::absolute(&config.system_dir))?;
        let system_dir = SystemDir::for_core(&config.core, &bios, &config.bios_path)?;
        let options = CoreOptions {
            system_dir: system_dir.path().to_owned(),
            save_dir: config.save_dir.canonicalize()?,
            gpu: config.gpu.clone(),
            variables: config.variables.clone(),
        };
        let mut core = loading::reporting(LoadPhase::Core, None, report, || {
            Core::load(&config.core, options)
                .with_context(|| format!("loading core {}", config.core.display()))
        })?;
        // SovEmu reads the whole file when the core takes content from memory: then the total
        // is known. A core that opens the file itself reads what it needs, an unknown amount.
        let total = match &config.rom {
            Some(rom) if !core.system_info().need_fullpath => {
                std::fs::metadata(rom).ok().map(|m| m.len())
            }
            _ => None,
        };
        let av = loading::reporting(LoadPhase::Content, total, report, || {
            core.load_game(config.rom.as_deref()).copied()
        })
        .context("loading content")?;
        let save_name =
            config.rom.as_deref().and_then(|r| r.file_stem()).map(|s| s.to_string_lossy());
        let mut persist =
            Persist::new(&config.save_dir, save_name.as_deref(), config.state_dir.clone());
        persist.restore(&mut core);
        tracing::info!(
            core.name = %core.system_info().library_name,
            core.version = %core.system_info().library_version,
            width = av.base_width,
            height = av.base_height,
            fps = av.fps,
            sample_rate = av.sample_rate,
            "game loaded"
        );
        let limits = Limits::new(config.video_min_kbps, config.video_kbps, config.video_max_kbps);
        Ok(Self {
            _system_dir: system_dir,
            video: Video::new(VideoSettings {
                codec: config.video_codec,
                encoder: config.video_encoder,
                device: match &config.gpu {
                    Some(sovemu_retro_host::GpuChoice::Device(node)) => Some(node.clone()),
                    _ => None,
                },
                bitrate_kbps: limits.start_kbps,
                fps: av.fps.round() as u32,
                keyframe_interval: config.keyframe_interval.max(1),
            }),
            audio: AudioEncoder::new(av.sample_rate, config.audio_kbps * 1000)?,
            fps: av.fps,
            core,
            rgba: Vec::new(),
            frame: 0,
            audio_samples: 0,
            buttons: vec![JoypadButtons::NONE; 2],
            last_seq: None,
            pending_ack: None,
            persist,
            // One snapshot every INTERVAL frames, so seconds × fps / INTERVAL of them.
            rewind: Rewind::new(
                config.rewind_max_mb as usize * 1024 * 1024,
                (f64::from(config.rewind_seconds) * av.fps / f64::from(crate::rewind::INTERVAL))
                    as usize,
                av.fps,
            ),
            fast_forward: config.fast_forward.max(1),
            run_ahead: RunAhead::new(config.run_ahead, av.fps),
            health: Health::new(config.health_interval, av.fps),
            estimate_kbps: None,
            stats_at: None,
            stats_logged_at: None,
            stats_log_interval: (config.health_interval > 0)
                .then(|| Duration::from_secs(u64::from(config.health_interval))),
            frames_sent: 0,
            drawn: false,
            pending_load: None,
            events: Vec::new(),
            pipeline: None,
            pipeline_to_client_at: None,
            adaptation: Adaptation::new(limits, config.audio_kbps),
            audio_kbps: config.audio_kbps,
            hotkeys: Hotkeys::default(),
            last_analog_seq: None,
            hotkeys_at: Instant::now(),
            buttons_at: Instant::now(),
            analog_at: None,
            ticks: 0,
            shot_size: (0, 0),
            finished: false,
        })
    }

    /// Saves the battery RAM and the auto state, once.
    pub(crate) fn finish(&mut self) {
        if std::mem::replace(&mut self.finished, true) {
            return;
        }
        let at = self.health_now();
        self.health.summary(Instant::now(), &at);
        let (width, height) = self.shot_size;
        let shot = Screenshot { rgba: &self.rgba, width, height };
        self.persist.finish(&mut self.core, self.frame > 0, Some(shot));
    }

    /// What the health lines report beside their counts.
    fn health_now(&self) -> health::Now {
        health::Now {
            video_kbps_target: self.video.bitrate_kbps(),
            video_encoder: self.video.encoder_name(),
            estimate_kbps: self.estimate_kbps,
            client_limit_kbps: self.adaptation.client_limit_kbps(),
            run_ahead: self.run_ahead.frames(),
        }
    }

    /// The options the core has declared now that its game is loaded.
    pub(crate) fn options(&self) -> (Vec<CoreOption>, Vec<CoreCategory>) {
        declared_options(&self.core)
    }

    /// The core's name and the content's geometry and timing.
    pub(crate) fn describe(&self) -> (String, AvInfo) {
        let av = *self.core.av_info().expect("a game is loaded");
        (self.core.system_info().library_name.clone(), av)
    }

    /// A multi-disc game's disc count; `None` for one disc or a core without disk control.
    pub(crate) fn discs(&mut self) -> Option<u32> {
        self.core.discs().map(|d| d.count)
    }

    /// Handles a control request. Returns `Ok(true)` if the loop should stop.
    fn control(&mut self, request: &Request) -> Result<bool, String> {
        match request {
            Request::Reset { .. } => {
                self.core.reset();
                self.rewind.clear();
                Ok(false)
            }
            Request::SetDisc { disc, .. } => {
                self.core.set_disc(*disc).map_err(|e| e.to_string())?;
                // The game reads the new disc from here on: rewinding across the swap would
                // restore states from the old one.
                self.rewind.clear();
                tracing::info!(disc = disc + 1, "changed disc");
                Ok(false)
            }
            Request::SetCheats { codes, .. } => {
                self.core.reset_cheats();
                for (i, code) in codes.iter().enumerate() {
                    self.core.set_cheat(i as u32, true, code);
                }
                tracing::info!(count = codes.len(), "cheats set");
                Ok(false)
            }
            Request::Shutdown { .. } => {
                self.finish();
                Ok(true)
            }
            Request::SaveState { slot, .. } => {
                let (width, height) = self.shot_size;
                let shot = Screenshot { rgba: &self.rgba, width, height };
                self.persist
                    .save_state(&mut self.core, *slot, Some(shot))
                    .map(|_| false)
                    .map_err(|e| format!("{e:#}"))
            }
            Request::LoadState { slot, .. } => {
                let bytes = self.persist.read_state(*slot).map_err(|e| format!("{e:#}"))?;
                if self.drawn {
                    Persist::apply_state(&mut self.core, *slot, &bytes)
                        .map_err(|e| format!("{e:#}"))?;
                    self.rewind.clear();
                    self.video.request_keyframe();
                } else {
                    // Continuing from a save: the client asks before the game has run at all.
                    // Some cores cannot take a state until they are up (PCSX2 showed nothing
                    // ever after one), so it waits for the first picture (`step`).
                    tracing::info!(slot = *slot, "state held until the core has drawn a frame");
                    self.pending_load = Some((*slot, bytes));
                }
                Ok(false)
            }
            Request::Offer { .. } => Err("offers are not control requests".into()),
        }
    }
}

/// The one address clients send media to: `--advertise-ip` if given, else the bound address,
/// else (for a wildcard bind) the machine's primary address.
///
/// Exactly one, because str0m's ICE answers a connectivity check only when the packet's
/// destination is one of its local candidates, and with a wildcard bind the OS does not tell us
/// which local address a datagram arrived on. So every received datagram is tagged with this
/// address. In a container that is the host's address (`SOVEMU_ADVERTISE_IP`), which the
/// container could not discover on its own.
pub(crate) fn advertised_addr(advertise: Option<IpAddr>, bound: SocketAddr) -> SocketAddr {
    let ip = advertise
        .or_else(|| (!bound.ip().is_unspecified()).then(|| bound.ip()))
        .or_else(primary_ip)
        .unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST));
    SocketAddr::new(ip, bound.port())
}

/// The address the OS would use for outbound traffic. Connecting a UDP socket sends nothing;
/// 192.0.2.1 is TEST-NET-1, reserved for documentation, so no real host is involved.
fn primary_ip() -> Option<IpAddr> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect((Ipv4Addr::new(192, 0, 2, 1), 9)).ok()?;
    let ip = socket.local_addr().ok()?.ip();
    (!ip.is_unspecified()).then_some(ip)
}

/// Builds a peer for a client's offer and returns it with the SDP answer.
fn accept_offer(
    offer: &SessionDescription,
    advertised: SocketAddr,
    video: Limits,
    codecs: CodecSetting,
) -> Result<(Peer, SessionDescription)> {
    let offer = SdpOffer::from_sdp_string(&offer.sdp).map_err(|e| anyhow!("bad offer: {e}"))?;
    let mut config = RtcConfig::new()
        .set_ice_lite(true)
        // Playout delay is not in str0m's standard extensions, so without this the 0/0 delay
        // `step` writes on every video packet is never negotiated and never sent. The receiver's
        // WebRTC (Android, iOS, and browsers that ignore `jitterBufferTarget`) then buffers frames
        // to smooth jitter, which costs 40 ms on a quiet network and hundreds of milliseconds
        // over Wi-Fi. With it, frames are shown as soon as they are decoded. The id only matters
        // if the client offers none: str0m takes the client's.
        .set_extension(PLAYOUT_DELAY_EXT_ID, Extension::PlayoutDelay)
        .clear_codecs()
        .enable_vp8(codecs.allows(VideoCodec::Vp8))
        .enable_opus(true, false)
        // The estimate starts at the starting rate and probes for more, up to the ceiling.
        .enable_bwe(Some(Bitrate::kbps(u64::from(video.start_kbps))));
    if codecs.allows(VideoCodec::H264) {
        // Constrained Baseline, level 3.1, packetization mode 1: what every WebRTC client
        // decodes, and the profile the encoder writes.
        config.codec_config().add_h264(
            H264_PT.into(),
            Some(H264_RTX_PT.into()),
            true,
            H264_PROFILE_LEVEL_ID,
        );
    }
    let mut rtc = config.build(Instant::now());
    rtc.bwe().set_bitrate(
        Bitrate::kbps(u64::from(video.min_kbps)),
        Bitrate::kbps(u64::from(video.max_kbps)),
    );
    rtc.add_local_candidate(Candidate::host(advertised, "udp")?);
    let answer = rtc.sdp_api().accept_offer(offer)?;
    let peer = Peer { rtc, video: None, audio: None, input: None, connected: false };
    Ok((peer, SessionDescription { kind: SdpKind::Answer, sdp: answer.to_sdp_string() }))
}

/// The payload type negotiated for `codec` on `mid`.
fn payload_type(rtc: &mut Rtc, mid: Mid, codec: Codec) -> Option<Pt> {
    let writer = rtc.writer(mid)?;
    writer.payload_params().find(|p| p.spec().codec == codec).map(|p| p.pt())
}

/// Runs the session until the command channel closes, a `Shutdown` arrives, or no client has been
/// connected for `idle_timeout`, and says which.
pub(crate) fn run(
    mut session: Session,
    socket: UdpSocket,
    advertised: SocketAddr,
    commands: Receiver<Command>,
    on_event: &mut dyn FnMut(StreamEvent),
    idle_timeout: Option<Duration>,
) -> Result<ExitReason> {
    // Tag every datagram with the advertised address; see `advertised_addr`.
    let local = advertised;
    let mut buf = vec![0u8; 2048];
    let mut peer: Option<Peer> = None;
    let mut pacer: Option<Pacer> = None;
    let mut next_frame = Instant::now();
    let mut idle_since = Some(Instant::now());

    watchdog::start();
    loop {
        watchdog::beat(watchdog::Phase::Network);
        // Commands: a new client replaces the current one; control requests run between frames.
        loop {
            match commands.try_recv() {
                Ok(Command::Offer { offer, reply }) => {
                    let result = accept_offer(
                        &offer,
                        advertised,
                        session.adaptation.limits(),
                        session.video.codec_setting(),
                    );
                    let answer = match result {
                        Ok((new_peer, answer)) => {
                            if peer.take().is_some_and(|p| p.connected) {
                                tracing::info!("new client replaces the current one");
                                session.health.suspend(Instant::now());
                                on_event(StreamEvent::ClientDisconnected);
                            }
                            session.new_client();
                            peer = Some(new_peer);
                            pacer = None;
                            Ok(answer)
                        }
                        Err(e) => {
                            tracing::warn!(error = %format!("{e:#}"), "rejected offer");
                            Err(format!("{e:#}"))
                        }
                    };
                    let _ = reply.send(answer);
                }
                Ok(Command::Control { request, reply }) => {
                    let result = session.control(&request);
                    let stop = matches!(result, Ok(true));
                    let _ = reply.send(result.map(|_| ()));
                    if stop {
                        tracing::info!("shutting down on request");
                        return Ok(ExitReason::Stopped);
                    }
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    tracing::info!("controller went away; shutting down");
                    return Ok(ExitReason::ControllerGone);
                }
            }
        }

        // Drive the peer until it is waiting on time or the network.
        let mut rtc_deadline = Instant::now() + IDLE_WAIT;
        if let Some(p) = &mut peer {
            let was_connected = p.connected;
            match drive(p, &socket, &mut session) {
                Ok(Some(deadline)) => {
                    rtc_deadline = rtc_deadline.min(deadline);
                    if p.connected && !was_connected {
                        on_event(StreamEvent::ClientConnected);
                    }
                }
                Ok(None) => {
                    tracing::info!("client disconnected; pausing");
                    session.health.suspend(Instant::now());
                    if was_connected {
                        on_event(StreamEvent::ClientDisconnected);
                    }
                    peer = None;
                    pacer = None;
                }
                Err(e) => {
                    tracing::warn!(error = %format!("{e:#}"), "peer failed; dropping it");
                    session.health.suspend(Instant::now());
                    if was_connected {
                        on_event(StreamEvent::ClientDisconnected);
                    }
                    peer = None;
                    pacer = None;
                }
            }
        }

        let running = peer.as_ref().is_some_and(|p| p.connected);
        if running {
            idle_since = None;
        } else {
            let since = *idle_since.get_or_insert_with(Instant::now);
            if idle_timeout.is_some_and(|limit| since.elapsed() >= limit) {
                tracing::info!(
                    idle_s = idle_timeout.unwrap_or_default().as_secs(),
                    "no client for the idle timeout; exiting"
                );
                return Ok(ExitReason::Idle);
            }
        }
        if running && pacer.is_none() {
            let now = Instant::now();
            pacer = Some(Pacer::new(session.fps, now));
            next_frame = now;
            session.health.resume(now);
            session.video.request_keyframe();
        }
        let deadline = if running { rtc_deadline.min(next_frame) } else { rtc_deadline };

        // Wait for a datagram or the deadline.
        let wait = deadline.saturating_duration_since(Instant::now());
        if !wait.is_zero() {
            socket.set_read_timeout(Some(wait))?;
            match socket.recv_from(&mut buf) {
                Ok((n, source)) => {
                    if let Some(p) = &mut peer
                        && let Ok(contents) = buf[..n].try_into()
                    {
                        let input = Input::Receive(
                            Instant::now(),
                            Receive { proto: Protocol::Udp, source, destination: local, contents },
                        );
                        if p.rtc.accepts(&input) {
                            p.rtc.handle_input(input)?;
                        }
                    }
                }
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
                // An ICMP error from an earlier send to a client that went away.
                Err(e) => tracing::debug!(error = %e, "UDP receive failed"),
            }
        }
        if let Some(p) = &mut peer {
            p.rtc.handle_input(Input::Timeout(Instant::now()))?;
        }

        // Emulate a frame if one is due.
        if let (Some(p), Some(pacer)) = (&mut peer, &mut pacer)
            && p.connected
            && Instant::now() >= next_frame
        {
            let behind = Instant::now().saturating_duration_since(next_frame);
            session.step(p, behind)?;
            next_frame = pacer.next_deadline(Instant::now());
        }
        for event in session.take_events() {
            on_event(event);
        }
    }
}

/// Polls the peer until it wants to wait. Returns its next deadline, or `None` if it is gone.
fn drive(p: &mut Peer, socket: &UdpSocket, session: &mut Session) -> Result<Option<Instant>> {
    loop {
        if !p.rtc.is_alive() {
            return Ok(None);
        }
        match p.rtc.poll_output()? {
            Output::Timeout(t) => return Ok(Some(t)),
            Output::Transmit(t) => {
                if let Err(e) = socket.send_to(&t.contents, t.destination) {
                    tracing::debug!(destination = %t.destination, error = %e, "UDP send failed");
                }
            }
            Output::Event(event) => match event {
                Event::Connected => {
                    tracing::info!("client connected");
                    p.connected = true;
                }
                Event::IceConnectionStateChange(IceConnectionState::Disconnected) => {
                    return Ok(None);
                }
                Event::MediaAdded(m) if m.kind == MediaKind::Video => {
                    let h264 = payload_type(&mut p.rtc, m.mid, Codec::H264);
                    let vp8 = payload_type(&mut p.rtc, m.mid, Codec::Vp8);
                    match session.video.negotiate(h264.is_some(), vp8.is_some()) {
                        Some(codec) => {
                            let pt = if codec == VideoCodec::H264 { h264 } else { vp8 };
                            p.video = pt.map(|pt| (m.mid, pt));
                            session.video.request_keyframe();
                            tracing::info!(codec = codec.name(), "video negotiated");
                        }
                        None => tracing::warn!(
                            h264 = h264.is_some(),
                            vp8 = vp8.is_some(),
                            "client offered video without a codec in common"
                        ),
                    }
                }
                Event::MediaAdded(m) => match payload_type(&mut p.rtc, m.mid, Codec::Opus) {
                    Some(pt) => p.audio = Some((m.mid, pt)),
                    None => {
                        tracing::warn!(kind = ?m.kind, "client offered media without the codec")
                    }
                },
                Event::ChannelOpen(id, label) if label == INPUT_CHANNEL => {
                    p.input = Some(id);
                    session.pipeline_to_client_at = None;
                }
                Event::ChannelClose(id) if Some(id) == p.input => {
                    tracing::warn!("input channel closed: releasing held input");
                    p.input = None;
                    session.release_input();
                    session.hotkeys = Hotkeys::default();
                }
                Event::ChannelData(data) if Some(data.id) == p.input => {
                    session.handle_input(&data.data);
                }
                Event::KeyframeRequest(_) => session.video.request_keyframe(),
                Event::EgressBitrateEstimate(
                    BweKind::Twcc { estimate, .. } | BweKind::Remb { estimate, .. },
                ) => session.adapt_bitrate(estimate.as_u64()),
                _ => {}
            },
        }
    }
}

impl Drop for Session {
    /// Idle timeout, a lost controller or an error: still keep the player's progress.
    fn drop(&mut self) {
        self.finish();
    }
}

impl Session {
    /// Applies a message from the input channel: controller state (if newer than the last one
    /// applied) or hotkeys.
    fn handle_input(&mut self, bytes: &[u8]) {
        let packet = match ClientMessage::decode(bytes) {
            Ok(ClientMessage::Input(p)) => p,
            Ok(ClientMessage::Hotkeys(h)) => {
                self.hotkeys = h;
                self.hotkeys_at = Instant::now();
                return;
            }
            Ok(ClientMessage::Analog(a)) => {
                if self.last_analog_seq.is_some_and(|last| !a.is_newer_than(last)) {
                    return;
                }
                self.last_analog_seq = Some(a.seq);
                let sticks: Vec<AnalogSticks> = a
                    .ports
                    .iter()
                    .map(|p| AnalogSticks { left: p.left, right: p.right, l2: p.l2, r2: p.r2 })
                    .collect();
                let off_centre = sticks.iter().any(|s| *s != AnalogSticks::CENTRED);
                self.analog_at = off_centre.then(Instant::now);
                self.core.set_analog(&sticks);
                return;
            }
            Ok(ClientMessage::Stats(stats)) => {
                let now = Instant::now();
                if self.stats_at.is_some_and(|at| now.duration_since(at) < STATS_SPACING) {
                    return;
                }
                self.stats_at = Some(now);
                let sample = ClientSample {
                    fps: stats.fps,
                    decode_ms: stats.decode_ms,
                    frames_dropped: stats.frames_dropped,
                    frames_sent: self.frames_sent,
                };
                match self.adaptation.client(sample, self.video.bitrate_kbps(), now) {
                    Some(LimitChange::Cut { to_kbps, why }) => tracing::info!(
                        to_kbps,
                        why = why.name(),
                        fps = stats.fps,
                        decode_ms = stats.decode_ms,
                        frames_dropped = stats.frames_dropped,
                        "client is struggling: video bitrate limited"
                    ),
                    Some(LimitChange::Raised { to_kbps }) => {
                        tracing::debug!(to_kbps, "client is keeping up: bitrate limit raised");
                    }
                    Some(LimitChange::Lifted) => {
                        tracing::info!("client is keeping up: bitrate limit lifted");
                    }
                    None => {}
                }
                self.apply_bitrate(now);
                // Every sample goes to the server (the admin's sessions page); the log gets
                // one as often as it gets a health line.
                let due = |every| self.stats_logged_at.is_none_or(|at| now - at >= every);
                if self.stats_log_interval.is_some_and(due) {
                    self.stats_logged_at = Some(now);
                    log_client_stats(&stats);
                }
                self.events.push(StreamEvent::ClientStats(*stats));
                return;
            }
            Err(e) => {
                tracing::debug!(error = %e, "bad input message");
                return;
            }
        };
        if self.last_seq.is_some_and(|last| !packet.is_newer_than(last)) {
            return;
        }
        self.last_seq = Some(packet.seq);
        self.buttons = packet.ports.iter().map(|&b| JoypadButtons(b)).collect();
        self.buttons_at = Instant::now();
        self.pending_ack = Some((packet.seq, packet.client_time_ms));
        self.health.input();
    }

    /// Lets go of every button and stick (the client stopped saying what it holds). Hotkeys
    /// (pause among them) lapse on their own, refreshed separately: a lapse of buttons must not
    /// unpause a game whose menu is still open.
    fn release_input(&mut self) {
        self.buttons.clear();
        self.analog_at = None;
        self.core.set_analog(&[]);
    }

    /// Releases held input the client has stopped refreshing ([`INPUT_LAPSE`]), and says so:
    /// a client that goes quiet while holding something is a fault worth seeing in the log.
    fn lapse_input(&mut self, now: Instant) {
        let held = self.buttons.iter().any(|b| b.0 != 0);
        if input_lapsed(held, self.buttons_at, self.analog_at, now) {
            tracing::warn!(
                buttons = ?self.buttons.iter().map(|b| b.0).collect::<Vec<_>>(),
                silent_ms = now.duration_since(self.buttons_at).as_millis() as u64,
                "client stopped sending input while holding it: released"
            );
            self.release_input();
        }
    }

    /// Takes a send-bandwidth estimate.
    fn adapt_bitrate(&mut self, estimate_bps: u64) {
        self.estimate_kbps = Some(estimate_bps / 1000);
        self.adaptation.estimate(estimate_bps);
        self.apply_bitrate(Instant::now());
    }

    /// Changes the encoder's bitrate to what the connection and the client call for now, if
    /// the change is worth making (see [`Adaptation`]).
    fn apply_bitrate(&mut self, now: Instant) {
        let current = self.video.bitrate_kbps();
        let reopens = self.video.rate_change_reopens();
        if let Some(target) = self.adaptation.decide(current, reopens, now) {
            tracing::debug!(
                from_kbps = current,
                to_kbps = target,
                estimate_kbps = self.estimate_kbps,
                client_limit_kbps = self.adaptation.client_limit_kbps(),
                "video bitrate changed"
            );
            if let Err(e) = self.video.set_bitrate(target) {
                tracing::warn!(error = %e, "cannot change the bitrate");
            }
        }
        // The player is told of a change worth a notice, through the pipeline message.
        let (told, _) = self.adaptation.told();
        let (kbps, by) = self.adaptation.tell(self.video.bitrate_kbps(), now);
        if kbps != told {
            tracing::info!(
                from_kbps = told,
                to_kbps = kbps,
                limited_by = by.name(),
                "video quality changed: the player is told"
            );
        }
    }

    /// A new client: its connection and its decoder are unknown, so the stream starts over
    /// at the starting rate.
    fn new_client(&mut self) {
        self.adaptation.reset();
        self.estimate_kbps = None;
        self.stats_at = None;
        self.stats_logged_at = None;
        self.apply_bitrate(Instant::now());
    }

    /// Runs one tick (one frame; several when fast-forwarding; a step back when rewinding) and
    /// sends its video, audio and any input ack to the peer. `behind`: how late it started.
    fn step(&mut self, p: &mut Peer, behind: Duration) -> Result<()> {
        watchdog::beat(watchdog::Phase::Emulating);
        let now = Instant::now();
        self.lapse_input(now);
        let hotkeys = if now.duration_since(self.hotkeys_at) < INPUT_LAPSE {
            self.hotkeys
        } else {
            Hotkeys::default()
        };
        // The client's menu is open: hold the game where it is. The last frame stays on the
        // client's screen and its audio goes quiet; a lapsed flag (client gone) lets go.
        if hotkeys.pause {
            self.health.paused();
            let at = self.health_now();
            if let Some(h) = self.health.report_if_due(now, &at) {
                self.events.push(StreamEvent::Health(h));
            }
            return Ok(());
        }
        if hotkeys.rewind {
            let core = &mut self.core;
            self.rewind.step_back(|state| core.unserialize(state));
        } else if hotkeys.fast_forward {
            for _ in 1..self.fast_forward {
                self.core.run_frame(&self.buttons)?;
                self.frame += 1;
                let core = &mut self.core;
                self.rewind.record(|| core.serialize());
            }
        }

        let out = self.core.run_frame(&self.buttons)?;
        let frame = self.frame;
        self.frame += 1;
        let tick = self.ticks;
        self.ticks += 1;

        if let Some(av) = out.av_changed {
            tracing::info!(
                width = av.base_width,
                height = av.base_height,
                fps = av.fps,
                sample_rate = av.sample_rate,
                "core changed its geometry or timing"
            );
            if (av.sample_rate - self.audio.input_rate()).abs() > 0.01 {
                self.audio.set_input_rate(av.sample_rate)?;
            }
            self.fps = av.fps;
            self.health.set_fps(av.fps);
        }

        let drew = out.video_updated && out.video.width > 0;
        if self.pending_load.is_some() {
            // Nothing of the fresh boot is shown or heard: the player asked to continue.
            if drew || self.frame >= PENDING_LOAD_FRAMES {
                let (slot, bytes) = self.pending_load.take().expect("checked above");
                self.drawn = true;
                if let Err(e) = Persist::apply_state(&mut self.core, slot, &bytes) {
                    tracing::warn!(slot, error = %format!("{e:#}"), "held state could not be loaded");
                }
                self.rewind.clear();
                self.video.request_keyframe();
            }
            self.health.ticked(behind, now.elapsed());
            self.health.core(self.core.take_timing());
            return Ok(());
        }

        let mut picture = None;
        if drew {
            self.drawn = true;
            out.video.write_rgba8(&mut self.rgba);
            picture = Some((out.video.width, out.video.height));
        }

        // The sound is always the real timeline's. Rewinding plays silence (the browser fills
        // the gap).
        let audio: &[i16] = if hotkeys.rewind { &[] } else { out.audio };
        for packet in self.audio.push(audio)? {
            if let Some((mid, pt)) = p.audio
                && let Some(writer) = p.rtc.writer(mid)
            {
                let rtp_time = MediaTime::new(self.audio_samples, Frequency::FORTY_EIGHT_KHZ);
                writer.write(pt, now, rtp_time, packet)?;
            }
            self.audio_samples += sovemu_media::audio::PACKET_FRAMES as u64;
        }

        // The picture runs ahead of the real frame, hiding the game's own input lag; not while
        // rewinding or fast-forwarding, whose pictures are meant to be elsewhere in time.
        if !hotkeys.rewind
            && !hotkeys.fast_forward
            && let Some(ahead) =
                self.run_ahead.picture(&mut self.core, &self.buttons, &mut self.rgba)?
        {
            picture = Some(ahead);
        }

        // The ack goes out before this frame's video, so the first frame a client draws after the
        // ack is the one that shows the input: timing from the press to that frame measures
        // button to frame, decoding and any frames the decoder holds included.
        if let (Some((seq, client_time_ms)), Some(id)) = (self.pending_ack.take(), p.input)
            && let Some(mut channel) = p.rtc.channel(id)
        {
            let ack = InputAck { seq, client_time_ms, frame: frame as u32 };
            let _ = channel.write(true, &ack.encode());
        }

        if let Some((w, h)) = picture {
            self.shot_size = (w, h);
            watchdog::beat(watchdog::Phase::Encoding);
            let encode_from = Instant::now();
            let encoded = self.video.encode(&self.rgba, w, h)?;
            if let Some(encoded) = &encoded {
                self.health.encoded(encode_from.elapsed(), encoded.data.len());
            }
            if let (Some(encoded), Some((mid, pt))) = (encoded, p.video)
                && let Some(writer) = p.rtc.writer(mid)
            {
                let rtp_time = MediaTime::from_90khz((tick as f64 * 90_000.0 / self.fps) as u64);
                writer.playout_delay(MediaTime::from_millis(0), MediaTime::from_millis(0)).write(
                    pt,
                    now,
                    rtp_time,
                    encoded.data,
                )?;
                self.frames_sent += 1;
            }
        }

        if !hotkeys.rewind {
            let core = &mut self.core;
            self.rewind.record(|| core.serialize());
        }
        self.persist.tick(&mut self.core, now);
        self.health.ticked(behind, now.elapsed());
        self.health.core(self.core.take_timing());
        // A client's limit lapses when its stats stop, with nothing else to notice it by.
        if tick.is_multiple_of(64) {
            self.apply_bitrate(now);
        }
        let at = self.health_now();
        if let Some(h) = self.health.report_if_due(Instant::now(), &at) {
            self.events.push(StreamEvent::Health(h));
        }
        self.refresh_pipeline(p);
        Ok(())
    }

    /// How the stream is made right now.
    fn pipeline(&self) -> Pipeline {
        let (width, height) = self.shot_size;
        let renderer = self.core.renderer();
        Pipeline {
            renderer: renderer.map(|r| r.renderer.clone()),
            renderer_software: renderer.is_some_and(|r| r.software),
            video: {
                let (kbps, by) = self.adaptation.told();
                let path = self.video.path(width, height, self.fps);
                VideoPath { kbps, limited_by: by.name().into(), ..path }
            },
            audio: AudioPath {
                codec: "opus".into(),
                source_hz: self.audio.input_rate(),
                kbps: self.audio_kbps,
            },
            run_ahead: self.run_ahead.frames(),
        }
    }

    /// Reports the pipeline to the server when it changes, and sends it to the client then and
    /// every [`PIPELINE_RESEND`] (the input channel drops what it loses).
    fn refresh_pipeline(&mut self, p: &mut Peer) {
        if self.shot_size == (0, 0) {
            return; // nothing encoded yet
        }
        let now = Instant::now();
        let pipeline = self.pipeline();
        if self.pipeline.as_ref() != Some(&pipeline) {
            self.events.push(StreamEvent::Pipeline(pipeline.clone()));
            self.pipeline = Some(pipeline);
            self.pipeline_to_client_at = None;
        }
        let due =
            self.pipeline_to_client_at.is_none_or(|at| now.duration_since(at) >= PIPELINE_RESEND);
        if due
            && let (Some(id), Some(pipeline)) = (p.input, &self.pipeline)
            && let Some(mut channel) = p.rtc.channel(id)
        {
            let _ = channel.write(true, &pipeline.encode());
            self.pipeline_to_client_at = Some(now);
        }
    }

    /// The events gathered since the last call.
    pub(crate) fn take_events(&mut self) -> Vec<StreamEvent> {
        std::mem::take(&mut self.events)
    }
}

/// A held state is loaded after this many frames even if the core has drawn nothing (ten seconds
/// at 60 fps): a core with no picture yet must not hold a save back for ever.
const PENDING_LOAD_FRAMES: u64 = 600;

/// How often the pipeline goes to the client even when it has not changed: the input channel
/// does not retransmit, so a lost one is replaced soon.
const PIPELINE_RESEND: Duration = Duration::from_secs(2);

/// Clients send their stats once a second; any faster is dropped.
const STATS_SPACING: Duration = Duration::from_millis(900);

/// The client's view of the stream, beside the worker's own `session health` lines.
fn log_client_stats(s: &ClientStats) {
    tracing::info!(
        client = s.client.as_deref(),
        estimate_ms = s.estimate_ms,
        input_to_frame_ms = s.input_to_frame_ms,
        input_rtt_p50_ms = s.input_rtt_p50_ms,
        input_rtt_p95_ms = s.input_rtt_p95_ms,
        net_rtt_ms = s.net_rtt_ms,
        jitter_ms = s.jitter_ms,
        decode_ms = s.decode_ms,
        width = s.width,
        height = s.height,
        fps = s.fps,
        kbps = s.kbps,
        packets_lost = s.packets_lost,
        frames_dropped = s.frames_dropped,
        game_mode = s.game_mode,
        wifi_lock = s.wifi_lock,
        low_latency_decode = s.low_latency_decode,
        decoder = s.decoder.as_deref(),
        "client stats"
    );
}

/// A core's declared options and categories, in the protocol's terms (each option with its
/// core default, not the value SovEmu set).
pub(crate) fn declared_options(core: &Core) -> (Vec<CoreOption>, Vec<CoreCategory>) {
    let options = core
        .variables()
        .into_iter()
        .map(|v| CoreOption {
            key: v.key,
            description: v.description,
            info: v.info,
            category: v.category,
            choices: v.options,
            labels: v.labels,
            default: v.default,
        })
        .collect();
    let categories = core
        .option_categories()
        .into_iter()
        .map(|c| CoreCategory { key: c.key, description: c.description, info: c.info })
        .collect();
    (options, categories)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn held_input_lets_go_once_the_client_stops_refreshing_it() {
        let t = Instant::now();
        let soon = t + Duration::from_millis(100);
        let late = t + INPUT_LAPSE;
        // Left held, refreshed every 100 ms: still held.
        assert!(!input_lapsed(true, t, None, soon));
        // The refreshes stop (the input channel stalled, the app lost its window): released.
        assert!(input_lapsed(true, t, None, late));
        // Nothing held: nothing to release, however long the client is quiet.
        assert!(!input_lapsed(false, t, None, late));
        // A stick pushed over, its heartbeat stopped: released too; a centred one sends none.
        assert!(input_lapsed(false, t, Some(t), late));
        assert!(!input_lapsed(false, t, Some(soon), soon + Duration::from_millis(100)));
    }

    #[test]
    fn an_explicit_advertise_address_wins() {
        let bound: SocketAddr = "0.0.0.0:50000".parse().unwrap();
        let ip: IpAddr = "203.0.113.7".parse().unwrap();
        assert_eq!(advertised_addr(Some(ip), bound), SocketAddr::new(ip, 50000));
    }

    #[test]
    fn a_specific_bind_is_advertised_as_is() {
        let bound: SocketAddr = "127.0.0.1:50001".parse().unwrap();
        assert_eq!(advertised_addr(None, bound), bound);
    }

    #[test]
    fn a_wildcard_bind_advertises_a_real_address() {
        let bound: SocketAddr = "0.0.0.0:50002".parse().unwrap();
        let addr = advertised_addr(None, bound);
        assert!(!addr.ip().is_unspecified());
        assert_eq!(addr.port(), 50002);
    }
}
