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
use sovemu_media::{AudioEncoder, Vp8Config, Vp8Encoder};
use sovemu_protocol::ipc::Request;
use sovemu_protocol::ipc::{CoreCategory, CoreOption, LoadPhase};
use sovemu_protocol::{
    ClientMessage, Hotkeys, INPUT_CHANNEL, InputAck, SdpKind, SessionDescription,
};
use sovemu_retro_host::{AnalogSticks, AvInfo, Core, CoreOptions, JoypadButtons};
use str0m::bwe::{Bitrate, BweKind};
use str0m::change::SdpOffer;
use str0m::channel::ChannelId;
use str0m::format::Codec;
use str0m::media::{Frequency, MediaKind, MediaTime, Mid, Pt};
use str0m::net::{Protocol, Receive};
use str0m::{Candidate, Event, IceConnectionState, Input, Output, Rtc, RtcConfig};

use crate::pacing::Pacer;
use crate::persist::{Persist, Screenshot};
use crate::rewind::Rewind;

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

/// The lowest video bitrate adaptation will choose, kbit/s.
const MIN_VIDEO_KBPS: u32 = 300;

/// The video bitrate to use for a send-bandwidth estimate: 85% of it (headroom for retransmits
/// and estimate noise) minus the audio, clamped to `[MIN_VIDEO_KBPS, max_kbps]`.
pub(crate) fn target_video_kbps(estimate_bps: u64, audio_kbps: u32, max_kbps: u32) -> u32 {
    let usable = (estimate_bps as f64 * 0.85 / 1000.0) as i64 - i64::from(audio_kbps);
    usable.clamp(i64::from(MIN_VIDEO_KBPS.min(max_kbps)), i64::from(max_kbps)) as u32
}

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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StreamEvent {
    ClientConnected,
    ClientDisconnected,
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
    pub video_kbps: u32,
    pub audio_kbps: u32,
    /// Where save states go (`slotN.state` + `slotN.png`); none disables them.
    pub state_dir: Option<PathBuf>,
    pub rewind_seconds: u32,
    pub rewind_max_mb: u32,
    pub fast_forward: u32,
    pub keyframe_interval: u32,
    /// The GPU for OpenGL cores; `None` refuses them.
    pub gpu: Option<sovemu_retro_host::GpuChoice>,
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
    video: Vp8Encoder,
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
    /// The configured (maximum) video bitrate; adaptation stays at or below it.
    max_video_kbps: u32,
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
            tracing::debug!("system dir {}: {e}", config.system_dir.display());
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
            "{} {}: {}x{} @ {:.4} fps, audio {} Hz",
            core.system_info().library_name,
            core.system_info().library_version,
            av.base_width,
            av.base_height,
            av.fps,
            av.sample_rate
        );
        Ok(Self {
            _system_dir: system_dir,
            video: Vp8Encoder::new(Vp8Config {
                bitrate_kbps: config.video_kbps,
                fps: av.fps.round() as u32,
                keyframe_interval: config.keyframe_interval.max(1),
                ..Vp8Config::default()
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
            ),
            fast_forward: config.fast_forward.max(1),
            max_video_kbps: config.video_kbps,
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
        let (width, height) = self.shot_size;
        let shot = Screenshot { rgba: &self.rgba, width, height };
        self.persist.finish(&mut self.core, self.frame > 0, Some(shot));
    }

    /// The core's name and the content's geometry and timing.
    /// The options the core has declared now that its game is loaded.
    pub(crate) fn options(&self) -> (Vec<CoreOption>, Vec<CoreCategory>) {
        declared_options(&self.core)
    }

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
                tracing::info!("changed to disc {}", disc + 1);
                Ok(false)
            }
            Request::SetCheats { codes, .. } => {
                self.core.reset_cheats();
                for (i, code) in codes.iter().enumerate() {
                    self.core.set_cheat(i as u32, true, code);
                }
                tracing::info!("{} cheat(s) active", codes.len());
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
                self.persist.load_state(&mut self.core, *slot).map_err(|e| format!("{e:#}"))?;
                self.rewind.clear();
                self.video.request_keyframe();
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
    max_video_kbps: u32,
) -> Result<(Peer, SessionDescription)> {
    let offer = SdpOffer::from_sdp_string(&offer.sdp).map_err(|e| anyhow!("bad offer: {e}"))?;
    let mut rtc = RtcConfig::new()
        .set_ice_lite(true)
        .clear_codecs()
        .enable_vp8(true)
        .enable_opus(true, false)
        .enable_bwe(Some(Bitrate::kbps(u64::from(max_video_kbps))))
        .build(Instant::now());
    rtc.bwe().set_bitrate(
        Bitrate::kbps(u64::from(MIN_VIDEO_KBPS)),
        Bitrate::kbps(u64::from(max_video_kbps)),
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
/// connected for `idle_timeout`.
pub(crate) fn run(
    mut session: Session,
    socket: UdpSocket,
    advertised: SocketAddr,
    commands: Receiver<Command>,
    on_event: &mut dyn FnMut(StreamEvent),
    idle_timeout: Option<Duration>,
) -> Result<()> {
    // Tag every datagram with the advertised address; see `advertised_addr`.
    let local = advertised;
    let mut buf = vec![0u8; 2048];
    let mut peer: Option<Peer> = None;
    let mut pacer: Option<Pacer> = None;
    let mut next_frame = Instant::now();
    let mut idle_since = Some(Instant::now());

    loop {
        // Commands: a new client replaces the current one; control requests run between frames.
        loop {
            match commands.try_recv() {
                Ok(Command::Offer { offer, reply }) => {
                    let result = accept_offer(&offer, advertised, session.max_video_kbps);
                    let answer = match result {
                        Ok((new_peer, answer)) => {
                            if peer.take().is_some_and(|p| p.connected) {
                                tracing::info!("new client replaces the current one");
                                on_event(StreamEvent::ClientDisconnected);
                            }
                            peer = Some(new_peer);
                            pacer = None;
                            Ok(answer)
                        }
                        Err(e) => {
                            tracing::warn!("rejected offer: {e:#}");
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
                        return Ok(());
                    }
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    tracing::info!("controller went away; shutting down");
                    return Ok(());
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
                    if was_connected {
                        on_event(StreamEvent::ClientDisconnected);
                    }
                    peer = None;
                    pacer = None;
                }
                Err(e) => {
                    tracing::warn!("peer failed: {e:#}; dropping it");
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
                tracing::info!("no client for {:?}; exiting", idle_timeout.unwrap_or_default());
                return Ok(());
            }
        }
        if running && pacer.is_none() {
            let now = Instant::now();
            pacer = Some(Pacer::new(session.fps, now));
            next_frame = now;
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
                Err(e) => tracing::debug!("UDP receive: {e}"),
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
            session.step(p)?;
            next_frame = pacer.next_deadline(Instant::now());
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
                    tracing::debug!("UDP send to {}: {e}", t.destination);
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
                Event::MediaAdded(m) => {
                    let codec = if m.kind == MediaKind::Video { Codec::Vp8 } else { Codec::Opus };
                    let pt = payload_type(&mut p.rtc, m.mid, codec);
                    match (m.kind, pt) {
                        (MediaKind::Video, Some(pt)) => p.video = Some((m.mid, pt)),
                        (MediaKind::Audio, Some(pt)) => p.audio = Some((m.mid, pt)),
                        (kind, None) => tracing::warn!("client offered {kind:?} without {codec:?}"),
                    }
                }
                Event::ChannelOpen(id, label) if label == INPUT_CHANNEL => p.input = Some(id),
                Event::ChannelClose(id) if Some(id) == p.input => {
                    tracing::warn!("input channel closed: releasing held input");
                    p.input = None;
                    session.release_input();
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
            Err(e) => {
                tracing::debug!("bad input message: {e}");
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
    }

    /// Lets go of every button and stick (the client stopped saying what it holds).
    fn release_input(&mut self) {
        self.buttons.clear();
        self.analog_at = None;
        self.core.set_analog(&[]);
        self.hotkeys = Hotkeys::default();
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

    /// Follows the send-bandwidth estimate, ignoring changes under 10%.
    fn adapt_bitrate(&mut self, estimate_bps: u64) {
        let target = target_video_kbps(estimate_bps, self.audio_kbps, self.max_video_kbps);
        let current = self.video.config().bitrate_kbps;
        if target.abs_diff(current) * 10 > current {
            tracing::debug!(
                "video bitrate {current} -> {target} kbit/s (estimate {estimate_bps} bit/s)"
            );
            if let Err(e) = self.video.set_bitrate(target) {
                tracing::warn!("cannot change the bitrate: {e}");
            }
        }
    }

    /// Runs one tick (one frame; several when fast-forwarding; a step back when rewinding) and
    /// sends its video, audio and any input ack to the peer.
    fn step(&mut self, p: &mut Peer) -> Result<()> {
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
                "core changed to {}x{} @ {:.4} fps, {} Hz",
                av.base_width,
                av.base_height,
                av.fps,
                av.sample_rate
            );
            if (av.sample_rate - self.audio.input_rate()).abs() > 0.01 {
                self.audio.set_input_rate(av.sample_rate)?;
            }
            self.fps = av.fps;
        }

        if out.video_updated && out.video.width > 0 {
            out.video.write_rgba8(&mut self.rgba);
            let (w, h) = (out.video.width, out.video.height);
            self.shot_size = (w, h);
            if let (Some(encoded), Some((mid, pt))) =
                (self.video.encode(&self.rgba, w, h)?, p.video)
                && let Some(writer) = p.rtc.writer(mid)
            {
                let rtp_time = MediaTime::from_90khz((tick as f64 * 90_000.0 / self.fps) as u64);
                writer.playout_delay(MediaTime::from_millis(0), MediaTime::from_millis(0)).write(
                    pt,
                    now,
                    rtp_time,
                    encoded.data,
                )?;
            }
        }

        // Rewinding plays silence (the browser fills the gap).
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

        if let (Some((seq, client_time_ms)), Some(id)) = (self.pending_ack.take(), p.input)
            && let Some(mut channel) = p.rtc.channel(id)
        {
            let ack = InputAck { seq, client_time_ms, frame: frame as u32 };
            let _ = channel.write(true, &ack.encode());
        }
        if !hotkeys.rewind {
            let core = &mut self.core;
            self.rewind.record(|| core.serialize());
        }
        self.persist.tick(&mut self.core, now);
        Ok(())
    }
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
    fn bitrate_follows_the_estimate_within_bounds() {
        assert_eq!(target_video_kbps(4_000_000, 128, 2500), 2500, "capped at the configured rate");
        assert_eq!(target_video_kbps(2_000_000, 128, 2500), 1572);
        assert_eq!(target_video_kbps(100_000, 128, 2500), 300, "never below the floor");
        assert_eq!(
            target_video_kbps(100_000, 128, 200),
            200,
            "a floor above the cap yields the cap"
        );
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
