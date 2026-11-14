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

use anyhow::{Context, Result, anyhow};
use sovemu_media::{AudioEncoder, Vp8Config, Vp8Encoder};
use sovemu_protocol::ipc::Request;
use sovemu_protocol::{INPUT_CHANNEL, InputAck, InputPacket, SdpKind, SessionDescription};
use sovemu_retro_host::{AvInfo, Core, CoreOptions, JoypadButtons};
use str0m::change::SdpOffer;
use str0m::channel::ChannelId;
use str0m::format::Codec;
use str0m::media::{Frequency, MediaKind, MediaTime, Mid, Pt};
use str0m::net::{Protocol, Receive};
use str0m::{Candidate, Event, IceConnectionState, Input, Output, Rtc, RtcConfig};

use crate::pacing::Pacer;
use crate::persist::{Persist, Screenshot};

/// How long the loop sleeps at most when idle, so new commands are picked up promptly.
const IDLE_WAIT: Duration = Duration::from_millis(20);

/// Work for the streaming loop from its front end (HTTP in `serve`, stdin in `session`).
#[derive(Debug)]
pub enum Command {
    /// A client's offer; the new client replaces any current one.
    Offer { offer: SessionDescription, reply: Sender<Result<SessionDescription, String>> },
    /// A control request: reset, save or load a state, shut down.
    Control { request: Request, reply: Sender<Result<(), String>> },
}

/// Something the front end may want to report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamEvent {
    ClientConnected,
    ClientDisconnected,
}

/// How to start a session.
#[derive(Debug, Clone)]
pub struct SessionConfig {
    pub core: PathBuf,
    pub rom: Option<PathBuf>,
    pub system_dir: PathBuf,
    pub save_dir: PathBuf,
    pub variables: Vec<(String, String)>,
    pub video_kbps: u32,
    pub audio_kbps: u32,
    /// Where save states go (`slotN.state` + `slotN.png`); none disables them.
    pub state_dir: Option<PathBuf>,
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
pub struct Session {
    core: Core,
    fps: f64,
    video: Vp8Encoder,
    audio: AudioEncoder,
    rgba: Vec<u8>,
    frame: u64,
    audio_samples: u64,
    buttons: Vec<JoypadButtons>,
    last_seq: Option<u32>,
    pending_ack: Option<(u32, u32)>,
    persist: Persist,
    /// Size of the frame in `rgba`, for save-state screenshots.
    shot_size: (u32, u32),
    finished: bool,
}

impl Session {
    /// Loads the core and content and sets up the encoders.
    pub fn open(config: &SessionConfig) -> Result<Self> {
        std::fs::create_dir_all(&config.system_dir)?;
        std::fs::create_dir_all(&config.save_dir)?;
        let options = CoreOptions {
            system_dir: config.system_dir.canonicalize()?,
            save_dir: config.save_dir.canonicalize()?,
            variables: config.variables.clone(),
        };
        let mut core = Core::load(&config.core, options)
            .with_context(|| format!("loading core {}", config.core.display()))?;
        let av = *core.load_game(config.rom.as_deref()).context("loading content")?;
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
            video: Vp8Encoder::new(Vp8Config {
                bitrate_kbps: config.video_kbps,
                fps: av.fps.round() as u32,
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
            shot_size: (0, 0),
            finished: false,
        })
    }

    /// Saves the battery RAM and the auto state, once.
    pub fn finish(&mut self) {
        if std::mem::replace(&mut self.finished, true) {
            return;
        }
        let (width, height) = self.shot_size;
        let shot = Screenshot { rgba: &self.rgba, width, height };
        self.persist.finish(&mut self.core, self.frame > 0, Some(shot));
    }

    /// The core's name and the content's geometry and timing.
    pub fn describe(&self) -> (String, AvInfo) {
        let av = *self.core.av_info().expect("a game is loaded");
        (self.core.system_info().library_name.clone(), av)
    }

    /// Handles a control request. Returns `Ok(true)` if the loop should stop.
    fn control(&mut self, request: &Request) -> Result<bool, String> {
        match request {
            Request::Reset { .. } => {
                self.core.reset();
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
pub fn advertised_addr(advertise: Option<IpAddr>, bound: SocketAddr) -> SocketAddr {
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
) -> Result<(Peer, SessionDescription)> {
    let offer = SdpOffer::from_sdp_string(&offer.sdp).map_err(|e| anyhow!("bad offer: {e}"))?;
    let mut rtc = RtcConfig::new()
        .set_ice_lite(true)
        .clear_codecs()
        .enable_vp8(true)
        .enable_opus(true, false)
        .build(Instant::now());
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
pub fn run(
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
                    let result = accept_offer(&offer, advertised);
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
                Event::ChannelData(data) if Some(data.id) == p.input => {
                    session.handle_input(&data.data);
                }
                Event::KeyframeRequest(_) => session.video.request_keyframe(),
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
    /// Applies an input packet, if it is newer than the last one applied.
    fn handle_input(&mut self, bytes: &[u8]) {
        let packet = match InputPacket::decode(bytes) {
            Ok(p) => p,
            Err(e) => {
                tracing::debug!("bad input packet: {e}");
                return;
            }
        };
        if self.last_seq.is_some_and(|last| !packet.is_newer_than(last)) {
            return;
        }
        self.last_seq = Some(packet.seq);
        self.buttons = packet.ports.iter().map(|&b| JoypadButtons(b)).collect();
        self.pending_ack = Some((packet.seq, packet.client_time_ms));
    }

    /// Runs one frame and sends its video, audio and any input ack to the peer.
    fn step(&mut self, p: &mut Peer) -> Result<()> {
        let out = self.core.run_frame(&self.buttons)?;
        let frame = self.frame;
        self.frame += 1;
        let now = Instant::now();

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
                let rtp_time = MediaTime::from_90khz((frame as f64 * 90_000.0 / self.fps) as u64);
                writer.playout_delay(MediaTime::from_millis(0), MediaTime::from_millis(0)).write(
                    pt,
                    now,
                    rtp_time,
                    encoded.data,
                )?;
            }
        }

        for packet in self.audio.push(out.audio)? {
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
        self.persist.tick(&mut self.core, now);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
