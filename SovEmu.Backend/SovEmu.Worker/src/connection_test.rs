//! `sovemu-worker connection-test`: a WebRTC peer with no media, on one of the game ports, so an
//! admin can check from a browser that it reaches the server's streams (the UDP port, through
//! any STUN or TURN server the clients are given) before anyone tries to play.
//!
//! It is answered exactly as a game is: full ICE, a host candidate at the advertised address (and
//! the IPv6 one, where the host has it), the public address STUN finds, the server's offer and answer in JSON lines ([`sovemu_protocol::ipc`]): one
//! [`Request::Offer`] on stdin, one [`Reply::Answer`] (or [`Reply::Error`]) on stdout. Whatever
//! the browser sends on a data channel comes straight back, so it can time the round trip
//! through the same path a game's input takes. The browser reads the path itself (direct, or
//! through a relay) from its own WebRTC statistics.
//!
//! It exits when the browser disconnects, when stdin closes (the server went away), or after
//! `--timeout` seconds, whichever is first.

use std::io::{BufRead, Write};
use std::net::UdpSocket;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use sovemu_protocol::ipc::{Reply, Request};
use sovemu_protocol::{SdpKind, SessionDescription};
use str0m::change::SdpOffer;
use str0m::net::{Protocol, Receive};
use str0m::{Event, IceConnectionState, Input, Output, Rtc, RtcConfig};

use crate::ConnectionTestArgs;
use crate::stream::Hosts;
use crate::udp::Udp;

pub(crate) fn run(args: ConnectionTestArgs) -> Result<()> {
    str0m::crypto::from_feature_flags().install_process_default();
    let socket = UdpSocket::bind(args.udp).with_context(|| format!("binding {}", args.udp))?;
    let advertised = crate::stream::advertised_addr(args.advertise, socket.local_addr()?);
    let udp = Udp::new(socket, advertised).with_ipv6(args.advertise_ipv6, 0)?;
    // Asked before the offer is read, so the answer is usually back by then.
    let mut reflexive = crate::stun::Reflexive::new(&args.stun, udp.primary());
    reflexive.ask(udp.primary());

    let mut stdin = std::io::stdin().lock();
    let mut line = String::new();
    stdin.read_line(&mut line).context("reading the offer")?;
    let Ok(Request::Offer { id, offer, .. }) = serde_json::from_str::<Request>(&line) else {
        bail!("expected an offer on stdin, got {:?}", line.trim());
    };
    let public = reflexive.current(&udp);
    let mut rtc = match answer(&offer, Hosts::of(&udp), public) {
        Ok((rtc, answer)) => {
            reply(&Reply::Answer { id, answer });
            rtc
        }
        Err(e) => {
            reply(&Reply::Error { id, message: format!("{e:#}") });
            return Err(e);
        }
    };
    drop(stdin);

    // The server closing stdin (it stopped, or gave up on the test) ends it at once.
    let server_gone = Arc::new(AtomicBool::new(false));
    let gone = server_gone.clone();
    std::thread::Builder::new().name("stdin".into()).spawn(move || {
        for _ in std::io::stdin().lock().lines() {}
        gone.store(true, Ordering::Relaxed);
    })?;

    let until = Instant::now() + Duration::from_secs(args.timeout);
    let ended = echo(&mut rtc, &udp, until, &server_gone)?;
    tracing::info!(advertised = %advertised, ipv6 = ?udp.advertised_v6(), ended, "connection test ended");
    Ok(())
}

/// A peer for `offer` with no media, and its SDP answer.
fn answer(
    offer: &SessionDescription,
    hosts: Hosts,
    public: Option<std::net::SocketAddr>,
) -> Result<(Rtc, SessionDescription)> {
    let offer = SdpOffer::from_sdp_string(&offer.sdp).map_err(|e| anyhow!("bad offer: {e}"))?;
    // As a game session's: full ICE, and its public address beside its own.
    let mut rtc = RtcConfig::new().clear_codecs().build(Instant::now());
    crate::stream::add_candidates(&mut rtc, hosts, public)?;
    let answer = rtc.sdp_api().accept_offer(offer)?;
    Ok((rtc, SessionDescription { kind: SdpKind::Answer, sdp: answer.to_sdp_string() }))
}

/// Echoes every data channel message until the browser leaves, the server goes, or `until`;
/// says which.
fn echo(
    rtc: &mut Rtc,
    socket: &Udp,
    until: Instant,
    server_gone: &AtomicBool,
) -> Result<&'static str> {
    let mut buf = vec![0u8; 2048];
    loop {
        // Everything the peer wants to send or say, up to its next deadline.
        let deadline = loop {
            if !rtc.is_alive() {
                return Ok("closed");
            }
            match rtc.poll_output()? {
                Output::Timeout(t) => break t,
                Output::Transmit(t) => {
                    if let Err(e) = socket.send_to(&t.contents, t.destination) {
                        tracing::debug!(destination = %t.destination, error = %e, "UDP send failed");
                    }
                }
                Output::Event(Event::Connected) => tracing::info!("browser connected"),
                Output::Event(Event::IceConnectionStateChange(
                    IceConnectionState::Disconnected,
                )) => return Ok("disconnected"),
                Output::Event(Event::ChannelData(data)) => {
                    if let Some(mut channel) = rtc.channel(data.id) {
                        let _ = channel.write(data.binary, &data.data);
                    }
                }
                Output::Event(_) => {}
            }
        };
        if Instant::now() >= until {
            return Ok("timeout");
        }
        if server_gone.load(Ordering::Relaxed) {
            return Ok("server gone");
        }
        // Wake at least every 200 ms to notice the server going.
        let wait = deadline
            .min(until)
            .saturating_duration_since(Instant::now())
            .min(Duration::from_millis(200));
        if !wait.is_zero() {
            match socket.recv(&mut buf, wait) {
                Ok(None) => {}
                Ok(Some((n, source, local))) => {
                    if let Ok(contents) = buf[..n].try_into() {
                        let input = Input::Receive(
                            Instant::now(),
                            Receive { proto: Protocol::Udp, source, destination: local, contents },
                        );
                        if rtc.accepts(&input) {
                            rtc.handle_input(input)?;
                        }
                    }
                }
                Err(e) => tracing::debug!(error = %e, "UDP receive failed"),
            }
        }
        rtc.handle_input(Input::Timeout(Instant::now()))?;
    }
}

/// Writes one reply as a JSON line on stdout.
fn reply(reply: &Reply) {
    let line = serde_json::to_string(reply).expect("replies serialise");
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{line}").and_then(|()| out.flush());
}
