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
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use sovemu_media::{H264Decoder, OpusDecoder};
use sovemu_protocol::{
    Hotkeys, INPUT_CHANNEL, InputAck, InputPacket, Link, Pipeline, SdpKind, SessionDescription,
};
use str0m::change::SdpAnswer;
use str0m::channel::{ChannelConfig, Reliability};
use str0m::media::MediaTime;
use str0m::media::{Direction, MediaKind};
use str0m::net::{Protocol, Receive};
use str0m::rtp::Extension;
use str0m::{Candidate, Event, Input, Output, RtcConfig};

/// Builds the test core and returns the path of its shared library (see the RetroHost tests).
#[path = "../../SovEmu.TestCore/locate.rs"]
mod locate;
use locate::test_core;

/// The worker process, killed when dropped.
struct Worker {
    /// None when testing an external server.
    child: Option<Child>,
    http: SocketAddr,
    dir: PathBuf,
    /// The session cookie, when talking to sovemu-server.
    cookie: Option<String>,
    /// What it prints on stdout after its addresses (`exit=<reason>` as it ends).
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    stdout: Option<std::sync::mpsc::Receiver<String>>,
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
                stdout: None,
            },
            Err(_) => Self::start(),
        }
    }

    fn start_with(extra: &[&std::ffi::OsStr]) -> Self {
        Self::start_with_env(extra, &[])
    }

    /// With `env` set for the worker (and so for its core) too.
    fn start_with_env(extra: &[&std::ffi::OsStr], env: &[(&str, &str)]) -> Self {
        Self::start_core(test_core(), extra, env)
    }

    /// Running `core` rather than the test core.
    fn start_core(
        core: &std::path::Path,
        extra: &[&std::ffi::OsStr],
        env: &[(&str, &str)],
    ) -> Self {
        // A folder of its own: the tests run side by side, and each deletes its folder when done.
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("sovemu-serve-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_sovemu-worker"))
            .arg("serve")
            .arg("--core")
            .arg(core)
            .args(["--http", "127.0.0.1:0", "--udp", "127.0.0.1:0"])
            .arg("--system-dir")
            .arg(dir.join("system"))
            .arg("--save-dir")
            .arg(dir.join("saves"))
            .args(extra)
            .envs(env.iter().copied())
            .env_remove("SovEmu__Streaming__AdvertiseAddress")
            .env_remove("SovEmu__Streaming__AdvertiseAddressV6")
            .env_remove("SovEmu__Paths__Web")
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
        let (tx, stdout) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for line in lines.map_while(Result::ok) {
                let _ = tx.send(line);
            }
        });
        Self {
            child: Some(child),
            http: http.expect("worker printed its HTTP address"),
            dir,
            cookie: None,
            stdout: Some(stdout),
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
    std::fs::create_dir_all(site.join("assets")).unwrap();
    std::fs::write(site.join("index.html"), "<title>Play · SovEmu</title>").unwrap();
    std::fs::write(site.join("assets/app.123.js"), "export {}").unwrap();
    let mut worker = Worker::start_with(&["--web-dir".as_ref(), site.as_os_str()]);

    let (status, body) = worker.request("GET", "/", "");
    assert_eq!((status, body.as_str()), (200, "<title>Play · SovEmu</title>"));
    assert_eq!(worker.request("GET", "/assets/app.123.js", "").0, 200);
    assert_eq!(worker.request("GET", "/play/", "").0, 200, "a screen gets the one page");
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

/// The id the test client offers playout delay at, unlike the worker's own, so the answer shows
/// that the worker took the client's.
const PLAYOUT_DELAY_ID: u8 = 9;

#[test]
fn streams_video_and_audio_and_acknowledges_input() {
    str0m::crypto::from_feature_flags().install_process_default();
    let mut worker = Worker::start_or_external();

    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    // Room for a burst: on a loaded machine (a CI runner) this loop falls behind the stream, and
    // a small receive buffer then drops packets until hardly a frame arrives whole.
    socket2::SockRef::from(&socket).set_recv_buffer_size(4 << 20).ok();
    let local = socket.local_addr().unwrap();
    // Offers playout delay as libwebrtc does (Android, iOS, browsers), at an id of its own, and
    // redundant audio (RED) as Chrome does. str0m unwraps RED, so audio arrives as plain Opus.
    let mut rtc = RtcConfig::new()
        .set_extension(PLAYOUT_DELAY_ID, Extension::PlayoutDelay)
        .enable_opus(true, true)
        .build(Instant::now());
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
    assert!(answer.sdp.contains("H264"), "answer negotiates H.264");
    assert!(answer.sdp.to_lowercase().contains("opus"), "answer negotiates Opus");
    let red_pt: u8 = answer
        .sdp
        .lines()
        .find_map(|l| l.strip_prefix("a=rtpmap:")?.strip_suffix(" red/48000/2"))
        .expect("answer negotiates redundant audio")
        .parse()
        .unwrap();
    let playout_delay = format!(
        "a=extmap:{PLAYOUT_DELAY_ID} http://www.webrtc.org/experiments/rtp-hdrext/playout-delay"
    );
    assert!(answer.sdp.contains(&playout_delay), "answer takes the offered playout delay id");
    rtc.sdp_api().accept_answer(pending, SdpAnswer::from_sdp_string(&answer.sdp).unwrap()).unwrap();

    let mut h264 = H264Decoder::new().unwrap();
    let mut opus = OpusDecoder::new().unwrap();
    let (mut video_frames, mut decoded, mut audio_packets, mut audio_samples) = (0, 0, 0, 0);
    // Video packets that ask to be shown at once (playout delay 0/0), so no jitter buffer.
    let mut zero_delay_frames = 0;
    let mut picture_size = None;
    let mut pipeline = None;
    // The check of the connection before the stream starts (#243): its result, and the video
    // frames received by the time it was done.
    let mut link: Option<Link> = None;
    let mut video_before_link = None;
    let mut channel_open = false;
    let mut acks: Vec<InputAck> = Vec::new();
    // Video frames received when each ack arrived: with the ack's emulated frame, how many frames
    // were emulated per frame shown.
    let mut acks_video: Vec<usize> = Vec::new();
    let mut seq = 0u32;
    let mut last_input = Instant::now();
    let started = Instant::now();
    let mut buf = vec![0u8; 2048];
    // RTP packets on the RED payload type: the header is not encrypted, so the wire shows the
    // audio going out wrapped. RTCP (packet types 192-223) shares the first byte's version bits.
    let mut red_packets = 0usize;
    let mut count_red = |packet: &[u8]| {
        if packet.len() >= 12
            && packet[0] >> 6 == 2
            && !(192..=223).contains(&packet[1])
            && packet[1] & 0x7f == red_pt
        {
            red_packets += 1;
        }
    };

    // After the basics, hold fast-forward for a while and see more frames emulated per frame shown.
    // A tick shows one frame however slow the machine is, and runs one emulated frame normally or
    // several fast-forwarding, so this ratio does not depend on the machine's speed (a rate in
    // frames per second does: a loaded CI runner managed under 1.5 times the normal rate).
    let ff_window = Duration::from_millis(1000);
    // How long fast-forward gets to show in the latest acks before the test gives up on it.
    let ff_deadline = Duration::from_secs(6);
    // When fast-forward began, and how many acks had arrived by then.
    let mut ff_start: Option<(Instant, usize)> = None;
    let mut normal_ratio = 0.0f64;
    let mut ff_ratio = 0.0f64;
    // Then pause (the client's menu) and see the frames stop.
    let mut pause_start: Option<(Instant, usize)> = None;
    // The current 300 ms window while paused: when it began, and the frames seen by then.
    let mut pause_window: Option<(Instant, usize)> = None;
    let mut paused_frames = usize::MAX;
    while started.elapsed() < Duration::from_secs(25) {
        // Normal play is measured as fast-forward is, over the latest acks once the stream is
        // steady, never from the first ack: while the stream starts, the game emulates frames the
        // test has not received yet, and a ratio from the start read 2.2 and 2.3 in CI though the
        // stream was normal, which put the bar for fast-forward (a true 4.0) at twice that.
        if ff_start.is_none()
            && video_frames >= 90
            && audio_packets >= 100
            && acks.len() > STARTUP_ACKS + RATIO_ACKS
        {
            let recent = acks.len() - (RATIO_ACKS + 1);
            normal_ratio = frames_per_shown(&acks, &acks_video, recent, acks.len());
            ff_start = Some((Instant::now(), acks.len()));
        }
        // Enough acks to measure past those first ones, however slowly they arrive (a 100 ms
        // heartbeat: a loaded CI runner had too few after a second, and measured nothing). The
        // ratio is taken over the latest acks only, and again until fast-forward shows in it or
        // its deadline passes: on a loaded runner, frames emulated before the hotkey took hold
        // can still be arriving a second later, and a ratio over every ack since then stayed
        // near 1 (0.9 and 0.0 against 1.0 in CI) though fast-forward worked.
        if let Some((at, from)) = ff_start
            && pause_start.is_none()
            && at.elapsed() >= ff_window
            && acks.len() >= from + 4 + RATIO_ACKS
        {
            let recent = acks.len().saturating_sub(RATIO_ACKS + 1).max(from + 4);
            ff_ratio = frames_per_shown(&acks, &acks_video, recent, acks.len());
            if ff_ratio >= normal_ratio * 2.0 || at.elapsed() >= ff_deadline {
                pause_start = Some((Instant::now(), video_frames));
            }
        }
        // Paused (the client's menu): frames stop. Frames sent before the pause can still be
        // arriving for a while when this test falls behind (a loaded CI machine), so it waits for
        // quiet: any 300 ms with at most one frame, within 5 s. A paused game goes quiet once that
        // backlog has drained; a running one (18 frames per 300 ms) never does.
        if let Some((at, _)) = pause_start {
            let (since, frames) = *pause_window.get_or_insert((at, video_frames));
            if since.elapsed() >= Duration::from_millis(300) {
                paused_frames = video_frames - frames;
                if paused_frames <= 1 || at.elapsed() >= Duration::from_secs(5) {
                    break;
                }
                pause_window = Some((Instant::now(), video_frames));
            }
        }
        // Input and hotkeys go out every 50 ms whatever is queued, as a client's timer sends them:
        // checked here, before the events, so a backlog of frames cannot hold them back (the
        // worker rightly lets a pause lapse after 500 ms without one).
        if channel_open && last_input.elapsed() > Duration::from_millis(50) {
            seq += 1;
            let packet = InputPacket { seq, client_time_ms: seq * 10, ports: vec![1 << 8] };
            let mut channel = rtc.channel(input).unwrap();
            channel.write(true, &packet.encode()).unwrap();
            if pause_start.is_some() {
                let pause = Hotkeys { pause: true, ..Hotkeys::default() };
                channel.write(true, &pause.encode()).unwrap();
            } else if ff_start.is_some() {
                let ff = Hotkeys { fast_forward: true, ..Hotkeys::default() };
                channel.write(true, &ff.encode()).unwrap();
            }
            last_input = Instant::now();
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
                        let zero = Some(MediaTime::from_millis(0));
                        if data.ext_vals.play_delay_min == zero
                            && data.ext_vals.play_delay_max == zero
                        {
                            zero_delay_frames += 1;
                        }
                        if let Ok(Some(pic)) = h264.decode(&data.data) {
                            decoded += 1;
                            picture_size = Some((pic.width, pic.height));
                        }
                    }
                    Event::MediaData(data) if data.mid == audio_mid => {
                        audio_packets += 1;
                        audio_samples += opus.decode(&data.data).unwrap().len() / 2;
                    }
                    Event::ChannelData(data) if data.id == input => {
                        if data.data.first() == Some(&0x82) {
                            pipeline =
                                Some(Pipeline::decode(&data.data).expect("a valid pipeline"));
                        } else if data.data.first() == Some(&0x83) {
                            let l = Link::decode(&data.data).expect("a valid link message");
                            if l.state == Link::DONE && link.is_none() {
                                video_before_link = Some(video_frames);
                                link = Some(l);
                            }
                        } else {
                            acks.push(InputAck::decode(&data.data).expect("a valid ack"));
                            acks_video.push(video_frames);
                        }
                    }
                    _ => {}
                }
                continue;
            }
            Output::Timeout(t) => t,
        };

        let wait = timeout
            .saturating_duration_since(Instant::now())
            .clamp(Duration::from_millis(1), Duration::from_millis(10));
        socket.set_read_timeout(Some(wait)).unwrap();
        fn receive(n: usize, source: SocketAddr, local: SocketAddr, buf: &[u8]) -> Input<'_> {
            Input::Receive(
                Instant::now(),
                Receive {
                    proto: Protocol::Udp,
                    source,
                    destination: local,
                    contents: buf[..n].try_into().unwrap(),
                },
            )
        }
        match socket.recv_from(&mut buf) {
            Ok((n, source)) => {
                count_red(&buf[..n]);
                rtc.handle_input(receive(n, source, local, &buf)).unwrap();
                // Take whatever else has queued up before going back to the events, so a
                // backlog drains in one go rather than a packet a turn.
                socket.set_nonblocking(true).unwrap();
                while let Ok((n, source)) = socket.recv_from(&mut buf) {
                    count_red(&buf[..n]);
                    rtc.handle_input(receive(n, source, local, &buf)).unwrap();
                }
                socket.set_nonblocking(false).unwrap();
            }
            Err(_) => rtc.handle_input(Input::Timeout(Instant::now())).unwrap(),
        }
    }

    assert!(video_frames >= 90, "received {video_frames} video frames");
    assert!(decoded >= 60, "decoded {decoded} of {video_frames} video frames");
    assert_eq!(zero_delay_frames, video_frames, "every video frame asks for no playout delay");
    let (w, h) = picture_size.expect("a decoded picture");
    eprintln!(
        "decoded {decoded}/{video_frames} video frames at {w}x{h}, {audio_packets} audio packets, {} acks",
        acks.len()
    );
    if std::env::var_os("SOVEMU_E2E_HTTP").is_none() {
        assert_eq!((w, h), (256, 224), "the test core's native resolution");
    }
    // No picture before the connection was checked: the client's loading screen stays up
    // while it is, and the stream starts at the rate it found.
    let link = link.expect("the worker said when the connection check was done");
    assert_eq!(video_before_link, Some(0), "video sent while checking the connection: {link:?}");
    assert!(!link.stopped_by.is_empty() && link.start_kbps > 0, "{link:?}");
    let pipeline = pipeline.expect("the worker said how it makes the stream");
    assert_eq!(pipeline.link.as_ref(), Some(&link), "the pipeline carries the result again");
    assert_eq!(pipeline.video.codec, "h264");
    assert!(!pipeline.video.encoder.is_empty(), "{pipeline:?}");
    assert_eq!((pipeline.video.width, pipeline.video.height), (w, h));
    assert_eq!(pipeline.audio.codec, "opus");
    assert_eq!(pipeline.renderer, None, "the test core draws on the CPU");
    assert!(audio_packets >= 100, "received {audio_packets} audio packets");
    assert_eq!(audio_samples, audio_packets * 480, "10 ms Opus packets");
    assert!(red_packets * 10 >= audio_packets * 9, "{red_packets}/{audio_packets} audio in RED");
    assert!(acks.len() >= 3, "received {} input acks", acks.len());
    for ack in &acks {
        assert!(ack.seq >= 1 && ack.seq <= seq);
        assert_eq!(ack.client_time_ms, ack.seq * 10, "client time echoed");
    }
    assert!(acks.windows(2).all(|w| w[0].frame < w[1].frame), "acks follow the frames");
    eprintln!(
        "emulated {normal_ratio:.1} frames per frame shown normally, {ff_ratio:.1} fast-forwarding"
    );
    // Four emulated frames a tick fast-forwarding (`--fast-forward`), one normally. Frames lost on
    // the way raise both ratios alike, so this compares them rather than expecting 1 and 4.
    assert!(
        ff_ratio >= normal_ratio * 2.0,
        "fast-forward: {ff_ratio:.1} vs {normal_ratio:.1} emulated frames per frame shown"
    );
    assert!(
        paused_frames <= 1,
        "still {paused_frames} video frames per 300 ms, 5 s after pausing: the game kept running"
    );
}

#[test]
fn a_client_without_h264_gets_vp8() {
    str0m::crypto::from_feature_flags().install_process_default();
    let mut worker = Worker::start();
    let mut rtc = RtcConfig::new()
        .clear_codecs()
        .enable_vp8(true)
        .enable_opus(true, false)
        .build(Instant::now());
    rtc.add_local_candidate(Candidate::host("127.0.0.1:9".parse().unwrap(), "udp").unwrap());
    let mut api = rtc.sdp_api();
    api.add_media(MediaKind::Video, Direction::RecvOnly, None, None, None);
    let (offer, _) = api.apply().expect("an offer");
    let request = SessionDescription { kind: SdpKind::Offer, sdp: offer.to_sdp_string() };
    let (status, body) =
        worker.request("POST", "/api/offer", &serde_json::to_string(&request).unwrap());
    assert_eq!(status, 200, "{body}");
    let answer: SessionDescription = serde_json::from_str(&body).unwrap();
    assert!(answer.sdp.contains("VP8"), "answer negotiates VP8");
    assert!(!answer.sdp.contains("H264"), "and no H.264, which the client did not offer");
    assert!(!answer.sdp.contains("red/48000"), "nor redundant audio, which it did not offer");
}

/// A player with IPv6 alone reaches a game at its IPv6 address, on the session's port, and the
/// picture comes back the same way: here loopback's, given as the setting would give a global
/// address.
#[test]
fn streams_video_over_ipv6() {
    let Ok(socket) = UdpSocket::bind("[::1]:0") else {
        eprintln!("no IPv6 loopback here: skipped");
        return;
    };
    str0m::crypto::from_feature_flags().install_process_default();
    let mut worker = Worker::start_with(&["--advertise-ipv6".as_ref(), "::1".as_ref()]);
    let local = socket.local_addr().unwrap();
    let mut rtc = RtcConfig::new().build(Instant::now());
    // Its only candidate is IPv6: nothing reaches it over IPv4.
    rtc.add_local_candidate(Candidate::host(local, "udp").unwrap());
    let mut api = rtc.sdp_api();
    api.add_media(MediaKind::Video, Direction::RecvOnly, None, None, None);
    let (offer, pending) = api.apply().expect("an offer");
    let request = SessionDescription { kind: SdpKind::Offer, sdp: offer.to_sdp_string() };
    let (status, body) =
        worker.request("POST", "/api/offer", &serde_json::to_string(&request).unwrap());
    assert_eq!(status, 200, "{body}");
    let answer: SessionDescription = serde_json::from_str(&body).unwrap();
    assert!(answer.sdp.contains(" ::1 "), "an IPv6 host candidate:\n{}", answer.sdp);
    rtc.sdp_api().accept_answer(pending, SdpAnswer::from_sdp_string(&answer.sdp).unwrap()).unwrap();

    let started = Instant::now();
    let mut buf = vec![0u8; 2048];
    let mut frames = 0;
    while frames < 10 {
        assert!(started.elapsed() < Duration::from_secs(20), "{frames} video frames in 20 s");
        let timeout = match rtc.poll_output().unwrap() {
            Output::Transmit(t) => {
                socket.send_to(&t.contents, t.destination).unwrap();
                continue;
            }
            Output::Event(Event::MediaData(_)) => {
                frames += 1;
                continue;
            }
            Output::Event(_) => continue,
            Output::Timeout(t) => t,
        };
        let wait = timeout
            .saturating_duration_since(Instant::now())
            .clamp(Duration::from_millis(1), Duration::from_millis(10));
        socket.set_read_timeout(Some(wait)).unwrap();
        if let Ok((n, source)) = socket.recv_from(&mut buf) {
            assert!(source.is_ipv6(), "from {source}");
            let contents = buf[..n].try_into().unwrap();
            let receive = Receive { proto: Protocol::Udp, source, destination: local, contents };
            rtc.handle_input(Input::Receive(Instant::now(), receive)).unwrap();
        }
        rtc.handle_input(Input::Timeout(Instant::now())).unwrap();
    }
}

/// A core that leaks (the test core's `SOVEMU_TEST_LEAK_MB`) is stopped by the session's memory
/// limit as a player quitting would stop it: the stream ends and the auto state is written
/// (sovereign-media/SovEmu#282). Linux only: resident memory is read from `/proc`.
#[cfg(target_os = "linux")]
#[test]
fn a_leaking_core_is_stopped_by_its_memory_limit_and_its_saves_are_written() {
    str0m::crypto::from_feature_flags().install_process_default();
    let limit: &std::ffi::OsStr = "--memory-limit-mb".as_ref();
    let mut worker =
        Worker::start_with_env(&[limit, "256".as_ref()], &[("SOVEMU_TEST_LEAK_MB", "2")]);
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    let local = socket.local_addr().unwrap();
    let mut rtc = RtcConfig::new().build(Instant::now());
    rtc.add_local_candidate(Candidate::host(local, "udp").unwrap());
    let mut api = rtc.sdp_api();
    api.add_media(MediaKind::Video, Direction::RecvOnly, None, None, None);
    let (offer, pending) = api.apply().expect("an offer");
    let request = SessionDescription { kind: SdpKind::Offer, sdp: offer.to_sdp_string() };
    let (status, body) =
        worker.request("POST", "/api/offer", &serde_json::to_string(&request).unwrap());
    assert_eq!(status, 200, "{body}");
    let answer: SessionDescription = serde_json::from_str(&body).unwrap();
    rtc.sdp_api().accept_answer(pending, SdpAnswer::from_sdp_string(&answer.sdp).unwrap()).unwrap();

    // The game runs, and leaks, only while a client is connected: keep this one going until the
    // worker says it has ended.
    let stdout = worker.stdout.take().unwrap();
    let started = Instant::now();
    let mut buf = vec![0u8; 2048];
    let exit = loop {
        assert!(started.elapsed() < Duration::from_secs(90), "still running after 90 s");
        if let Ok(line) = stdout.try_recv()
            && let Some(reason) = line.strip_prefix("exit=")
        {
            break reason.to_owned();
        }
        let timeout = match rtc.poll_output().unwrap() {
            Output::Transmit(t) => {
                socket.send_to(&t.contents, t.destination).unwrap();
                continue;
            }
            Output::Event(_) => continue,
            Output::Timeout(t) => t,
        };
        let wait = timeout
            .saturating_duration_since(Instant::now())
            .clamp(Duration::from_millis(1), Duration::from_millis(10));
        socket.set_read_timeout(Some(wait)).unwrap();
        if let Ok((n, source)) = socket.recv_from(&mut buf) {
            let contents = buf[..n].try_into().unwrap();
            let receive = Receive { proto: Protocol::Udp, source, destination: local, contents };
            rtc.handle_input(Input::Receive(Instant::now(), receive)).unwrap();
        }
        rtc.handle_input(Input::Timeout(Instant::now())).unwrap();
    };
    assert_eq!(exit, "memory_limit");
    let auto_state = worker.dir.join("saves/states/slot0.state");
    assert!(auto_state.exists(), "no auto state at {}", auto_state.display());
}

/// With run-ahead, the picture sent is a frame ahead's and the real frame's is never drawn:
/// the stream still sends a picture every tick, and it moves.
#[test]
fn run_ahead_streams_the_picture_of_a_frame_ahead() {
    str0m::crypto::from_feature_flags().install_process_default();
    let mut worker = Worker::start_with(&["--run-ahead".as_ref(), "2".as_ref()]);
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    let mut client = VideoClient::connect(&mut worker, socket);
    let mut h264 = H264Decoder::new().unwrap();
    let mut lumas = Vec::new();
    client.receive(Duration::from_secs(30), |data| {
        if let Ok(Some(picture)) = h264.decode(&data.data) {
            lumas.push(picture.luma);
        }
        lumas.len() < 30
    });
    assert_eq!(lumas.len(), 30, "decoded {} pictures in 30 s", lumas.len());
    // The test core's gradient moves a step every frame: no two pictures in a row alike.
    assert!(lumas.windows(2).all(|w| w[0] != w[1]), "a picture repeated");
}

/// A client that receives video alone, over a socket of its own.
struct VideoClient {
    rtc: str0m::Rtc,
    socket: UdpSocket,
    local: SocketAddr,
}

impl VideoClient {
    /// Offers to receive video and applies the worker's answer.
    fn connect(worker: &mut Worker, socket: UdpSocket) -> Self {
        // Room for a burst, should the test fall behind the stream.
        socket2::SockRef::from(&socket).set_recv_buffer_size(4 << 20).ok();
        let local = socket.local_addr().unwrap();
        let mut rtc = RtcConfig::new().build(Instant::now());
        rtc.add_local_candidate(Candidate::host(local, "udp").unwrap());
        let mut api = rtc.sdp_api();
        api.add_media(MediaKind::Video, Direction::RecvOnly, None, None, None);
        let (offer, pending) = api.apply().expect("an offer");
        let request = SessionDescription { kind: SdpKind::Offer, sdp: offer.to_sdp_string() };
        let (status, body) =
            worker.request("POST", "/api/offer", &serde_json::to_string(&request).unwrap());
        assert_eq!(status, 200, "{body}");
        let answer: SessionDescription = serde_json::from_str(&body).unwrap();
        let answer = SdpAnswer::from_sdp_string(&answer.sdp).unwrap();
        rtc.sdp_api().accept_answer(pending, answer).unwrap();
        Self { rtc, socket, local }
    }

    /// Runs the connection, handing each video frame to `frame`, until it returns false.
    /// Fails if that takes longer than `limit`.
    fn receive(
        &mut self,
        limit: Duration,
        mut frame: impl FnMut(&str0m::media::MediaData) -> bool,
    ) {
        let started = Instant::now();
        let mut buf = vec![0u8; 2048];
        loop {
            assert!(started.elapsed() < limit, "still receiving after {limit:?}");
            let timeout = match self.rtc.poll_output().unwrap() {
                Output::Transmit(t) => {
                    self.socket.send_to(&t.contents, t.destination).unwrap();
                    continue;
                }
                Output::Event(Event::MediaData(data)) => {
                    if !frame(&data) {
                        return;
                    }
                    continue;
                }
                Output::Event(_) => continue,
                Output::Timeout(t) => t,
            };
            let wait = timeout
                .saturating_duration_since(Instant::now())
                .clamp(Duration::from_millis(1), Duration::from_millis(10));
            self.socket.set_read_timeout(Some(wait)).unwrap();
            if let Ok((n, source)) = self.socket.recv_from(&mut buf) {
                let contents = buf[..n].try_into().unwrap();
                let destination = self.local;
                let receive = Receive { proto: Protocol::Udp, source, destination, contents };
                self.rtc.handle_input(Input::Receive(Instant::now(), receive)).unwrap();
            }
            self.rtc.handle_input(Input::Timeout(Instant::now())).unwrap();
        }
    }
}

/// A benchmark, not a check: how evenly frames reach a client. Streams the test core for
/// `SOVEMU_BENCH_SECONDS` (20 by default), over IPv6 too with `SOVEMU_BENCH_IPV6=1` (the worker's
/// two-socket path), or a real core with `SOVEMU_BENCH_CORE` (and its `SOVEMU_BENCH_ROM`), and
/// prints how late each frame arrived against its RTP timestamp, after the
/// earliest: what the worker's wake-ups, pacing and sending add before a frame leaves. The
/// worker's own `session health` lines (every two seconds) say how late its ticks started.
///
/// `cargo test --release -p sovemu-worker --test session -- --ignored --nocapture frame_pacing`
#[test]
#[ignore = "a benchmark: run it by name"]
fn frame_pacing() {
    let seconds: u64 =
        std::env::var("SOVEMU_BENCH_SECONDS").ok().and_then(|s| s.parse().ok()).unwrap_or(20);
    let ipv6 = std::env::var("SOVEMU_BENCH_IPV6").is_ok_and(|v| v == "1");
    str0m::crypto::from_feature_flags().install_process_default();
    let mut extra = vec!["--health-interval", "2"];
    if ipv6 {
        extra.extend(["--advertise-ipv6", "::1"]);
    }
    let Ok(socket) = UdpSocket::bind(if ipv6 { "[::1]:0" } else { "127.0.0.1:0" }) else {
        eprintln!("no IPv6 loopback here: skipped");
        return;
    };
    let rom = std::env::var("SOVEMU_BENCH_ROM").ok();
    if let Some(rom) = &rom {
        extra.extend(["--rom", rom.as_str()]);
    }
    let extra: Vec<&std::ffi::OsStr> = extra.iter().map(|a| a.as_ref()).collect();
    let core = std::env::var_os("SOVEMU_BENCH_CORE")
        .map_or_else(|| test_core().to_path_buf(), PathBuf::from);
    let mut worker = Worker::start_core(&core, &extra, &[]);
    let mut client = VideoClient::connect(&mut worker, socket);

    // (arrival, the frame's RTP time in seconds), from the first frame on.
    let mut frames: Vec<(Instant, f64)> = Vec::new();
    let mut first: Option<Instant> = None;
    client.receive(Duration::from_secs(seconds + 30), |data| {
        let now = Instant::now();
        first.get_or_insert(now);
        frames.push((now, data.time.as_seconds()));
        first.is_some_and(|f| f.elapsed() < Duration::from_secs(seconds))
    });

    // The first two seconds start the stream (its keyframe, the encoder settling): left out.
    let steady: Vec<_> = frames
        .iter()
        .filter(|(at, _)| at.duration_since(first.unwrap()) >= Duration::from_secs(2))
        .collect();
    assert!(steady.len() > 60, "only {} frames", steady.len());
    let (t0, r0) = *steady[0];
    let offsets: Vec<f64> =
        steady.iter().map(|(at, rtp)| at.duration_since(t0).as_secs_f64() - (rtp - r0)).collect();
    let earliest = offsets.iter().copied().fold(f64::INFINITY, f64::min);
    let mut late: Vec<f64> = offsets.iter().map(|o| (o - earliest) * 1000.0).collect();
    late.sort_by(f64::total_cmp);
    let mut gaps: Vec<f64> = steady
        .windows(2)
        .map(|w| {
            let shown = w[1].0.duration_since(w[0].0).as_secs_f64();
            ((shown - (w[1].1 - w[0].1)) * 1000.0).abs()
        })
        .collect();
    gaps.sort_by(f64::total_cmp);
    let at = |v: &[f64], p: f64| v[((v.len() - 1) as f64 * p) as usize];
    let span = steady.last().unwrap().0.duration_since(t0).as_secs_f64();
    eprintln!(
        "frame pacing over {}: {} frames in {span:.1} s ({:.1} fps)\n\
         \x20 arrival lateness ms: p50 {:.2}  p95 {:.2}  p99 {:.2}  max {:.2}\n\
         \x20 frame-gap error ms:  p50 {:.2}  p95 {:.2}  p99 {:.2}  max {:.2}",
        if ipv6 { "IPv6 (two sockets)" } else { "IPv4 (one socket)" },
        steady.len(),
        steady.len() as f64 / span,
        at(&late, 0.5),
        at(&late, 0.95),
        at(&late, 0.99),
        late[late.len() - 1],
        at(&gaps, 0.5),
        at(&gaps, 0.95),
        at(&gaps, 0.99),
        gaps[gaps.len() - 1],
    );
}

/// The acks each ratio is measured over: the latest ones, normally and fast-forwarding.
const RATIO_ACKS: usize = 6;
/// The first acks, while the stream starts, which no ratio is measured over.
const STARTUP_ACKS: usize = 2;

/// Emulated frames per video frame received, between the acks `from` and `to` (exclusive): how
/// far the game moved against how many frames were shown meanwhile. 0 if there are too few.
fn frames_per_shown(acks: &[InputAck], video: &[usize], from: usize, to: usize) -> f64 {
    if to < from + 2 || to > acks.len() {
        return 0.0;
    }
    let (first, last) = (from, to - 1);
    let shown = video[last].saturating_sub(video[first]);
    let emulated = acks[last].frame.saturating_sub(acks[first].frame);
    if shown == 0 { 0.0 } else { f64::from(emulated) / shown as f64 }
}
