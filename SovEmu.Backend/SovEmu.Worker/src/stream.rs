//! The streaming loop shared by `serve` (standalone) and `session` (driven by the server).
//!
//! Everything media-related runs on one thread: the core, the encoders and the str0m peer
//! (sans-IO) sharing the session's UDP port (`udp.rs`: IPv4, and IPv6 where the host has it).
//! Each turn of the loop takes any pending command (a client's offer, a control request), drives
//! the peer until it has nothing to send, waits on the port
//! until whichever comes first of the peer's next timer and the next frame's deadline, then runs
//! a frame if one is due. Emulation runs only while a client is connected, so the game pauses
//! when nobody is watching.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use crate::loading::{self, Report};
use crate::sysdir::SystemDir;
use crate::udp::Udp;
use anyhow::{Context, Result, anyhow};
use sovemu_media::{AudioEncoder, H264Preference};
use sovemu_protocol::ipc::{CoreCategory, CoreOption, LoadPhase};
use sovemu_protocol::ipc::{ExitReason, Request};
use sovemu_protocol::{
    ClientMessage, ClientStats, Hotkeys, INPUT_CHANNEL, InputAck, SdpKind, SessionDescription,
};
use sovemu_retro_host::{
    AnalogSticks, AvInfo, Core, CoreOptions, JoypadButtons, PixelFormat, VideoFrame,
};
use str0m::bwe::{Bitrate, BweKind};
use str0m::change::SdpOffer;
use str0m::channel::ChannelId;
use str0m::format::Codec;
use str0m::media::{Frequency, MediaKind, MediaTime, Mid, Pt};
use str0m::net::{Protocol, Receive};
use str0m::rtp::Extension;
use str0m::{Candidate, Event, IceConnectionState, Input, Output, Rtc, RtcConfig, RtcError};

use crate::adapt::{Adaptation, ClientSample, LimitChange, Limits};
use crate::health::{self, Health};
use crate::link::{PathClass, Probe, Stop};
use crate::pacing::{MediaClock, Pacer};
use crate::persist::{Persist, Screenshot};
use crate::rewind::Rewind;
use crate::runahead::RunAhead;
use crate::video::{CodecSetting, Video, VideoCodec, VideoSettings};
use crate::watchdog;
use sovemu_protocol::ipc::SessionHealth;
use sovemu_protocol::{AudioPath, Link, Pipeline, VideoPath};

/// Held input lapses if the client stops refreshing it: buttons and off-centre sticks after
/// `--input-lapse-ms` (`SovEmu__Streaming__Input__LapseMilliseconds`, 500 by default), hotkeys
/// after [`HOTKEY_LAPSE`]. Every client resends what is held at least every 100 ms, so a release
/// that never arrives (the input channel stalled or closed while the video kept going, the app
/// lost its window) lets go within the lapse instead of holding the button for the rest of the
/// session.
///
/// Hotkeys keep a fixed half second, whatever the setting. A lost button release is put right
/// by the next 100 ms refresh, which carries every button; the lapse only matters once the
/// client goes quiet. Clients stop sending hotkeys once none is held, so a lost hotkey release
/// is put right only by the lapse: a longer one would leave a game fast-forwarding or rewinding
/// longer after the player let go.
const HOTKEY_LAPSE: Duration = Duration::from_millis(500);

/// Whether held input has gone unrefreshed for `lapse`: buttons held (`held`) and last sent at
/// `buttons_at`, or sticks off-centre and last sent at `analog_at`.
fn input_lapsed(
    lapse: Duration,
    held: bool,
    buttons_at: Instant,
    analog_at: Option<Instant>,
    now: Instant,
) -> bool {
    let stale = |at: Instant| now.duration_since(at) >= lapse;
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
    /// A client's offer, and the codec the server chose for it (none: this worker's setting).
    Offer {
        offer: SessionDescription,
        codec: Option<String>,
        reply: Sender<Result<SessionDescription, String>>,
    },
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
    /// Testing only: the share (percent) of outgoing video packets to throw away (`loss.rs`).
    pub drop_video_percent: f64,
    /// Testing only: milliseconds each outgoing video packet is held (`loss.rs`).
    pub delay_video_ms: u32,
    /// Testing only: milliseconds of every 10 seconds in which all video packets are dropped.
    pub drop_video_burst_ms: u32,
    /// Testing only: the most video the link carries, kbit/s (0: no limit).
    pub limit_video_kbps: u32,
    /// Testing only: the most each video packet's delay varies by, milliseconds.
    pub jitter_video_ms: u32,
    /// How long a client must be silent before its return is taken as the end of a dropout
    /// and answered with a keyframe, in milliseconds (0: never).
    pub dropout_keyframe_ms: u32,
    /// How long a new client's link is checked before the stream starts (`link.rs`), in
    /// milliseconds (0: not checked), and on the same network.
    pub probe_ms: u32,
    pub probe_lan_ms: u32,
    /// How long held buttons and sticks stay held once the client stops refreshing them, in
    /// milliseconds ([`input_lapsed`]).
    pub input_lapse_ms: u32,
    /// The GPU for OpenGL cores; `None` refuses them. Its render node also encodes H.264.
    pub gpu: Option<sovemu_retro_host::GpuChoice>,
    pub video_codec: CodecSetting,
    pub video_encoder: H264Preference,
    /// Pictures are scaled up this many times (1 or 2) before they are encoded.
    pub video_prescale: u32,
    /// VP8's screen-content mode (0, 1 or 2).
    pub video_screen_content: u8,
    /// Testing only: frames shown instead of the game's picture (`replay.rs`).
    pub replay_frames: Option<std::path::PathBuf>,
}

/// The connected (or connecting) client.
struct Peer {
    rtc: Rtc,
    video: Option<(Mid, Pt)>,
    audio: Option<(Mid, Pt)>,
    input: Option<ChannelId>,
    connected: bool,
    /// Where the client's packets last came from: its end of the selected pair (the relay's
    /// loopback address when it is relayed).
    remote: Option<SocketAddr>,
    /// The session's address they were sent to: its end of the pair.
    local: Option<SocketAddr>,
}

/// The emulation and encoding state, kept across clients.
pub(crate) struct Session {
    core: Core,
    fps: f64,
    /// The video's RTP clock, which follows the core's frame rate without jumping.
    clock: MediaClock,
    video: Video,
    audio: AudioEncoder,
    /// The picture to encode: RGBA, or a GPU core's frame as the GPU converted it to I420.
    picture: VideoFrame,
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
    /// What the video is really being sent at, by which a falling estimate is judged.
    send_rate: crate::adapt::SendRate,
    /// Which of the client's keyframe requests are answered.
    keyframes: crate::keyframes::Gate,
    /// Simulated loss on the way out, for testing; drops nothing unless asked.
    loss: crate::loss::Loss,
    /// How long a client is silent before its return counts as the end of a dropout.
    dropout: Duration,
    /// The video bitrate and the send-bandwidth estimate when the client was last heard:
    /// what to return to after a dropout.
    kbps_when_heard: u32,
    estimate_when_heard: Option<u64>,
    /// When the client's last stats were taken (at most every [`STATS_SPACING`], however
    /// often a client sends them), and when they were last logged.
    stats_at: Option<Instant>,
    stats_logged_at: Option<Instant>,
    /// How often they are logged; `None`: never (as the health lines).
    stats_log_interval: Option<Duration>,
    /// Video frames sent to clients.
    frames_sent: u64,
    /// A frame was left out because the connection's send queue was full: the next one is a
    /// keyframe.
    video_gap: bool,
    /// The core has drawn a picture: it is up, and can take a state.
    drawn: bool,
    /// A state asked for before that, loaded once it has (its slot and bytes).
    pending_load: Option<(u32, Vec<u8>)>,
    /// Buttons held as a continued game's play started, per port: ignored until let go, so
    /// what was pressed at the loading screen does not act on the first frames ([`unheld`]).
    held_at_start: Vec<u16>,
    /// Until when the client's stats are logged as they come, not once per health line: the
    /// first seconds of play, where a slow start shows.
    stats_detail_until: Option<Instant>,
    /// Events for the front end, handed over by the loop after each turn.
    events: Vec<StreamEvent>,
    /// The pipeline last reported, and when it last went to the client (`None`: it must go
    /// now, as when the channel has just opened).
    pipeline: Option<Pipeline>,
    pipeline_to_client_at: Option<Instant>,
    /// The video bitrate, through the stream.
    adaptation: Adaptation,
    /// The check of a new client's link before its stream starts, while it runs; its budget
    /// (zero: no check), and its budget on the same network.
    probe: Option<Probe>,
    probe_budget: Duration,
    probe_lan_budget: Duration,
    /// What the client had lost, and its best round trip, as the check started: more loss or
    /// a round trip well up on it during the check means the link is full.
    probe_lost: Option<u64>,
    probe_rtt_ms: Option<f64>,
    /// Where the client's link goes: as the worker saw it, refined by what the client says.
    path: Option<PathClass>,
    client_path: Option<String>,
    /// The check's result once done (for the pipeline), and when a `0x83` last went out.
    link: Option<Link>,
    link_sent_at: Option<Instant>,
    audio_kbps: u32,
    hotkeys: Hotkeys,
    hotkeys_at: Instant,
    /// How long held buttons and sticks outlast the client's last refresh ([`input_lapsed`]).
    input_lapse: Duration,
    /// When `buttons` last arrived, and when the sticks last did while off-centre (`None`:
    /// centred, nothing to lapse).
    buttons_at: Instant,
    analog_at: Option<Instant>,
    /// Frames sent to the client, for RTP timestamps: wall-clock paced even when fast-forward
    /// runs several emulated frames per tick.
    ticks: u64,
    /// Size of the frame in `picture`, for save-state screenshots.
    shot_size: (u32, u32),
    /// Testing only: a clip shown instead of the game's picture.
    replay: Option<crate::replay::Replay>,
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
        // A GPU core's frames come back already converted for the encoder, unless they are to
        // be pre-scaled, which works on RGBA.
        core.set_gpu_i420(config.video_prescale <= 1);
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
                prescale: config.video_prescale.clamp(1, 2),
                screen_content: config.video_screen_content.min(2),
            }),
            audio: AudioEncoder::new(av.sample_rate, config.audio_kbps * 1000)?,
            fps: av.fps,
            clock: MediaClock::new(av.fps),
            core,
            picture: VideoFrame::default(),
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
            send_rate: crate::adapt::SendRate::default(),
            keyframes: crate::keyframes::Gate::new(),
            dropout: Duration::from_millis(u64::from(config.dropout_keyframe_ms)),
            kbps_when_heard: limits.start_kbps,
            estimate_when_heard: None,
            loss: crate::loss::Loss::new(
                config.drop_video_percent,
                config.delay_video_ms,
                config.drop_video_burst_ms,
            )
            .shaped(config.limit_video_kbps, config.jitter_video_ms),
            stats_at: None,
            stats_logged_at: None,
            stats_log_interval: (config.health_interval > 0)
                .then(|| Duration::from_secs(u64::from(config.health_interval))),
            frames_sent: 0,
            video_gap: false,
            drawn: false,
            pending_load: None,
            held_at_start: Vec::new(),
            stats_detail_until: None,
            events: Vec::new(),
            pipeline: None,
            pipeline_to_client_at: None,
            adaptation: Adaptation::new(limits, config.audio_kbps),
            probe: None,
            probe_budget: Duration::from_millis(u64::from(config.probe_ms)),
            probe_lan_budget: Duration::from_millis(u64::from(config.probe_lan_ms)),
            probe_lost: None,
            probe_rtt_ms: None,
            path: None,
            client_path: None,
            link: None,
            link_sent_at: None,
            audio_kbps: config.audio_kbps,
            hotkeys: Hotkeys::default(),
            last_analog_seq: None,
            hotkeys_at: Instant::now(),
            input_lapse: Duration::from_millis(u64::from(config.input_lapse_ms)),
            buttons_at: Instant::now(),
            analog_at: None,
            ticks: 0,
            shot_size: (0, 0),
            replay: config.replay_frames.as_deref().map(crate::replay::Replay::open).transpose()?,
            finished: false,
        })
    }

    /// Saves the battery RAM and the auto state, once.
    pub(crate) fn finish(&mut self) {
        if self.loss.active() {
            tracing::warn!(
                video_packets = self.loss.seen,
                dropped = self.loss.dropped,
                delay_ms = self.loss.delay_ms(),
                limit_kbps = self.loss.shape().0,
                jitter_ms = self.loss.shape().1,
                overflowed = self.loss.overflowed,
                "a simulated link was on (--drop-video-percent, --delay-video-ms, --limit-video-kbps, …): video packets were thrown away or held"
            );
        }
        if std::mem::replace(&mut self.finished, true) {
            return;
        }
        let at = self.health_now();
        self.health.summary(Instant::now(), &at);
        let (width, height) = self.shot_size;
        let rgba = screenshot_rgba(&self.picture);
        let shot = Screenshot { rgba: &rgba, width, height };
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
                let rgba = screenshot_rgba(&self.picture);
                let shot = Screenshot { rgba: &rgba, width, height };
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

pub(crate) use sovemu_protocol::net::advertised_addr;

/// Builds a peer for a client's offer and returns it with the SDP answer.
fn accept_offer(
    offer: &SessionDescription,
    hosts: Hosts,
    public: &[SocketAddr],
    video: Limits,
    starting_estimate_kbps: u64,
    codecs: CodecSetting,
) -> Result<(Peer, SessionDescription)> {
    let offer = SdpOffer::from_sdp_string(&offer.sdp).map_err(|e| anyhow!("bad offer: {e}"))?;
    // Full ICE, not ICE-lite: the worker sends connectivity checks of its own to the client's
    // addresses, which is what opens the home router to a player away from home (and ICE-lite
    // takes no server-reflexive candidate).
    let mut config = RtcConfig::new()
        // Playout delay is not in str0m's standard extensions, so without this the 0/0 delay
        // `step` writes on every video packet is never negotiated and never sent. The receiver's
        // WebRTC (Android, iOS, and browsers that ignore `jitterBufferTarget`) then buffers frames
        // to smooth jitter, which costs 40 ms on a quiet network and hundreds of milliseconds
        // over Wi-Fi. With it, frames are shown as soon as they are decoded. The id only matters
        // if the client offers none: str0m takes the client's.
        .set_extension(PLAYOUT_DELAY_EXT_ID, Extension::PlayoutDelay)
        .clear_codecs()
        .enable_vp8(codecs.allows(VideoCodec::Vp8))
        // Redundant audio (RED, RFC 2198) where the client offers it: each packet also carries
        // the one before, so a single lost packet is rebuilt on arrival instead of clicking.
        // Opus's own in-band FEC cannot do this: it lives in SILK, and the encoder runs CELT only
        // (restricted low delay). Costs about the audio bitrate again; clients without RED get
        // plain Opus.
        .enable_opus(true, true)
        // The estimate starts at what leaves the starting rate for video, and probes for more,
        // up to the ceiling.
        .enable_bwe(Some(Bitrate::kbps(starting_estimate_kbps)));
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
    add_candidates(&mut rtc, hosts, public)?;
    let answer = rtc.sdp_api().accept_offer(offer)?;
    let peer = Peer {
        rtc,
        video: None,
        audio: None,
        input: None,
        connected: false,
        remote: None,
        local: None,
    };
    Ok((peer, SessionDescription { kind: SdpKind::Answer, sdp: answer.to_sdp_string() }))
}

/// The session's own addresses: IPv4, and IPv6 where it has one (`udp.rs`).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Hosts {
    pub(crate) v4: SocketAddr,
    pub(crate) v6: Option<SocketAddr>,
}

impl Hosts {
    pub(crate) fn of(udp: &Udp) -> Self {
        Self { v4: udp.advertised(), v6: udp.advertised_v6() }
    }
}

/// The session's own addresses, and its public ones (`stun.rs`): the address the STUN servers see
/// it at, and the one the home router forwards to it (UPnP or NAT-PMP), each once and only when
/// it is another. Behind a router, those are where a player away from home reaches it. A global
/// IPv6 address needs no such help: players with IPv6 send to it as it is.
pub(crate) fn add_candidates(rtc: &mut Rtc, hosts: Hosts, public: &[SocketAddr]) -> Result<()> {
    rtc.add_local_candidate(Candidate::host(hosts.v4, "udp")?);
    if let Some(v6) = hosts.v6 {
        rtc.add_local_candidate(Candidate::host(v6, "udp")?);
    }
    let mut offered = vec![hosts.v4];
    for &addr in public {
        if offered.contains(&addr) || !addr.is_ipv4() {
            continue;
        }
        rtc.add_local_candidate(Candidate::server_reflexive(addr, hosts.v4, "udp")?);
        offered.push(addr);
    }
    Ok(())
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
    udp: Udp,
    mut reflexive: crate::stun::Reflexive,
    commands: Receiver<Command>,
    on_event: &mut dyn FnMut(StreamEvent),
    idle_timeout: Option<Duration>,
) -> Result<ExitReason> {
    let hosts = Hosts::of(&udp);
    // The public address, asked for now so the first client's answer needn't wait for it.
    reflexive.ask(udp.primary());
    let mut buf = vec![0u8; 2048];
    let mut peer: Option<Peer> = None;
    let mut pacer: Option<Pacer> = None;
    let mut next_frame = Instant::now();
    let mut idle_since = Some(Instant::now());

    watchdog::start();
    loop {
        watchdog::beat(watchdog::Phase::Network);
        // Over the memory limit: end as a quit does, so the session's saves are written.
        if crate::limits::over_memory() {
            return Ok(ExitReason::MemoryLimit);
        }
        // Commands: a new client replaces the current one; control requests run between frames.
        loop {
            match commands.try_recv() {
                Ok(Command::Offer { offer, codec, reply }) => {
                    // The server's choice for this client, when it made one, over the setting.
                    let codecs = match codec.as_deref().map(str::parse::<CodecSetting>) {
                        Some(Ok(chosen)) => chosen,
                        Some(Err(e)) => {
                            tracing::warn!(error = %e, "the server chose a codec this worker does not know");
                            session.video.codec_setting()
                        }
                        None => session.video.codec_setting(),
                    };
                    let public = reflexive.addresses(&udp);
                    let result = accept_offer(
                        &offer,
                        hosts,
                        &public,
                        session.adaptation.limits(),
                        session.adaptation.starting_estimate_kbps(),
                        codecs,
                    );
                    let answer = match result {
                        Ok((new_peer, answer)) => {
                            if peer.take().is_some_and(|p| p.connected) {
                                tracing::info!("new client replaces the current one");
                                session.health.suspend(Instant::now());
                                on_event(StreamEvent::ClientDisconnected);
                            }
                            session.new_client();
                            session.video.allow_for_client(codecs);
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
            match drive(p, &udp, &mut session) {
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
        // While a new client's link is checked the game waits: no video goes out to show a
        // picture early, and the player misses nothing.
        if let Some(p) = &mut peer
            && p.connected
        {
            session.poll_probe(p, Instant::now());
        }
        let emulating = running && !session.probing();
        if emulating && pacer.is_none() {
            let now = Instant::now();
            pacer = Some(Pacer::new(session.fps, now));
            next_frame = now;
            session.health.resume(now);
            session.video.request_keyframe();
        }
        let mut deadline = if emulating { rtc_deadline.min(next_frame) } else { rtc_deadline };
        // Simulated delay (testing only): packets whose wait is over go now, and the loop
        // wakes for the next.
        session.loss.flush(&udp, Instant::now());
        if let Some(due) = session.loss.next_due() {
            deadline = deadline.min(due);
        }

        // Wait for a datagram or the deadline. Each comes tagged with the address it was sent
        // to: str0m answers only checks to one of its candidates.
        let wait = deadline.saturating_duration_since(Instant::now());
        if !wait.is_zero() {
            match udp.recv(&mut buf, wait) {
                Ok(None) => {}
                Ok(Some((n, source, local))) => {
                    // A STUN server's answer is the session's, never the peer's.
                    if reflexive.handle(&buf[..n]) {
                    } else if let Some(p) = &mut peer
                        && let Ok(contents) = buf[..n].try_into()
                    {
                        let input = Input::Receive(
                            Instant::now(),
                            Receive { proto: Protocol::Udp, source, destination: local, contents },
                        );
                        if session.loss.incoming_lost(Instant::now()) {
                            // Simulated dropout (testing only): the client is not heard.
                        } else if p.rtc.accepts(&input) {
                            if p.connected
                                && let Some(kbps) = session.heard_client(Instant::now())
                            {
                                // The estimator has watched seconds of nothing arriving and
                                // would pace what follows (the keyframe first) at a crawl:
                                // start it again from where it was before the dropout.
                                p.rtc.bwe().reset(Bitrate::kbps(kbps));
                            }
                            p.remote = Some(source);
                            p.local = Some(local);
                            p.rtc.handle_input(input)?;
                        }
                    }
                }
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
            && !session.probing()
            && Instant::now() >= next_frame
        {
            let behind = Instant::now().saturating_duration_since(next_frame);
            session.step(p, behind)?;
            // The core changed its frame rate: pace at the new one from the next frame on.
            if pacer.fps() != session.fps {
                pacer.set_fps(session.fps);
            }
            next_frame = pacer.next_deadline(Instant::now());
        }
        for event in session.take_events() {
            on_event(event);
        }
    }
}

/// Whether str0m took a frame or sound packet to send. It holds what it has not sent yet and
/// refuses more past 512 (`WriteWithoutPoll`, though the loop polls between every frame): the
/// link has carried less than the stream for seconds. What is refused is left out, as a congested
/// link would lose it, rather than ending the session (sovereign-media/SovEmu#296).
fn queued(written: Result<(), RtcError>) -> Result<bool> {
    match written {
        Ok(()) => Ok(true),
        Err(RtcError::WriteWithoutPoll) => Ok(false),
        Err(e) => Err(e.into()),
    }
}

/// Polls the peer until it wants to wait. Returns its next deadline, or `None` if it is gone.
fn drive(p: &mut Peer, socket: &Udp, session: &mut Session) -> Result<Option<Instant>> {
    loop {
        if !p.rtc.is_alive() {
            return Ok(None);
        }
        match p.rtc.poll_output()? {
            Output::Timeout(t) => return Ok(Some(t)),
            Output::Transmit(t) => {
                let audio = p.audio.map(|(_, pt)| *pt);
                if session
                    .loss
                    .outgoing(&t.contents, t.destination, audio, Instant::now())
                    .is_none()
                {
                    continue;
                }
                if let Err(e) = socket.send_to(&t.contents, t.destination) {
                    tracing::debug!(destination = %t.destination, error = %e, "UDP send failed");
                }
            }
            Output::Event(event) => match event {
                Event::Connected => {
                    tracing::info!("client connected");
                    p.connected = true;
                    let (remote, local) = (p.remote, p.local);
                    session.start_probe(p, remote, local, Instant::now());
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
                    // A check already done (or turned off) before the channel opened: the client
                    // waits on it to say what comes next.
                    if let Some(link) = &session.link {
                        send_link(p, link);
                    }
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
                Event::KeyframeRequest(_) => {
                    // One keyframe for a burst of requests, and none for a request sent
                    // before the last keyframe could have arrived.
                    let answered = session.keyframes.asked(Instant::now());
                    session.adaptation.trouble(Instant::now());
                    session.health.keyframe_requested(answered);
                    if answered {
                        session.video.request_keyframe();
                    }
                }
                // While the client's link is checked, its estimates are the check's.
                Event::EgressBitrateEstimate(kind) if session.probing() => {
                    let now = Instant::now();
                    let probe = session.probe.as_mut().expect("probing");
                    match kind {
                        BweKind::Twcc { estimate, can_probe: true } => {
                            probe.estimate(estimate.as_u64(), now);
                        }
                        // No transport-wide feedback: nothing to probe with.
                        _ => probe.unavailable(now),
                    }
                }
                Event::EgressBitrateEstimate(
                    BweKind::Twcc { estimate, .. } | BweKind::Remb { estimate, .. },
                ) => {
                    session.adapt_bitrate(estimate.as_u64());
                    // A fall with nothing to back it: the estimator starts again from where
                    // it was, so the pacer does not slow a stream the link still carries.
                    if let Some(bps) = session.adaptation.take_restart() {
                        tracing::debug!(
                            fell_to_kbps = estimate.as_u64() / 1000,
                            back_to_kbps = bps / 1000,
                            "the bandwidth estimate fell with no sign of trouble: asked again"
                        );
                        p.rtc.bwe().reset(Bitrate::kbps(bps / 1000));
                    }
                }
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
                if let Some(rtt_ms) = stats.net_rtt_ms {
                    self.keyframes.round_trip(rtt_ms);
                }
                if stats.path.is_some() {
                    self.client_path.clone_from(&stats.path);
                }
                self.probe_stats(&stats, now);
                let sample = ClientSample {
                    fps: stats.fps,
                    decode_ms: stats.decode_ms,
                    frames_dropped: stats.frames_dropped,
                    frames_sent: self.frames_sent,
                    packets_lost: stats.packets_lost,
                    rtt_ms: stats.net_rtt_ms,
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
                let starting = self.stats_detail_until.is_some_and(|until| now < until);
                let every = self
                    .stats_log_interval
                    .map(|every| if starting { every.min(STATS_SPACING) } else { every });
                if every.is_some_and(due) {
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
        self.buttons = unheld(&mut self.held_at_start, &packet.ports);
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

    /// Releases held input the client has stopped refreshing ([`input_lapsed`]), and says so:
    /// a client that goes quiet while holding something is a fault worth seeing in the log.
    fn lapse_input(&mut self, now: Instant) {
        let held = self.buttons.iter().any(|b| b.0 != 0);
        if input_lapsed(self.input_lapse, held, self.buttons_at, self.analog_at, now) {
            tracing::warn!(
                buttons = ?self.buttons.iter().map(|b| b.0).collect::<Vec<_>>(),
                silent_ms = now.duration_since(self.buttons_at).as_millis() as u64,
                "client stopped sending input while holding it: released"
            );
            self.release_input();
        }
    }

    /// The client was heard at `now`. If it had been silent long enough to have been cut off
    /// (a Wi-Fi dropout), it has missed that much video and will stall until it asks for a
    /// keyframe, which it only does after waiting on retransmissions: send one now. Returns
    /// the send-bandwidth estimate from before the dropout (kbit/s), for the caller to start
    /// the estimator again from: a link that was down was not congested.
    fn heard_client(&mut self, now: Instant) -> Option<u64> {
        let Some(silent) = self.keyframes.heard(now, self.dropout) else {
            self.kbps_when_heard = self.video.bitrate_kbps();
            self.estimate_when_heard = self.estimate_kbps.or(self.estimate_when_heard);
            return None;
        };
        self.health.dropout();
        // The link was down, not congested: back to the bitrate from before it, at once.
        let cut_to = self.video.bitrate_kbps();
        self.adaptation.dropout(self.kbps_when_heard, now);
        self.apply_bitrate(now);
        let answered = self.keyframes.asked(now);
        if answered {
            self.video.request_keyframe();
        }
        tracing::info!(
            silent_ms = silent.as_millis() as u64,
            keyframe = answered,
            kbps_during = cut_to,
            kbps = self.video.bitrate_kbps(),
            estimate_kbps = self.estimate_when_heard,
            "client heard again after a dropout"
        );
        // With no estimate yet, the stream's own bitrate is the best there is.
        Some(self.estimate_when_heard.unwrap_or(u64::from(self.kbps_when_heard)))
    }

    /// Takes a send-bandwidth estimate.
    fn adapt_bitrate(&mut self, estimate_bps: u64) {
        let now = Instant::now();
        self.estimate_kbps = Some(estimate_bps / 1000);
        self.adaptation.sending(self.send_rate.kbps(now));
        self.adaptation.estimate(estimate_bps, now);
        self.apply_bitrate(now);
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
                ceiling_kbps = self.adaptation.ceiling_kbps(),
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

    /// The client's link is being checked: the game waits and no video is sent (`link.rs`).
    fn probing(&self) -> bool {
        self.probe.is_some()
    }

    /// The client has connected from `remote`: check its link before the stream starts, unless
    /// checks are off. The estimator pads toward the most the stream may use; on the same
    /// network it starts there too, with a shorter budget.
    fn start_probe(
        &mut self,
        p: &mut Peer,
        remote: Option<SocketAddr>,
        local: Option<SocketAddr>,
        now: Instant,
    ) {
        self.path = remote.map(|r| PathClass::of(r.ip(), local.map(|l| l.ip())));
        let limits = self.adaptation.limits();
        if !Probe::wanted(self.probe_budget) {
            self.link = Some(Link {
                state: Link::DONE.into(),
                path: self.path.map(|c| c.name().into()).unwrap_or_default(),
                start_kbps: limits.start_kbps,
                stopped_by: "off".into(),
                ..Link::default()
            });
            return;
        }
        let ceiling = u64::from(limits.max_kbps + self.audio_kbps);
        let mut probe = Probe::new(self.probe_budget, ceiling, now);
        if self.path == Some(PathClass::Lan) {
            probe.shorten(self.probe_lan_budget);
            p.rtc.bwe().reset(Bitrate::kbps(ceiling * 9 / 10));
        }
        p.rtc.bwe().set_bitrate(Bitrate::kbps(u64::from(limits.min_kbps)), Bitrate::kbps(ceiling));
        tracing::info!(
            path = self.path.map(PathClass::name),
            budget_ms = probe.budget().as_millis() as u64,
            ceiling_kbps = ceiling,
            "checking the client's connection"
        );
        self.probe = Some(probe);
        self.link_sent_at = None;
    }

    /// The client's stats while its link is checked: packets lost since the check began, or a
    /// round trip well up on the best it has reported, mean the link is full.
    fn probe_stats(&mut self, stats: &ClientStats, now: Instant) {
        let Some(probe) = &mut self.probe else { return };
        let lost = stats.packets_lost.unwrap_or(0);
        let baseline = *self.probe_lost.get_or_insert(lost);
        let rtt = stats.net_rtt_ms;
        let best = match (self.probe_rtt_ms, rtt) {
            (Some(best), Some(rtt)) => best.min(rtt),
            (best, rtt) => best.or(rtt).unwrap_or(f64::MAX),
        };
        self.probe_rtt_ms = Some(best);
        let rtt_rose = rtt.is_some_and(|rtt| rtt > best * 1.5 + 20.0);
        if lost > baseline || rtt_rose {
            tracing::debug!(
                lost,
                baseline,
                rtt_ms = rtt,
                best_rtt_ms = best,
                "trouble while checking the connection"
            );
            probe.trouble(now);
        }
    }

    /// While the client's link is checked: tells the client how far it has got, a few times
    /// a second, and once the check is done starts the stream from what it found.
    fn poll_probe(&mut self, p: &mut Peer, now: Instant) {
        let Some(probe) = &mut self.probe else { return };
        let done = probe.done(now);
        let path = self.path.map(|c| c.with_client(self.client_path.as_deref()));
        let report = Link {
            state: Link::PROBING.into(),
            elapsed_ms: probe.elapsed(now).as_millis() as u32,
            budget_ms: probe.budget().as_millis() as u32,
            path: path.map(|c| c.name().into()).unwrap_or_default(),
            estimate_kbps: probe.estimate_kbps().unwrap_or(0) as u32,
            ..Link::default()
        };
        match done {
            None => {
                if self.link_sent_at.is_none_or(|at| now.duration_since(at) >= LINK_REPORT) {
                    send_link(p, &report);
                    self.link_sent_at = Some(now);
                }
            }
            Some(why) => self.finish_probe(p, why, report, now),
        }
    }

    /// The check is done: the stream starts at what the link was seen to carry, and the
    /// estimator goes back to its usual ceiling, started from the measured estimate.
    fn finish_probe(&mut self, p: &mut Peer, why: Stop, report: Link, now: Instant) {
        let Some(probe) = self.probe.take() else { return };
        let limits = self.adaptation.limits();
        let path = self.path.map(|c| c.with_client(self.client_path.as_deref()));
        let measured = probe.estimate_kbps().filter(|_| why != Stop::Unavailable);
        let start = measured.map_or(limits.start_kbps, |kbps| {
            crate::link::start_kbps(
                kbps,
                self.audio_kbps,
                limits,
                path.unwrap_or(PathClass::Direct),
            )
        });
        self.adaptation.start_at(start);
        if let Some(kbps) = measured {
            p.rtc.bwe().reset(Bitrate::kbps(kbps));
        }
        p.rtc.bwe().set_bitrate(
            Bitrate::kbps(u64::from(limits.min_kbps)),
            Bitrate::kbps(u64::from(limits.max_kbps)),
        );
        self.apply_bitrate(now);
        let link = Link {
            state: Link::DONE.into(),
            start_kbps: self.video.bitrate_kbps(),
            rtt_ms: self.probe_rtt_ms.filter(|rtt| *rtt < f64::MAX),
            stopped_by: why.name().into(),
            ..report
        };
        tracing::info!(
            path = %link.path,
            stopped_by = %link.stopped_by,
            elapsed_ms = link.elapsed_ms,
            estimate_kbps = link.estimate_kbps,
            start_kbps = link.start_kbps,
            "connection checked: the stream starts"
        );
        self.stats_detail_until = Some(now + STARTING_STATS);
        send_link(p, &link);
        self.link = Some(link);
        self.link_sent_at = Some(now);
        self.pipeline_to_client_at = None;
    }

    /// A new client: its connection and its decoder are unknown, so the stream starts over
    /// at the starting rate.
    fn new_client(&mut self) {
        self.probe = None;
        self.probe_lost = None;
        self.probe_rtt_ms = None;
        self.path = None;
        self.client_path = None;
        self.link = None;
        self.link_sent_at = None;
        self.send_rate.clear();
        self.keyframes.new_client();
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
        let hotkeys = if now.duration_since(self.hotkeys_at) < HOTKEY_LAPSE {
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

        // A held state's boot is never seen, so it gets no input: a button pressed at the
        // loading screen must not reach a BIOS menu, or be held into the game.
        let input: &[JoypadButtons] = if self.pending_load.is_some() { &[] } else { &self.buttons };
        let out = self.core.run_frame(input)?;
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
            self.clock.set_fps(tick, av.fps);
            self.health.set_fps(av.fps);
        }

        let drew = out.video_updated && out.video.width > 0;
        if self.pending_load.is_some() {
            // Nothing of the fresh boot is shown or heard: the player asked to continue.
            if drew || self.frame >= PENDING_LOAD_FRAMES {
                let (slot, bytes) = self.pending_load.take().expect("checked above");
                self.drawn = true;
                let settling = Instant::now();
                if let Err(e) = Persist::apply_state(&mut self.core, slot, &bytes) {
                    tracing::warn!(slot, error = %format!("{e:#}"), "held state could not be loaded");
                }
                self.rewind.clear();
                // Still behind the loading screen: what costs time once is paid here, not
                // on the first frames the player sees and plays.
                let core = &mut self.core;
                self.rewind.trial(|| core.serialize());
                self.start_continued_play(p);
                tracing::info!(
                    slot,
                    booted_frames = self.frame,
                    settle_ms = settling.elapsed().as_secs_f64() * 1000.0,
                    "continuing from the save: play starts"
                );
            }
            self.health.ticked(behind, now.elapsed());
            self.health.core(self.core.take_timing());
            return Ok(());
        }

        let mut picture = None;
        if drew {
            self.drawn = true;
            crate::video::keep_picture(out.video, &mut self.picture);
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
                if !queued(writer.write(pt, now, rtp_time, packet))? {
                    self.health.unsent();
                }
            }
            self.audio_samples += sovemu_media::audio::PACKET_FRAMES as u64;
        }

        // The picture runs ahead of the real frame, hiding the game's own input lag; not while
        // rewinding or fast-forwarding, whose pictures are meant to be elsewhere in time.
        if !hotkeys.rewind
            && !hotkeys.fast_forward
            && let Some(ahead) =
                self.run_ahead.picture(&mut self.core, &self.buttons, &mut self.picture)?
        {
            picture = Some(ahead);
        }

        if picture.is_some()
            && let Some(replay) = &mut self.replay
        {
            let (w, h) = replay.next(&mut self.picture.data)?;
            (self.picture.width, self.picture.height) = (w, h);
            self.picture.format = PixelFormat::Rgba8888;
            picture = Some((w, h));
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
            // A frame was left out, so the next one sent must not depend on it.
            if self.video_gap {
                self.video.request_keyframe();
            }
            watchdog::beat(watchdog::Phase::Encoding);
            let encode_from = Instant::now();
            let encoded = self.video.encode_picture(&self.picture)?;
            if let Some(encoded) = &encoded {
                self.health.encoded(encode_from.elapsed(), encoded.data.len(), encoded.keyframe);
                self.send_rate.sent(encoded.data.len(), now);
                if encoded.keyframe {
                    self.keyframes.sent(Instant::now());
                }
            }
            if let (Some(encoded), Some((mid, pt))) = (encoded, p.video)
                && let Some(writer) = p.rtc.writer(mid)
            {
                let rtp_time = MediaTime::from_90khz(self.clock.at(tick));
                let written = writer
                    .playout_delay(MediaTime::from_millis(0), MediaTime::from_millis(0))
                    .write(pt, now, rtp_time, encoded.data);
                if queued(written)? {
                    self.frames_sent += 1;
                    self.video_gap = false;
                } else {
                    self.health.unsent();
                    self.video_gap = true;
                }
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

    /// A continued game's play starts, from its save, on the next frame. The loading screen
    /// sent no video: the estimator watched seconds of nothing and would pace the first
    /// keyframe out slowly, a backlog the player then sees played out fast with their input
    /// in it. It starts again from what the connection check measured, as after a dropout.
    fn start_continued_play(&mut self, p: &mut Peer) {
        let measured = self.link.as_ref().map(|l| u64::from(l.estimate_kbps)).filter(|k| *k > 0);
        let kbps = measured.unwrap_or(u64::from(self.video.bitrate_kbps()));
        p.rtc.bwe().reset(Bitrate::kbps(kbps));
        self.video.request_keyframe();
        self.held_at_start = self.buttons.iter().map(|b| b.0).collect();
        self.buttons = vec![JoypadButtons::NONE; self.buttons.len()];
        self.stats_detail_until = Some(Instant::now() + STARTING_STATS);
        self.stats_logged_at = None;
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
                VideoPath {
                    kbps,
                    limited_by: by.name().into(),
                    estimate_kbps: self.estimate_kbps.unwrap_or(0) as u32,
                    ..path
                }
            },
            audio: AudioPath {
                codec: "opus".into(),
                source_hz: self.audio.input_rate(),
                kbps: self.audio_kbps,
            },
            run_ahead: self.run_ahead.frames(),
            link: self.link.clone(),
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

/// How often a client is told how far the check of its link has got.
const LINK_REPORT: Duration = Duration::from_millis(200);

/// Sends a `0x83` to the client, if its input channel is open (the pipeline carries the
/// result again later, so a lost one is not missed).
fn send_link(p: &mut Peer, link: &Link) {
    if let Some(id) = p.input
        && let Some(mut channel) = p.rtc.channel(id)
    {
        let _ = channel.write(true, &link.encode());
    }
}

/// Clients send their stats once a second; any faster is dropped.
const STATS_SPACING: Duration = Duration::from_millis(900);

/// How long after play starts every client stats sample is logged.
const STARTING_STATS: Duration = Duration::from_secs(15);

/// The buttons that act, of those a client holds (`ports`): one held since play started
/// (`held_at_start`, which forgets each button as it is let go) acts once pressed again.
fn unheld(held_at_start: &mut Vec<u16>, ports: &[u16]) -> Vec<JoypadButtons> {
    held_at_start.truncate(ports.len());
    for (held, &now) in held_at_start.iter_mut().zip(ports) {
        *held &= now;
    }
    ports
        .iter()
        .enumerate()
        .map(|(i, &b)| JoypadButtons(b & !held_at_start.get(i).copied().unwrap_or(0)))
        .collect()
}

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

/// The picture as RGBA, for a save state's screenshot.
fn screenshot_rgba(picture: &VideoFrame) -> std::borrow::Cow<'_, [u8]> {
    match picture.format {
        PixelFormat::Rgba8888 => std::borrow::Cow::Borrowed(&picture.data),
        _ => std::borrow::Cow::Owned(picture.to_rgba8()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The candidates `add_candidates` gives a peer, as its offer lists them.
    fn offered(hosts: Hosts, public: &[SocketAddr]) -> Vec<String> {
        str0m::crypto::from_feature_flags().install_process_default();
        let mut rtc = RtcConfig::new().build(Instant::now());
        add_candidates(&mut rtc, hosts, public).unwrap();
        let mut api = rtc.sdp_api();
        api.add_channel("input".into());
        let (offer, _) = api.apply().unwrap();
        let sdp = offer.to_sdp_string();
        // `a=candidate:<foundation> 1 udp <priority> <address> <port> typ <kind> ...`
        sdp.lines()
            .filter_map(|l| l.strip_prefix("a=candidate:"))
            .map(|c| c.split(' ').skip(4).take(4).collect::<Vec<_>>().join(" "))
            .collect()
    }

    #[test]
    fn a_full_send_queue_leaves_the_frame_out_instead_of_ending_the_session() {
        str0m::crypto::from_feature_flags().install_process_default();
        let now = Instant::now();
        let mut sender = RtcConfig::new().build(now);
        let mut receiver = RtcConfig::new().build(now);
        let mut api = sender.sdp_api();
        let mid =
            api.add_media(MediaKind::Video, str0m::media::Direction::SendOnly, None, None, None);
        let (offer, pending) = api.apply().unwrap();
        let answer = receiver.sdp_api().accept_offer(offer).unwrap();
        sender.sdp_api().accept_answer(pending, answer).unwrap();
        let pt = payload_type(&mut sender, mid, Codec::Vp8).unwrap();

        // Never connected, so nothing is sent and every frame waits in str0m's queue.
        let mut refused = 0;
        for frame in 0..600u64 {
            let writer = sender.writer(mid).unwrap();
            let written = writer.write(pt, now, MediaTime::from_90khz(frame * 1500), vec![0; 100]);
            if !queued(written).expect("a full queue is not an error") {
                refused += 1;
            }
        }
        assert!(refused > 0, "str0m queued every frame: its limit moved");
        assert!(refused < 600, "str0m refused every frame");
    }

    #[test]
    fn a_session_offers_its_ipv6_address_beside_ipv4_and_the_public_one() {
        let v4: SocketAddr = "192.168.1.10:50000".parse().unwrap();
        let v6: SocketAddr = "[2600::5]:50000".parse().unwrap();
        let public: SocketAddr = "203.0.113.9:41000".parse().unwrap();
        let candidates = offered(Hosts { v4, v6: Some(v6) }, &[public]);
        assert!(candidates.contains(&"192.168.1.10 50000 typ host".into()), "{candidates:?}");
        assert!(candidates.contains(&"2600::5 50000 typ host".into()), "{candidates:?}");
        assert!(candidates.contains(&"203.0.113.9 41000 typ srflx".into()), "{candidates:?}");
        assert_eq!(candidates.len(), 3, "{candidates:?}");

        // No IPv6 address: IPv4 alone, as before.
        let candidates = offered(Hosts { v4, v6: None }, &[]);
        assert_eq!(candidates, vec!["192.168.1.10 50000 typ host".to_string()]);
    }

    #[test]
    fn the_address_the_router_forwards_is_offered_once_beside_stun_s() {
        let v4: SocketAddr = "192.168.1.10:50000".parse().unwrap();
        let stun: SocketAddr = "203.0.113.9:41000".parse().unwrap();
        let forwarded: SocketAddr = "203.0.113.9:50000".parse().unwrap();
        // STUN saw another port than the router forwards (a symmetric NAT): both are offered.
        let candidates = offered(Hosts { v4, v6: None }, &[stun, forwarded]);
        assert!(candidates.contains(&"203.0.113.9 41000 typ srflx".into()), "{candidates:?}");
        assert!(candidates.contains(&"203.0.113.9 50000 typ srflx".into()), "{candidates:?}");
        assert_eq!(candidates.len(), 3, "{candidates:?}");
        // The same address twice, or the host's own, is offered once.
        let candidates = offered(Hosts { v4, v6: None }, &[forwarded, forwarded, v4]);
        assert_eq!(candidates.len(), 2, "{candidates:?}");
    }

    #[test]
    fn held_input_lets_go_once_the_client_stops_refreshing_it() {
        let lapse = Duration::from_millis(500);
        let t = Instant::now();
        let soon = t + Duration::from_millis(100);
        let late = t + lapse;
        // Left held, refreshed every 100 ms: still held.
        assert!(!input_lapsed(lapse, true, t, None, soon));
        // The refreshes stop (the input channel stalled, the app lost its window): released.
        assert!(input_lapsed(lapse, true, t, None, late));
        // Nothing held: nothing to release, however long the client is quiet.
        assert!(!input_lapsed(lapse, false, t, None, late));
        // A stick pushed over, its heartbeat stopped: released too; a centred one sends none.
        assert!(input_lapsed(lapse, false, t, Some(t), late));
        assert!(!input_lapsed(lapse, false, t, Some(soon), soon + Duration::from_millis(100)));
    }

    #[test]
    fn a_button_held_as_play_starts_acts_once_let_go_and_pressed_again() {
        const A: u16 = 1 << 8;
        const RIGHT: u16 = 1 << 7;
        let bits = |b: Vec<JoypadButtons>| b.iter().map(|b| b.0).collect::<Vec<_>>();
        // Port 0 held A through the loading screen; port 1 held nothing.
        let mut held = vec![A, 0];
        assert_eq!(bits(unheld(&mut held, &[A, 0])), [0, 0], "still the same press");
        // A new button acts at once, the held one still does not.
        assert_eq!(bits(unheld(&mut held, &[A | RIGHT, RIGHT])), [RIGHT, RIGHT]);
        // Let go, then pressed again: now it is the player's.
        assert_eq!(bits(unheld(&mut held, &[0, 0])), [0, 0]);
        assert_eq!(bits(unheld(&mut held, &[A, 0])), [A, 0]);
        assert_eq!(held, [0, 0]);
        // A packet with fewer ports forgets the ports it left out.
        let mut held = vec![A, A];
        assert_eq!(bits(unheld(&mut held, &[A])), [0]);
        assert_eq!(bits(unheld(&mut held, &[A, A])), [0, A]);
    }

    #[test]
    fn a_longer_lapse_holds_input_through_a_longer_silence() {
        // Raised for a lossy remote link: a second of silence no longer lets go.
        let lapse = Duration::from_millis(1500);
        let t = Instant::now();
        let second = t + Duration::from_millis(1000);
        assert!(!input_lapsed(lapse, true, t, None, second));
        assert!(!input_lapsed(lapse, false, t, Some(t), second));
        // It still lets go once the client has been quiet that long.
        assert!(input_lapsed(lapse, true, t, None, t + lapse));
        assert!(input_lapsed(lapse, false, t, Some(t), t + lapse));
    }
}
