//! End to end: `sovemu-worker serve` streaming `SovEmu.TestCore` to a WebRTC client.
//!
//! The client is str0m in this process, doing what the browser test page does: offer a
//! receive-only video and audio track plus an unordered, no-retransmit `input` DataChannel,
//! POST the offer, apply the answer, then receive VP8 and Opus over a real UDP socket on
//! loopback while sending input packets and collecting their acks.
//!
//! Set `SOVEMU_E2E_HTTP=host:port` to run the streaming test against an already running
//! `sovemu-worker serve` instead. Against `sovemu-server` (for example `docker compose up`),
//! also set `SOVEMU_E2E_LOGIN=user:password` and `SOVEMU_E2E_GAME=<game id>`: the test signs in,
//! starts that game and sends its offer to the session.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream, UdpSocket};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use sovemu_media::{OpusDecoder, Vp8Decoder};
use sovemu_protocol::{INPUT_CHANNEL, InputAck, InputPacket, SdpKind, SessionDescription};
use str0m::change::SdpAnswer;
use str0m::channel::{ChannelConfig, Reliability};
use str0m::media::{Direction, MediaKind};
use str0m::net::{Protocol, Receive};
use str0m::{Candidate, Event, Input, Output, RtcConfig};

/// Builds the test core and returns the path of its shared library (see the RetroHost tests).
fn test_core() -> PathBuf {
    if let Some(path) = std::env::var_os("SOVEMU_TEST_CORE") {
        return PathBuf::from(path);
    }
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let status = Command::new(cargo)
        .args(["build", "--quiet", "-p", "sovemu-test-core"])
        .current_dir(&workspace)
        .status()
        .expect("failed to run cargo to build sovemu-test-core");
    assert!(status.success(), "building sovemu-test-core failed");
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map_or_else(|| workspace.join("target"), PathBuf::from);
    target.join("debug").join(format!(
        "{}sovemu_test_core{}",
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_SUFFIX
    ))
}

/// The worker process, killed when dropped.
struct Worker {
    /// None when testing an external server.
    child: Option<Child>,
    http: SocketAddr,
    dir: PathBuf,
    /// The session cookie, when talking to sovemu-server.
    cookie: Option<String>,
}

impl Worker {
    fn start() -> Self {
        Self::start_with(&[])
    }

    /// The server named by `SOVEMU_E2E_HTTP`, or a freshly started worker.
    fn start_or_external() -> Self {
        match std::env::var("SOVEMU_E2E_HTTP") {
            Ok(addr) => Self {
                child: None,
                http: addr.parse().expect("SOVEMU_E2E_HTTP is host:port"),
                dir: PathBuf::new(),
                cookie: None,
            },
            Err(_) => Self::start(),
        }
    }

    fn start_with(extra: &[&std::ffi::OsStr]) -> Self {
        let dir = std::env::temp_dir().join(format!("sovemu-serve-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_sovemu-worker"))
            .arg("serve")
            .arg("--core")
            .arg(test_core())
            .args(["--http", "127.0.0.1:0", "--udp", "127.0.0.1:0"])
            .arg("--system-dir")
            .arg(dir.join("system"))
            .arg("--save-dir")
            .arg(dir.join("saves"))
            .args(extra)
            .env_remove("SOVEMU_ADVERTISE_IP")
            .env_remove("SOVEMU_WEB_DIR")
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("start sovemu-worker serve");

        // The worker prints `http=<addr>` and `udp=<addr>` once it is listening.
        let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
        let mut http = None;
        for line in lines.by_ref() {
            let line = line.unwrap();
            if let Some(addr) = line.strip_prefix("http=") {
                http = Some(addr.parse().unwrap());
            }
            if line.starts_with("udp=") {
                break;
            }
        }
        std::thread::spawn(move || lines.for_each(drop));
        Self {
            child: Some(child),
            http: http.expect("worker printed its HTTP address"),
            dir,
            cookie: None,
        }
    }

    /// Plain HTTP/1.1 so the test needs no HTTP client crate.
    fn request(&mut self, method: &str, path: &str, body: &str) -> (u16, String) {
        let mut stream = TcpStream::connect(self.http).unwrap();
        let cookie = self.cookie.as_ref().map(|c| format!("Cookie: {c}\r\n")).unwrap_or_default();
        write!(
            stream,
            "{method} {path} HTTP/1.1\r\nHost: test\r\nContent-Type: application/json\r\n\
             {cookie}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        let (head, body) = response.split_once("\r\n\r\n").unwrap();
        if let Some(set) = head.lines().find_map(|l| l.strip_prefix("set-cookie: ")) {
            self.cookie = Some(set.split(';').next().unwrap().to_owned());
        }
        let status = head.split(' ').nth(1).unwrap().parse().unwrap();
        (status, body.to_owned())
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        if let Some(child) = &mut self.child {
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
}

#[test]
fn serves_the_web_client() {
    let site = std::env::temp_dir().join(format!("sovemu-site-{}", std::process::id()));
    std::fs::create_dir_all(site.join("_astro")).unwrap();
    std::fs::write(site.join("index.html"), "<title>Play · SovEmu</title>").unwrap();
    std::fs::write(site.join("_astro/app.123.js"), "export {}").unwrap();
    let mut worker = Worker::start_with(&["--web-dir".as_ref(), site.as_os_str()]);

    let (status, body) = worker.request("GET", "/", "");
    assert_eq!((status, body.as_str()), (200, "<title>Play · SovEmu</title>"));
    assert_eq!(worker.request("GET", "/_astro/app.123.js", "").0, 200);
    assert_eq!(worker.request("GET", "/../Cargo.toml", "").0, 404);
    assert_eq!(worker.request("GET", "/api/nope", "").0, 404);
    std::fs::remove_dir_all(&site).ok();
}

#[test]
fn rejects_bad_offers_and_explains_a_missing_web_client() {
    let mut worker = Worker::start();
    let (status, body) = worker.request("GET", "/", "");
    assert_eq!(status, 200);
    assert!(body.contains("web client is not installed"));
    assert_eq!(worker.request("POST", "/api/offer", "not json").0, 400);
    let answer_as_offer = r#"{"type":"answer","sdp":"v=0"}"#;
    assert_eq!(worker.request("POST", "/api/offer", answer_as_offer).0, 400);
    assert_eq!(worker.request("POST", "/api/offer", r#"{"type":"offer","sdp":"x"}"#).0, 400);
    assert_eq!(worker.request("GET", "/nope", "").0, 404);
}

#[test]
fn streams_video_and_audio_and_acknowledges_input() {
    str0m::crypto::from_feature_flags().install_process_default();
    let mut worker = Worker::start_or_external();

    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    let local = socket.local_addr().unwrap();
    let mut rtc = RtcConfig::new().build(Instant::now());
    rtc.add_local_candidate(Candidate::host(local, "udp").unwrap());

    let mut api = rtc.sdp_api();
    let video_mid = api.add_media(MediaKind::Video, Direction::RecvOnly, None, None, None);
    let audio_mid = api.add_media(MediaKind::Audio, Direction::RecvOnly, None, None, None);
    let input = api.add_channel_with_config(ChannelConfig {
        label: INPUT_CHANNEL.into(),
        ordered: false,
        reliability: Reliability::MaxRetransmits { retransmits: 0 },
        ..ChannelConfig::default()
    });
    let (offer, pending) = api.apply().expect("an offer");

    let request = SessionDescription { kind: SdpKind::Offer, sdp: offer.to_sdp_string() };
    let offer_path = match (std::env::var("SOVEMU_E2E_LOGIN"), std::env::var("SOVEMU_E2E_GAME")) {
        (Ok(login), Ok(game)) => {
            let (user, pass) = login.split_once(':').expect("SOVEMU_E2E_LOGIN is user:password");
            let creds = serde_json::json!({"username": user, "password": pass}).to_string();
            assert_eq!(worker.request("POST", "/api/auth/login", &creds).0, 200, "sign in");
            let start = format!(r#"{{"game_id":{game}}}"#);
            let (status, body) = worker.request("POST", "/api/sessions", &start);
            assert_eq!(status, 201, "{body}");
            let session: serde_json::Value = serde_json::from_str(&body).unwrap();
            eprintln!("started {} on {}", session["title"], session["core"]);
            format!("/api/sessions/{}/offer", session["id"].as_str().unwrap())
        }
        _ => "/api/offer".to_owned(),
    };
    let (status, body) =
        worker.request("POST", &offer_path, &serde_json::to_string(&request).unwrap());
    assert_eq!(status, 200, "{body}");
    let answer: SessionDescription = serde_json::from_str(&body).unwrap();
    assert_eq!(answer.kind, SdpKind::Answer);
    assert!(answer.sdp.contains("VP8"), "answer negotiates VP8");
    assert!(answer.sdp.to_lowercase().contains("opus"), "answer negotiates Opus");
    rtc.sdp_api().accept_answer(pending, SdpAnswer::from_sdp_string(&answer.sdp).unwrap()).unwrap();

    let mut vp8 = Vp8Decoder::new().unwrap();
    let mut opus = OpusDecoder::new().unwrap();
    let (mut video_frames, mut decoded, mut audio_packets, mut audio_samples) = (0, 0, 0, 0);
    let mut picture_size = None;
    let mut channel_open = false;
    let mut acks: Vec<InputAck> = Vec::new();
    let mut seq = 0u32;
    let mut last_input = Instant::now();
    let started = Instant::now();
    let mut buf = vec![0u8; 2048];

    while started.elapsed() < Duration::from_secs(15) {
        if video_frames >= 90 && audio_packets >= 100 && acks.len() >= 3 {
            break;
        }
        let timeout = match rtc.poll_output().unwrap() {
            Output::Transmit(t) => {
                socket.send_to(&t.contents, t.destination).unwrap();
                continue;
            }
            Output::Event(event) => {
                match event {
                    Event::ChannelOpen(id, _) if id == input => channel_open = true,
                    Event::MediaData(data) if data.mid == video_mid => {
                        video_frames += 1;
                        if let Ok(Some(pic)) = vp8.decode(&data.data) {
                            decoded += 1;
                            picture_size = Some((pic.width, pic.height));
                        }
                    }
                    Event::MediaData(data) if data.mid == audio_mid => {
                        audio_packets += 1;
                        audio_samples += opus.decode(&data.data).unwrap().len() / 2;
                    }
                    Event::ChannelData(data) if data.id == input => {
                        acks.push(InputAck::decode(&data.data).expect("a valid ack"));
                    }
                    _ => {}
                }
                continue;
            }
            Output::Timeout(t) => t,
        };

        if channel_open && last_input.elapsed() > Duration::from_millis(50) {
            seq += 1;
            let packet = InputPacket { seq, client_time_ms: seq * 10, ports: vec![1 << 8] };
            rtc.channel(input).unwrap().write(true, &packet.encode()).unwrap();
            last_input = Instant::now();
            continue;
        }

        let wait = timeout
            .saturating_duration_since(Instant::now())
            .clamp(Duration::from_millis(1), Duration::from_millis(10));
        socket.set_read_timeout(Some(wait)).unwrap();
        let input = match socket.recv_from(&mut buf) {
            Ok((n, source)) => Input::Receive(
                Instant::now(),
                Receive {
                    proto: Protocol::Udp,
                    source,
                    destination: local,
                    contents: buf[..n].try_into().unwrap(),
                },
            ),
            Err(_) => Input::Timeout(Instant::now()),
        };
        rtc.handle_input(input).unwrap();
    }

    assert!(video_frames >= 90, "received {video_frames} video frames");
    assert!(decoded >= 60, "decoded {decoded} of {video_frames} video frames");
    let (w, h) = picture_size.expect("a decoded picture");
    eprintln!(
        "decoded {decoded}/{video_frames} video frames at {w}x{h}, {audio_packets} audio packets, {} acks",
        acks.len()
    );
    if std::env::var_os("SOVEMU_E2E_HTTP").is_none() {
        assert_eq!((w, h), (256, 224), "the test core's native resolution");
    }
    assert!(audio_packets >= 100, "received {audio_packets} audio packets");
    assert_eq!(audio_samples, audio_packets * 480, "10 ms Opus packets");
    assert!(acks.len() >= 3, "received {} input acks", acks.len());
    for ack in &acks {
        assert!(ack.seq >= 1 && ack.seq <= seq);
        assert_eq!(ack.client_time_ms, ack.seq * 10, "client time echoed");
    }
    assert!(acks.windows(2).all(|w| w[0].frame < w[1].frame), "acks follow the frames");
}
