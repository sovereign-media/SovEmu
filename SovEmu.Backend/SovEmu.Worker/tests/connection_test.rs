//! `sovemu-worker connection-test` over its real stdin/stdout and a real UDP socket: a str0m
//! client offers a data channel, as the admin's browser does, connects, and gets back what it
//! sends.

use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, UdpSocket};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use sovemu_protocol::ipc::{Reply, Request};
use sovemu_protocol::{SdpKind, SessionDescription};
use str0m::change::SdpAnswer;
use str0m::net::{Protocol, Receive};
use str0m::{Candidate, Event, Input, Output, RtcConfig};

/// Starts the test peer, sends it `offer` and returns its reply, with the process.
fn start(offer: SessionDescription) -> (std::process::Child, Reply) {
    start_with(offer, &[])
}

/// As [`start`], with more arguments.
fn start_with(offer: SessionDescription, extra: &[&str]) -> (std::process::Child, Reply) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_sovemu-worker"))
        .args(["connection-test", "--udp", "127.0.0.1:0", "--timeout", "20"])
        .args(extra)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("start sovemu-worker");
    let request = serde_json::to_string(&Request::Offer { id: 7, offer, codec: None }).unwrap();
    writeln!(child.stdin.as_mut().unwrap(), "{request}").unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap()).read_line(&mut line).unwrap();
    (child, serde_json::from_str(&line).expect("a reply"))
}

#[test]
fn offers_the_address_the_router_forwards() {
    str0m::crypto::from_feature_flags().install_process_default();
    let mut rtc = RtcConfig::new().build(Instant::now());
    rtc.add_local_candidate(Candidate::host("127.0.0.1:9".parse().unwrap(), "udp").unwrap());
    let mut api = rtc.sdp_api();
    api.add_channel("connection-test".into());
    let (offer, _) = api.apply().expect("an offer");
    let (mut child, reply) = start_with(
        SessionDescription { kind: SdpKind::Offer, sdp: offer.to_sdp_string() },
        &["--public", "203.0.113.9:51000"],
    );
    let Reply::Answer { answer, .. } = reply else { panic!("expected an answer: {reply:?}") };
    let srflx: Vec<&str> = answer.sdp.lines().filter(|l| l.contains("typ srflx")).collect();
    assert_eq!(srflx.len(), 1, "{}", answer.sdp);
    assert!(srflx[0].contains("203.0.113.9 51000 typ srflx"), "{}", srflx[0]);
    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn echoes_a_browsers_data_channel_on_a_game_port() {
    echoes_over(UdpSocket::bind("127.0.0.1:0").unwrap(), &[]);
}

/// A player with IPv6 only reaches the session at its IPv6 address, on the same port: here
/// loopback's, given as the setting would give a global one.
#[test]
fn echoes_over_ipv6_on_the_same_port() {
    let Ok(socket) = UdpSocket::bind("[::1]:0") else {
        eprintln!("no IPv6 loopback here: skipped");
        return;
    };
    let answer = echoes_over(socket, &["--advertise-ipv6", "::1"]);
    let ports: Vec<&str> = answer
        .lines()
        .filter_map(|l| l.strip_prefix("a=candidate:"))
        .filter_map(|c| c.split(' ').nth(5))
        .collect();
    assert!(answer.contains(" ::1 "), "an IPv6 host candidate:\n{answer}");
    assert!(ports.len() >= 2 && ports.iter().all(|p| *p == ports[0]), "one port: {ports:?}");
}

/// Connects from `socket` (its address the only candidate offered), sends a message and gets it
/// back; returns the answer's SDP.
fn echoes_over(socket: UdpSocket, extra: &[&str]) -> String {
    str0m::crypto::from_feature_flags().install_process_default();
    let local = socket.local_addr().unwrap();
    let mut rtc = RtcConfig::new().build(Instant::now());
    rtc.add_local_candidate(Candidate::host(local, "udp").unwrap());
    let mut api = rtc.sdp_api();
    let channel = api.add_channel("connection-test".into());
    let (offer, pending) = api.apply().expect("an offer");

    let (mut child, reply) =
        start_with(SessionDescription { kind: SdpKind::Offer, sdp: offer.to_sdp_string() }, extra);
    let Reply::Answer { id: 7, answer } = reply else { panic!("expected an answer: {reply:?}") };
    assert_eq!(answer.kind, SdpKind::Answer);
    assert!(!answer.sdp.contains("m=video") && !answer.sdp.contains("m=audio"), "no media");
    rtc.sdp_api().accept_answer(pending, SdpAnswer::from_sdp_string(&answer.sdp).unwrap()).unwrap();

    let started = Instant::now();
    let mut buf = vec![0u8; 2048];
    let mut sent = false;
    let echoed = loop {
        assert!(started.elapsed() < Duration::from_secs(10), "no echo within 10 s");
        let timeout = match rtc.poll_output().unwrap() {
            Output::Transmit(t) => {
                socket.send_to(&t.contents, t.destination).unwrap();
                continue;
            }
            Output::Event(Event::ChannelOpen(id, _)) if id == channel && !sent => {
                rtc.channel(channel).unwrap().write(false, b"ping 1").unwrap();
                sent = true;
                continue;
            }
            Output::Event(Event::ChannelData(data)) if data.id == channel => break data,
            Output::Event(_) => continue,
            Output::Timeout(t) => t,
        };
        let wait = timeout
            .saturating_duration_since(Instant::now())
            .clamp(Duration::from_millis(1), Duration::from_millis(20));
        socket.set_read_timeout(Some(wait)).unwrap();
        if let Ok((n, source)) = socket.recv_from(&mut buf) {
            let receive = |source: SocketAddr, contents| Receive {
                proto: Protocol::Udp,
                source,
                destination: local,
                contents,
            };
            rtc.handle_input(Input::Receive(
                Instant::now(),
                receive(source, buf[..n].try_into().unwrap()),
            ))
            .unwrap();
        }
        rtc.handle_input(Input::Timeout(Instant::now())).unwrap();
    };
    assert_eq!((echoed.binary, echoed.data.as_slice()), (false, b"ping 1".as_slice()));

    // Closing stdin (the server going away) ends it at once, well before its timeout.
    drop(child.stdin.take());
    let waited = Instant::now();
    while child.try_wait().unwrap().is_none() {
        assert!(waited.elapsed() < Duration::from_secs(5), "still running after stdin closed");
        std::thread::sleep(Duration::from_millis(50));
    }
    answer.sdp
}

#[test]
fn says_why_it_cannot_answer_a_bad_offer() {
    let (mut child, reply) =
        start(SessionDescription { kind: SdpKind::Offer, sdp: "not an offer".into() });
    let Reply::Error { id: 7, message } = reply else { panic!("expected an error: {reply:?}") };
    assert!(message.contains("bad offer"), "{message}");
    assert!(!child.wait().unwrap().success());
}
