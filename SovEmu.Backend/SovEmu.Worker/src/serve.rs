//! `sovemu-worker serve`: run a core and stream it to one WebRTC client.
//!
//! Phase 1 is standalone: the worker serves the web client (`SovEmu.Web`'s build, from
//! `--web-dir`) and takes its offer on `POST /api/offer` itself. Phase 2 moves the web client,
//! the API and signalling into the server, which spawns this same loop per session.
//!
//! Everything media-related runs on one thread: the core, the encoders and the str0m peer
//! (sans-IO) sharing one UDP socket. Each turn of the loop drives the peer until it has nothing
//! to send, waits on the socket until whichever comes first of the peer's next timer and the
//! next frame's deadline, then runs a frame if one is due. Emulation runs only while a client is
//! connected, so the game pauses when nobody is watching.

use std::io::ErrorKind;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use sovemu_media::{AudioEncoder, Vp8Config, Vp8Encoder};
use sovemu_protocol::{INPUT_CHANNEL, InputAck, InputPacket, SdpKind, SessionDescription};
use sovemu_retro_host::{Core, CoreOptions, JoypadButtons};
use str0m::change::SdpOffer;
use str0m::channel::ChannelId;
use str0m::format::Codec;
use str0m::media::{Frequency, MediaKind, MediaTime, Mid, Pt};
use str0m::net::{Protocol, Receive};
use str0m::{Candidate, Event, IceConnectionState, Input, Output, Rtc, RtcConfig};

use crate::ServeArgs;
use crate::pacing::Pacer;
use crate::web;

/// How long the loop sleeps at most when idle, so new offers are picked up promptly.
const IDLE_WAIT: Duration = Duration::from_millis(20);
/// How long the HTTP thread waits for the session loop to answer an offer.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(5);

/// An offer from the HTTP thread, with the way back for its answer.
struct OfferRequest {
    offer: SessionDescription,
    reply: Sender<Result<SessionDescription, String>>,
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
struct Session {
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
}

pub fn run(args: ServeArgs) -> Result<()> {
    str0m::crypto::from_feature_flags().install_process_default();

    std::fs::create_dir_all(&args.system_dir)?;
    std::fs::create_dir_all(&args.save_dir)?;
    let options = CoreOptions {
        system_dir: args.system_dir.canonicalize()?,
        save_dir: args.save_dir.canonicalize()?,
        variables: args.variables.clone(),
    };
    let mut core = Core::load(&args.core, options)
        .with_context(|| format!("loading core {}", args.core.display()))?;
    let av = *core.load_game(args.rom.as_deref()).context("loading content")?;
    tracing::info!(
        "{} {}: {}x{} @ {:.4} fps, audio {} Hz",
        core.system_info().library_name,
        core.system_info().library_version,
        av.base_width,
        av.base_height,
        av.fps,
        av.sample_rate
    );

    let socket = UdpSocket::bind(args.udp).with_context(|| format!("binding UDP {}", args.udp))?;
    let udp_addr = socket.local_addr()?;
    let advertised = advertised_addr(args.advertise, udp_addr);
    tracing::info!("WebRTC on UDP {udp_addr}, advertising {advertised}");

    let (offers_tx, offers_rx) = mpsc::channel();
    let http_addr = spawn_http(args.http, args.web_dir.clone(), offers_tx)?;
    // Machine-readable lines for whoever started us (tests, and later the server).
    println!("http={http_addr}");
    println!("udp={udp_addr}");
    tracing::info!("open http://{http_addr}/ in a browser");

    let session = Session {
        video: Vp8Encoder::new(Vp8Config {
            bitrate_kbps: args.video_kbps,
            fps: av.fps.round() as u32,
            ..Vp8Config::default()
        }),
        audio: AudioEncoder::new(av.sample_rate, args.audio_kbps * 1000)?,
        fps: av.fps,
        core,
        rgba: Vec::new(),
        frame: 0,
        audio_samples: 0,
        buttons: vec![JoypadButtons::NONE; 2],
        last_seq: None,
        pending_ack: None,
    };
    session_loop(session, socket, advertised, offers_rx)
}

/// The one address clients send media to: `--advertise-ip` if given, else the bound address,
/// else (for a wildcard bind) the machine's primary address.
///
/// Exactly one, because str0m's ICE answers a connectivity check only when the packet's
/// destination is one of its local candidates, and with a wildcard bind the OS does not tell us
/// which local address a datagram arrived on. So every received datagram is tagged with this
/// address. In a container that is the host's address (`SOVEMU_ADVERTISE_IP`), which the
/// container could not discover on its own.
fn advertised_addr(advertise: Option<IpAddr>, bound: SocketAddr) -> SocketAddr {
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

/// Serves the web client and `POST /api/offer` on a background thread.
fn spawn_http(
    addr: SocketAddr,
    web_dir: Option<PathBuf>,
    offers: Sender<OfferRequest>,
) -> Result<SocketAddr> {
    let server = tiny_http::Server::http(addr).map_err(|e| anyhow!("binding HTTP {addr}: {e}"))?;
    let bound = server
        .server_addr()
        .to_ip()
        .ok_or_else(|| anyhow!("HTTP server is not on an IP socket"))?;
    thread::Builder::new().name("http".into()).spawn(move || {
        for mut request in server.incoming_requests() {
            let method = request.method().clone();
            let url = request.url().to_owned();
            let mut body = String::new();
            let body =
                std::io::Read::read_to_string(request.as_reader(), &mut body).ok().map(|_| body);
            if let Err(e) = handle_http(&method, &url, body, request, web_dir.as_deref(), &offers) {
                tracing::debug!("HTTP response failed: {e}");
            }
        }
    })?;
    Ok(bound)
}

fn handle_http(
    method: &tiny_http::Method,
    url: &str,
    body: Option<String>,
    request: tiny_http::Request,
    web_dir: Option<&Path>,
    offers: &Sender<OfferRequest>,
) -> std::io::Result<()> {
    use tiny_http::{Header, Method, Response};
    let json = Header::from_bytes("Content-Type", "application/json").expect("valid header");
    let html =
        Header::from_bytes("Content-Type", "text/html; charset=utf-8").expect("valid header");
    let path = url.split('?').next().unwrap_or("/");

    match (method, path) {
        (Method::Post, "/api/offer") => {
            let offer = body
                .and_then(|b| serde_json::from_str::<SessionDescription>(&b).ok())
                .filter(|d| d.kind == SdpKind::Offer);
            let Some(offer) = offer else {
                return request
                    .respond(Response::from_string("expected an SDP offer").with_status_code(400));
            };
            let (reply, answer) = mpsc::channel();
            if offers.send(OfferRequest { offer, reply }).is_err() {
                return request
                    .respond(Response::from_string("session ended").with_status_code(503));
            }
            match answer.recv_timeout(ANSWER_TIMEOUT) {
                Ok(Ok(answer)) => request.respond(
                    Response::from_string(serde_json::to_string(&answer).expect("serialisable"))
                        .with_header(json),
                ),
                Ok(Err(e)) => request.respond(Response::from_string(e).with_status_code(400)),
                Err(_) => request.respond(Response::from_string("no answer").with_status_code(503)),
            }
        }
        (Method::Get | Method::Head, path) if !path.starts_with("/api/") => {
            match web_dir.and_then(|dir| web::lookup(dir, path)) {
                Some(file) => {
                    let content_type = Header::from_bytes("Content-Type", file.content_type)
                        .expect("valid header");
                    let cache = Header::from_bytes("Cache-Control", file.cache_control)
                        .expect("valid header");
                    request.respond(
                        Response::from_data(file.body).with_header(content_type).with_header(cache),
                    )
                }
                None if web_dir.is_none() && path == "/" => {
                    request.respond(Response::from_string(web::NOT_INSTALLED).with_header(html))
                }
                None => request.respond(Response::from_string("not found").with_status_code(404)),
            }
        }
        _ => request.respond(Response::from_string("not found").with_status_code(404)),
    }
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

fn session_loop(
    mut session: Session,
    socket: UdpSocket,
    advertised: SocketAddr,
    offers: Receiver<OfferRequest>,
) -> Result<()> {
    // Tag every datagram with the advertised address; see `advertised_addr`.
    let local = advertised;
    let mut buf = vec![0u8; 2048];
    let mut peer: Option<Peer> = None;
    let mut pacer: Option<Pacer> = None;
    let mut next_frame = Instant::now();

    loop {
        // New client? It replaces the current one.
        match offers.try_recv() {
            Ok(request) => {
                let result = accept_offer(&request.offer, advertised);
                let reply = match result {
                    Ok((new_peer, answer)) => {
                        if peer.is_some() {
                            tracing::info!("new client replaces the current one");
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
                let _ = request.reply.send(reply);
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => return Err(anyhow!("HTTP thread exited")),
        }

        // Drive the peer until it is waiting on time or the network.
        let mut rtc_deadline = Instant::now() + IDLE_WAIT;
        if let Some(p) = &mut peer {
            match drive(p, &socket, &mut session) {
                Ok(Some(deadline)) => rtc_deadline = rtc_deadline.min(deadline),
                Ok(None) => {
                    tracing::info!("client disconnected; pausing");
                    peer = None;
                    pacer = None;
                }
                Err(e) => {
                    tracing::warn!("peer failed: {e:#}; dropping it");
                    peer = None;
                    pacer = None;
                }
            }
        }

        let running = peer.as_ref().is_some_and(|p| p.connected);
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
