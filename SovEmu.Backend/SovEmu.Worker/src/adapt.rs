//! Picks the video bitrate through a stream: it starts at the configured rate and then follows
//! two things, staying within the configured floor and ceiling.
//!
//! - **The connection**: the send-bandwidth estimate (transport feedback from the client's
//!   WebRTC), which rises while there is headroom and falls on congestion. Taken as the median
//!   of the last [`ESTIMATE_WINDOW`]: on a home network the estimate jumps (0.9 to 17.8 Mbit/s
//!   within seconds, measured to a TV while a PS2 game ran slow and its frames went out in
//!   bursts), and following each jump moved the quality up and down every few seconds. A dip or
//!   a spike alone now moves nothing; a drop that lasts is followed within half the window.
//!   A fall is followed only when the stream is sending more than the estimate leaves, and
//!   then only when it is backed. Over Wi-Fi the estimator falls when packets merely arrive
//!   unevenly, and a busy stream was cut by a quarter each time. So the client's stats are
//!   asked: trouble it has just shown (packets lost, a keyframe asked for, frames arriving
//!   short, a long round trip) is believed at once, and on a link that has shown none yet a
//!   clean report overrules the estimator, which is started again from where it was. A client
//!   that says nothing, or a link that has shown trouble before, gets one pass
//!   ([`PASS_SPACING`]) and a second fall is believed. A fall that is believed leaves a
//!   ceiling under what was being sent, tried a step higher after a while: the estimate
//!   alone climbs straight back past what a slow link carries.
//! - **The client**: the stats it sends once a second ([`ClientStats`]). The estimate only sees
//!   the network; a client whose decoder falls behind at a rate the network carries happily
//!   shows it here, as frames it did not decode, frames it dropped, or a decode time that has
//!   climbed. That lowers a limit on the bitrate, which is raised again in steps while the
//!   client stays well. A client that sends no stats has no limit.
//!
//! Cuts happen at once; raises wait [`RAISE_HOLD`] after a cut, come [`RAISE_SPACING`] apart and
//! go a quarter at a time, so a wavering connection does not whip the quality up and down.
//! Changes smaller than a tenth are ignored. Where a change reopens the encoder (VA-API and
//! NVENC take their rate at open, and a reopen costs a keyframe), a raise must also be worth a
//! quarter and wait [`REOPEN_HOLD`] after the last change; a cut is never held back.
//!
//! [`ClientStats`]: sovemu_protocol::ClientStats

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// What the video is really being sent at: the encoded bytes of the last [`SEND_WINDOW`].
#[derive(Debug, Default)]
pub(crate) struct SendRate {
    /// Each frame sent: when, and its bytes.
    frames: VecDeque<(Instant, usize)>,
}

/// The stretch a send rate is taken over: long enough to smooth one large frame, short enough
/// to follow a picture that turns busy.
const SEND_WINDOW: Duration = Duration::from_secs(2);

impl SendRate {
    /// A frame of `bytes` was sent at `now`.
    pub(crate) fn sent(&mut self, bytes: usize, now: Instant) {
        self.frames.push_back((now, bytes));
        while self.frames.front().is_some_and(|(at, _)| now.duration_since(*at) > SEND_WINDOW) {
            self.frames.pop_front();
        }
    }

    /// The rate over the window, in kbit/s; none until half a window of frames is held.
    pub(crate) fn kbps(&self, now: Instant) -> Option<u32> {
        let (first, _) = self.frames.front()?;
        let span = now.duration_since(*first);
        if span < SEND_WINDOW / 2 {
            return None;
        }
        let bytes: usize = self.frames.iter().map(|(_, bytes)| bytes).sum();
        Some((bytes as f64 * 8.0 / 1000.0 / span.as_secs_f64()) as u32)
    }

    /// A new client: nothing sent to it yet.
    pub(crate) fn clear(&mut self) {
        self.frames.clear();
    }
}

/// The most one raise takes the bitrate to: a quarter more (rounded up, so it is always worth
/// an encoder's reopen).
fn raise_step(kbps: u32) -> u32 {
    kbps.saturating_add(kbps.div_ceil(4))
}

/// The estimates the connection is judged on: those of this long past, by their median.
const ESTIMATE_WINDOW: Duration = Duration::from_secs(2);
/// How long after a dropout new estimates are left alone: the estimator has seen seconds of
/// nothing arriving, which is not what the connection is like now.
const DROPOUT_GRACE: Duration = Duration::from_secs(1);
/// At most this many estimates are kept (str0m reports several a second).
const ESTIMATES_KEPT: usize = 64;

/// How long after a sign of trouble from the client (packets lost, a keyframe asked for) a
/// falling estimate is believed at once.
const TROUBLE_FRESH: Duration = Duration::from_secs(2);
/// A falling estimate with no sign of trouble is given one pass in this long: the estimator
/// is put back where it was and the bitrate stays. One that falls again inside it is believed.
const PASS_SPACING: Duration = Duration::from_secs(5);
/// How long after a pass new estimates are left alone, while the estimator starts again.
const PASS_GRACE: Duration = Duration::from_millis(300);
/// When a fall is believed, the link is taken to carry this share of what was being sent: the
/// bitrate is not raised past it ([`Adaptation::ceiling_kbps`]).
const CEILING_SHARE: f64 = 0.85;
/// Falls this soon after one that lowered the ceiling do not lower it again: for as long as
/// the bitrate is held from rising ([`RAISE_HOLD`]) it is under the ceiling already, and the
/// estimator is still reeling from the packets lost (an 8 Mbit/s link's ceiling went from 6.4
/// to 2.7 Mbit/s on a second fall two seconds after the first, and stayed there).
const CEILING_SETTLE: Duration = RAISE_HOLD;
/// A link's ceiling is tried a step higher after this long without a fall, and after twice as
/// long each time the try fails (to [`CEILING_HOLD_MOST`]): a link's capacity changes rarely,
/// and each try past it costs the player a stutter.
const CEILING_HOLD: Duration = Duration::from_secs(30);
const CEILING_HOLD_MOST: Duration = Duration::from_secs(300);
/// The step a ceiling is tried higher by.
const CEILING_RAISE: f64 = 0.05;
/// A try that stood its time doubles the next one's step, to this: a ceiling learnt from a
/// link that was only briefly full is left behind in a few tries, where one at the link's
/// real limit fails at the first and goes back to the smallest step.
const CEILING_RAISE_MOST: f64 = 0.20;
/// How long a client's word that the link is clean stands: a little over two of its samples.
const CLEAN_FRESH: Duration = Duration::from_millis(2500);
/// The estimator is started again at most this often.
const RESTART_SPACING: Duration = Duration::from_secs(1);
/// Frames are arriving short when the client decodes less than this share of those sent while
/// its decoder has time to spare: a queue is building on the way, before anything is lost.
const ARRIVED_SHARE: f64 = 0.92;
/// A round trip is long when it is over the best the client has reported by more than this
/// many milliseconds, and by more than the best itself.
const RTT_RISE_MS: f64 = 25.0;
/// Samples in a row that must show frames arriving short, or a long round trip, to count as
/// trouble: a single one is a Wi-Fi stall, made up in the next.
const SUSPECT_SAMPLES: u32 = 2;
/// A client is losing packets when more than this share of those sent to it are lost.
const LOST_SHARE: f64 = 0.02;
/// A video packet's size, for counting the packets a bitrate comes to.
const PACKET_BYTES: f64 = 1100.0;

/// The share of the estimate used for video and audio: headroom for retransmits and noise.
const ESTIMATE_SHARE: f64 = 0.85;
/// How long after a change an encoder that reopens for one is left alone before a raise.
const REOPEN_HOLD: Duration = Duration::from_secs(10);
/// After the bitrate is cut, it is not raised again for this long. Cuts answer congestion at
/// once (a queue is lag); raises wait, so a connection that wavers does not send the quality up
/// and down (down, straight back up, down again).
const RAISE_HOLD: Duration = Duration::from_secs(10);
/// Raises are at least this far apart, each at most a quarter more ([`raise_step`]).
const RAISE_SPACING: Duration = Duration::from_secs(2);
/// Samples in a row a client must be struggling before the bitrate is cut for it.
const BAD_SAMPLES: u32 = 2;
/// The cut: to this share of the bitrate the client was struggling at.
const CUT: f64 = 0.7;
/// Samples in a row a client must be well before its limit is raised a step.
const GOOD_SAMPLES: u32 = 5;
/// The step a limit is raised by.
const RAISE: f64 = 1.25;
/// A client is behind when it decodes less than this share of the frames sent to it, while its
/// decoder is busy (see [`BUSY_SHARE`]).
const DECODED_SHARE: f64 = 0.85;
/// A decoder is busy when a frame takes it at least this share of a frame's time. Frames lost or
/// late on the way (a connection that faltered, a burst from a server that fell behind) are not
/// decoded either, but its decode time stays short: those are the network estimate's to answer,
/// and cutting for them too stacks a second cut on the first, for a minute after the network
/// has recovered. (A TV decoding 640×448 in 3-9 ms was cut to the floor that way.)
const BUSY_SHARE: f64 = 0.75;
/// A client is behind when it drops more than this share of the frames sent to it.
const DROPPED_SHARE: f64 = 0.05;
/// A client's limit lapses when it has sent no stats for this long (it stopped sending them,
/// or never will again: there is nothing to raise the limit by).
const STATS_LAPSE: Duration = Duration::from_secs(10);
/// A gap between samples longer than this makes the next one a fresh start, not a comparison.
const SAMPLE_GAP: Duration = Duration::from_secs(3);
/// Fewer frames than this between samples (paused, loading) say nothing about the client.
const FEW_FRAMES: u64 = 10;

/// The player is told of a change in bitrate (a notice on the screen) when it has moved by a
/// quarter of what they were last told, and no sooner than this after the last notice.
const NOTICE_SPACING: Duration = Duration::from_secs(15);

/// What held the bitrate down when the player was last told it had fallen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LimitedBy {
    /// It has not fallen.
    Nothing,
    Connection,
    Client,
}

impl LimitedBy {
    /// The pipeline message's `limited_by`.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Nothing => "",
            Self::Connection => "connection",
            Self::Client => "client",
        }
    }
}

/// The configured bitrates, kbit/s.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Limits {
    pub(crate) min_kbps: u32,
    pub(crate) start_kbps: u32,
    pub(crate) max_kbps: u32,
}

impl Limits {
    /// The floor is never above the starting rate, nor the ceiling below it.
    pub(crate) fn new(min_kbps: u32, start_kbps: u32, max_kbps: u32) -> Self {
        let start_kbps = start_kbps.max(1);
        Self {
            min_kbps: min_kbps.clamp(1, start_kbps),
            start_kbps,
            max_kbps: max_kbps.max(start_kbps),
        }
    }
}

/// What a client's stats say about how it is keeping up, with what was sent to it meanwhile.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ClientSample {
    /// Frames it decoded per second.
    pub(crate) fps: Option<f64>,
    pub(crate) decode_ms: Option<f64>,
    /// Frames it dropped, since the stream began.
    pub(crate) frames_dropped: Option<u64>,
    /// Video frames sent to it, since the session began.
    pub(crate) frames_sent: u64,
    /// Packets it has lost, since the stream began.
    pub(crate) packets_lost: Option<u64>,
    /// The network's round trip as it measures it, milliseconds.
    pub(crate) rtt_ms: Option<f64>,
}

/// Why a client counts as struggling, for the log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Struggle {
    /// It decoded too few of the frames sent.
    Behind,
    /// It dropped too many of them.
    Dropping,
    /// Decoding a frame takes longer than a frame lasts, and twice what it took at best.
    SlowDecode,
}

impl Struggle {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Behind => "decoding fewer frames than sent",
            Self::Dropping => "dropping frames",
            Self::SlowDecode => "decode time climbed",
        }
    }
}

/// A change to the client's limit, for the log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LimitChange {
    Cut { to_kbps: u32, why: Struggle },
    Raised { to_kbps: u32 },
    Lifted,
}

pub(crate) struct Adaptation {
    limits: Limits,
    /// The limits as configured: a new client starts from these again, whatever the last one
    /// was measured to carry ([`Adaptation::start_at`]).
    configured: Limits,
    audio_kbps: u32,
    /// What the estimate leaves for video; none before the first one.
    network_kbps: Option<u32>,
    /// What the video is being sent at now ([`SendRate`]); none until it is known.
    sending_kbps: Option<u32>,
    /// The recent estimates (when, bit/s), for their median.
    estimates: VecDeque<(Instant, u64)>,
    /// The bitrate the client last struggled above; none while it has not.
    client_kbps: Option<u32>,
    bad: u32,
    good: u32,
    /// The shortest decode time the client has reported.
    best_decode_ms: Option<f64>,
    /// The sample before this one: when, the frames sent by then, the frames dropped by then.
    last: Option<(Instant, u64, Option<u64>)>,
    changed_at: Option<Instant>,
    /// Estimates are not taken before this: those just after a dropout still describe the
    /// dropout, not the connection.
    ignore_estimates_until: Option<Instant>,
    /// The next raise is the return from a dropout: taken whole and at once.
    returning: bool,
    /// When the bitrate was last cut, and last raised.
    cut_at: Option<Instant>,
    raised_at: Option<Instant>,
    /// The bitrate the player was last told of, why, and when; none before the first.
    told: Option<(u32, LimitedBy, Instant)>,
    /// The last estimate (the window's median, bit/s) that did not ask for a cut.
    believed_bps: Option<u64>,
    /// When the client last showed trouble on the way to it: packets lost, a keyframe asked for.
    trouble_at: Option<Instant>,
    /// When a falling estimate was last given a pass.
    passed_at: Option<Instant>,
    /// The estimate to start the estimator again from (bit/s), until the caller takes it.
    restart_bps: Option<u64>,
    /// The packets the client had lost by its last sample.
    lost_then: Option<u64>,
    /// When the client last reported a link with nothing wrong on it.
    clean_at: Option<Instant>,
    /// When the estimator was last started again.
    restarted_at: Option<Instant>,
    /// The shortest round trip the client has reported, milliseconds.
    best_rtt_ms: Option<f64>,
    /// Samples in a row with frames arriving short or a long round trip.
    suspect: u32,
    /// What the link is taken to carry for video, learnt from a fall that was believed: the
    /// bitrate is not raised past it. None while no fall has been believed.
    ceiling_kbps: Option<u32>,
    /// When the ceiling was last set or tried higher, how long it then stands, and whether
    /// it has been tried higher since it was set.
    ceiling_at: Option<Instant>,
    /// When a fall last lowered the ceiling.
    ceiling_cut_at: Option<Instant>,
    ceiling_hold: Duration,
    ceiling_tried: bool,
    /// The last ceiling that stood its whole hold without a fall.
    ceiling_stood: Option<u32>,
    /// The share the next try raises the ceiling by.
    ceiling_step: f64,
}

impl Adaptation {
    pub(crate) fn new(limits: Limits, audio_kbps: u32) -> Self {
        Self {
            limits,
            configured: limits,
            audio_kbps,
            network_kbps: None,
            sending_kbps: None,
            estimates: VecDeque::new(),
            client_kbps: None,
            bad: 0,
            good: 0,
            best_decode_ms: None,
            last: None,
            changed_at: None,
            ignore_estimates_until: None,
            returning: false,
            cut_at: None,
            raised_at: None,
            told: None,
            believed_bps: None,
            trouble_at: None,
            passed_at: None,
            restart_bps: None,
            lost_then: None,
            clean_at: None,
            restarted_at: None,
            best_rtt_ms: None,
            suspect: 0,
            ceiling_kbps: None,
            ceiling_at: None,
            ceiling_cut_at: None,
            ceiling_hold: CEILING_HOLD,
            ceiling_tried: false,
            ceiling_stood: None,
            ceiling_step: CEILING_RAISE,
        }
    }

    /// The estimate to start the estimator from, kbit/s: the one that leaves exactly the
    /// starting rate for video. Started from the starting rate itself, its first word left
    /// 85% of that less the audio, and every stream opened by cutting itself (5,000 to 4,122
    /// kbit/s) and waiting ten seconds to climb back.
    pub(crate) fn starting_estimate_kbps(&self) -> u64 {
        let wanted = f64::from(self.limits.start_kbps + self.audio_kbps);
        (wanted / ESTIMATE_SHARE).ceil() as u64 + 1
    }

    pub(crate) fn limits(&self) -> Limits {
        self.limits
    }

    /// A new client: nothing is known about its connection or its decoder.
    pub(crate) fn reset(&mut self) {
        *self = Self::new(self.configured, self.audio_kbps);
    }

    /// This client's link was measured before the stream started (`link.rs`): it starts at
    /// `start_kbps` instead of the configured rate, within the floor and ceiling. Before the
    /// first estimate the target is the starting rate, taken at once.
    pub(crate) fn start_at(&mut self, start_kbps: u32) {
        let Limits { min_kbps, max_kbps, .. } = self.configured;
        let start = start_kbps.clamp(min_kbps, max_kbps);
        self.limits = Limits { min_kbps, start_kbps: start, max_kbps };
    }

    /// The limit the client's stats have put on the bitrate, if any.
    pub(crate) fn client_limit_kbps(&self) -> Option<u32> {
        self.client_kbps
    }

    /// The bitrate to tell the player of, and what held it down if it has fallen: the one in
    /// use when it has moved far enough from the last one told, and not too soon after it.
    pub(crate) fn tell(&mut self, current_kbps: u32, now: Instant) -> (u32, LimitedBy) {
        match self.told {
            None => self.told = Some((current_kbps, LimitedBy::Nothing, now)),
            Some((told, _, at))
                if current_kbps.abs_diff(told) * 4 >= told
                    && now.duration_since(at) >= NOTICE_SPACING =>
            {
                let by = if current_kbps > told {
                    LimitedBy::Nothing
                } else if self.client_kbps.is_some_and(|limit| limit <= current_kbps) {
                    LimitedBy::Client
                } else {
                    LimitedBy::Connection
                };
                self.told = Some((current_kbps, by, now));
            }
            Some(_) => {}
        }
        let (kbps, by, _) = self.told.expect("set above");
        (kbps, by)
    }

    /// What the player was last told, without telling them anything new.
    pub(crate) fn told(&self) -> (u32, LimitedBy) {
        self.told.map_or((self.limits.start_kbps, LimitedBy::Nothing), |(kbps, by, _)| (kbps, by))
    }

    /// Takes a send-bandwidth estimate.
    /// The client is back after a dropout (the link was down, not congested). The estimates
    /// taken across it are forgotten, with the cut they caused: the network's limit is again
    /// `kbps_before`, what the stream was sent at when the client was last heard, and new
    /// estimates are left alone for [`DROPOUT_GRACE`], while the estimator finds its feet.
    /// What the client's own decoder limits it to is untouched.
    pub(crate) fn dropout(&mut self, kbps_before: u32, now: Instant) {
        let Limits { min_kbps, max_kbps, .. } = self.limits;
        self.estimates.clear();
        self.network_kbps = Some(kbps_before.clamp(min_kbps, max_kbps));
        self.ignore_estimates_until = Some(now + DROPOUT_GRACE);
        self.returning = true;
        self.cut_at = None;
    }

    pub(crate) fn estimate(&mut self, estimate_bps: u64, now: Instant) {
        if self.ignore_estimates_until.is_some_and(|until| now < until) {
            return;
        }
        self.estimates.push_back((now, estimate_bps));
        while self.estimates.len() > ESTIMATES_KEPT
            || self
                .estimates
                .front()
                .is_some_and(|(at, _)| now.duration_since(*at) > ESTIMATE_WINDOW)
        {
            self.estimates.pop_front();
        }
        let mut recent: Vec<u64> = self.estimates.iter().map(|(_, bps)| *bps).collect();
        recent.sort_unstable();
        let median = recent[recent.len() / 2];
        let usable = (median as f64 * ESTIMATE_SHARE / 1000.0) as i64 - i64::from(self.audio_kbps);
        let Limits { min_kbps, start_kbps, max_kbps } = self.limits;
        let allowed = usable.clamp(i64::from(min_kbps), i64::from(max_kbps)) as u32;
        // An estimate is only as good as what was sent to take it: a quiet picture sends a
        // fraction of its bitrate, and the estimator then sags towards that fraction at the
        // first late packet, on a link with room to spare (a LAN's estimate swung between 1.2
        // and 9.2 Mbit/s that way, and the player was told of a "slow connection" every
        // fifteen seconds). So an estimate lowers the bitrate only when the stream is sending
        // more than it leaves: that is a connection being overrun. One the stream is already
        // under proves nothing, and the bitrate stays; if the link really has shrunk, the
        // next busy picture goes over the estimate and is cut then.
        let lowers = allowed < self.network_kbps.unwrap_or(start_kbps);
        if !lowers {
            self.believed_bps = Some(median);
            self.network_kbps = Some(allowed);
            return;
        }
        if self.sending_kbps.is_some_and(|sending| i64::from(sending) <= usable) {
            return;
        }
        // The stream is over the estimate. On a link that is full, that is the moment to cut;
        // but the estimator also falls (to under what is being delivered) when packets merely
        // arrive unevenly, as they do over Wi-Fi and from a server whose frames go out late,
        // and a stream sending all of its bitrate is then always over it. A TV on a home
        // network that lost no packet and dropped no frame in eight minutes was told of a
        // change in quality seven times. So the client is asked: it sees the link from the
        // far end, once a second.
        let fresh = |at: Option<Instant>, within| {
            at.is_some_and(|at: Instant| now.duration_since(at) <= within)
        };
        // Trouble it has just shown (packets lost, frames arriving short, a long round trip,
        // a keyframe asked for): the fall is believed at once.
        if fresh(self.trouble_at, TROUBLE_FRESH) {
            self.believe(allowed, now);
            return;
        }
        // It has just reported a clean link: the estimator is overruled. It is started again
        // from where it was, so the pacer does not slow a stream the link carries, and the
        // bitrate stays. If the link is full after all, the client's next sample says so
        // (see `link`).
        //
        // Only on a link that has shown no trouble yet (no ceiling learnt). On one that has,
        // the estimator's fall is the early word that a queue is building, a second or two
        // before the client can see it, and starting the estimator again sends a burst of
        // probing packets that a link at its limit then loses (an 8 Mbit/s link froze for
        // seconds that way): there the fall is asked again once and then believed, as below.
        if self.ceiling_kbps.is_none() && fresh(self.clean_at, CLEAN_FRESH) {
            self.pass(now);
            return;
        }
        // A client that says nothing (it sends no stats, or has not yet): the estimator is
        // asked again instead. A link with room gives the same answer as before, and a full
        // one falls again within the second, which is believed.
        if self.believed_bps.is_some()
            && self.sending_kbps.is_some()
            && !fresh(self.passed_at, PASS_SPACING)
        {
            self.pass(now);
            return;
        }
        self.believe(allowed, now);
    }

    /// A fall is not followed: the estimates that made it are forgotten, and the estimator is
    /// started again from the last estimate that was believed (at most every
    /// [`RESTART_SPACING`]).
    fn pass(&mut self, now: Instant) {
        self.passed_at = Some(now);
        self.estimates.clear();
        self.ignore_estimates_until = Some(now + PASS_GRACE);
        if self.restarted_at.is_none_or(|at| now.duration_since(at) >= RESTART_SPACING) {
            self.restart_bps = self.believed_bps;
            self.restarted_at = self.restart_bps.map(|_| now);
        }
    }

    /// A fall is believed: the bitrate comes down to `allowed`. The estimate alone would then
    /// climb straight back past what the link carries (it reads high between falls, and
    /// raises come a quarter at a time): a 3 Mbit/s link was overrun every twenty seconds that
    /// way. So what was being sent when it fell is remembered as too much, and the bitrate
    /// stays under a ceiling below it. The ceiling also bounds the cut, at half of itself:
    /// once packets are lost the estimator falls to a fraction of what the link carries
    /// (741 kbit/s on an 8 Mbit/s link), and following it there costs the player the
    /// picture for the ten seconds it takes to climb back.
    fn believe(&mut self, allowed: u32, now: Instant) {
        let Limits { min_kbps, start_kbps, .. } = self.limits;
        // Not at a stream's start: the starting rate is a guess, and says nothing of the link.
        // Nor before the send rate is known: there is then no telling what was too much.
        if self.believed_bps.is_some() && self.sending_kbps.is_some() {
            // One fall lowers the ceiling once: those that follow it within
            // [`CEILING_SETTLE`] are the same fall still being measured.
            let settled =
                self.ceiling_cut_at.is_none_or(|at| now.duration_since(at) >= CEILING_SETTLE);
            if settled {
                let failed = self.sending_kbps.unwrap_or(start_kbps);
                let ceiling = ((f64::from(failed) * CEILING_SHARE) as u32).max(min_kbps);
                // A fall after a try higher: the try failed. The next waits longer, and the
                // ceiling goes back to the one that stood before it, not under it: that rate
                // was carried for the whole of its hold.
                let ceiling = if std::mem::take(&mut self.ceiling_tried) {
                    self.ceiling_hold = (self.ceiling_hold * 2).min(CEILING_HOLD_MOST);
                    self.ceiling_step = CEILING_RAISE;
                    self.ceiling_stood.map_or(ceiling, |stood| stood.max(ceiling))
                } else {
                    self.ceiling_kbps.map_or(ceiling, |old| old.min(ceiling))
                };
                self.ceiling_kbps = Some(ceiling);
                self.ceiling_at = Some(now);
                self.ceiling_cut_at = Some(now);
            }
        }
        let floor = self.ceiling_kbps.map_or(min_kbps, |ceiling| (ceiling / 2).max(min_kbps));
        self.network_kbps = Some(allowed.max(floor));
    }

    /// What the link is taken to carry, if a fall has been believed.
    pub(crate) fn ceiling_kbps(&self) -> Option<u32> {
        self.ceiling_kbps
    }

    /// The client showed trouble on the way to it at `now`: it lost packets, or asked for a
    /// keyframe. A falling estimate is believed at once for [`TROUBLE_FRESH`] after.
    pub(crate) fn trouble(&mut self, now: Instant) {
        self.trouble_at = Some(now);
    }

    /// The estimate to start the estimator again from (bit/s), once: a fall was given a pass.
    pub(crate) fn take_restart(&mut self) -> Option<u64> {
        self.restart_bps.take()
    }

    /// Takes what the video is being sent at now, by which a falling estimate is judged.
    pub(crate) fn sending(&mut self, kbps: Option<u32>) {
        self.sending_kbps = kbps;
    }

    /// Takes a client's stats, with the bitrate in use while it measured them.
    pub(crate) fn client(
        &mut self,
        sample: ClientSample,
        current_kbps: u32,
        now: Instant,
    ) -> Option<LimitChange> {
        self.link(&sample, current_kbps, now);
        self.lost_then = sample.packets_lost;
        let verdict = self.judge(&sample, now);
        self.last = Some((now, sample.frames_sent, sample.frames_dropped));
        match verdict? {
            Some(why) => {
                self.good = 0;
                self.bad += 1;
                if self.bad < BAD_SAMPLES {
                    return None;
                }
                self.bad = 0;
                let to_kbps = ((f64::from(current_kbps) * CUT) as u32).max(self.limits.min_kbps);
                if self.client_kbps.is_some_and(|limit| limit <= to_kbps) {
                    return None;
                }
                self.client_kbps = Some(to_kbps);
                Some(LimitChange::Cut { to_kbps, why })
            }
            None => {
                self.bad = 0;
                let limit = self.client_kbps?;
                self.good += 1;
                if self.good < GOOD_SAMPLES {
                    return None;
                }
                self.good = 0;
                let to_kbps = (f64::from(limit) * RAISE) as u32;
                if to_kbps >= self.limits.max_kbps {
                    self.client_kbps = None;
                    return Some(LimitChange::Lifted);
                }
                self.client_kbps = Some(to_kbps);
                Some(LimitChange::Raised { to_kbps })
            }
        }
    }

    /// What the client's sample says of the link, as seen from its end: trouble (packets lost;
    /// or, two samples running, frames arriving short or a long round trip) or a clean link.
    /// A sample that cannot be compared with the one before says nothing.
    fn link(&mut self, sample: &ClientSample, current_kbps: u32, now: Instant) {
        let best_rtt = self.best_rtt_ms;
        if let Some(ms) = sample.rtt_ms.filter(|ms| *ms > 0.0) {
            self.best_rtt_ms = Some(best_rtt.map_or(ms, |best| best.min(ms)));
        }
        let Some((at, sent_then, _)) = self.last else { return };
        let elapsed = now.duration_since(at);
        let frames = sample.frames_sent.saturating_sub(sent_then);
        if elapsed > SAMPLE_GAP || frames < FEW_FRAMES {
            self.suspect = 0;
            return;
        }
        let secs = elapsed.as_secs_f64();
        let sending = self.sending_kbps.unwrap_or(current_kbps);
        // Packets lost on the way are the connection's trouble, not the decoder's.
        let packets = f64::from(sending) * 125.0 / PACKET_BYTES * secs;
        let losing = matches!((sample.packets_lost, self.lost_then), (Some(lost), Some(then))
            if lost.saturating_sub(then) as f64 > packets * LOST_SHARE);
        // Fewer frames decoded than sent by a decoder with time to spare: they are late.
        let frame_ms = secs * 1000.0 / frames as f64;
        let idle = sample.decode_ms.is_some_and(|ms| ms < frame_ms * BUSY_SHARE);
        let short =
            idle && sample.fps.is_some_and(|fps| fps * secs < frames as f64 * ARRIVED_SHARE);
        let slow = matches!((sample.rtt_ms, best_rtt), (Some(ms), Some(best))
            if ms > best + RTT_RISE_MS.max(best));
        self.suspect = if short || slow { self.suspect + 1 } else { 0 };
        if losing || self.suspect >= SUSPECT_SAMPLES {
            self.trouble(now);
            // Just after a fall was not followed: that was wrong, and the estimator, started
            // again, may take seconds to say so itself.
            if self.passed_at.is_some_and(|at| now.duration_since(at) <= PASS_SPACING) {
                self.passed_at = None;
                self.believe((f64::from(sending) * CEILING_SHARE) as u32, now);
            }
        } else if self.suspect == 0 {
            self.clean_at = Some(now);
        }
    }

    /// `None`: the sample cannot be judged (the first, one after a gap, too few frames sent).
    /// Otherwise how the client is struggling, if it is.
    fn judge(&mut self, sample: &ClientSample, now: Instant) -> Option<Option<Struggle>> {
        if let Some(ms) = sample.decode_ms.filter(|ms| *ms > 0.0) {
            self.best_decode_ms = Some(self.best_decode_ms.map_or(ms, |best| best.min(ms)));
        }
        let (at, sent_then, dropped_then) = self.last?;
        let elapsed = now.duration_since(at);
        let sent = sample.frames_sent.saturating_sub(sent_then);
        if elapsed > SAMPLE_GAP || sent < FEW_FRAMES {
            return None;
        }
        let sent = sent as f64;
        let frame_ms = elapsed.as_secs_f64() * 1000.0 / sent;
        // A client that does not report its decode time is taken at its word.
        let busy = sample.decode_ms.is_none_or(|ms| ms >= frame_ms * BUSY_SHARE);
        if let Some(fps) = sample.fps
            && fps * elapsed.as_secs_f64() < sent * DECODED_SHARE
            && busy
        {
            return Some(Some(Struggle::Behind));
        }
        if let (Some(dropped), Some(then)) = (sample.frames_dropped, dropped_then)
            && dropped.saturating_sub(then) as f64 > sent * DROPPED_SHARE
        {
            return Some(Some(Struggle::Dropping));
        }
        if let (Some(ms), Some(best)) = (sample.decode_ms, self.best_decode_ms)
            && ms > frame_ms
            && ms > best * 2.0
        {
            return Some(Some(Struggle::SlowDecode));
        }
        Some(None)
    }

    /// The bitrate wanted now: what the connection carries (the starting rate until it has
    /// been estimated), under the client's limit.
    fn target(&mut self, now: Instant) -> u32 {
        if self.last.is_some_and(|(at, ..)| now.duration_since(at) > STATS_LAPSE) {
            self.client_kbps = None;
            self.last = None;
            (self.bad, self.good) = (0, 0);
        }
        // A ceiling that has stood its time without a fall is tried a step higher.
        if let (Some(ceiling), Some(at)) = (self.ceiling_kbps, self.ceiling_at)
            && now.duration_since(at) >= self.ceiling_hold
        {
            // The last try stood: this one goes further.
            if self.ceiling_tried {
                self.ceiling_step = (self.ceiling_step * 2.0).min(CEILING_RAISE_MOST);
            }
            self.ceiling_stood = Some(ceiling);
            let higher = (f64::from(ceiling) * (1.0 + self.ceiling_step)) as u32;
            self.ceiling_kbps = (higher < self.limits.max_kbps).then_some(higher);
            self.ceiling_at = Some(now);
            self.ceiling_tried = true;
        }
        let network = self.network_kbps.unwrap_or(self.limits.start_kbps);
        let network = self.ceiling_kbps.map_or(network, |ceiling| network.min(ceiling));
        self.client_kbps.map_or(network, |limit| network.min(limit))
    }

    /// The bitrate to change to, if the change is worth making. `reopens`: the encoder in use
    /// reopens (with a keyframe) to change its rate.
    pub(crate) fn decide(&mut self, current_kbps: u32, reopens: bool, now: Instant) -> Option<u32> {
        let mut target = self.target(now);
        // Before the first estimate the target is the starting rate, taken at once (a new client
        // starts afresh), and so is the return from a dropout; otherwise raises are held and
        // stepped.
        let returning = std::mem::take(&mut self.returning);
        if returning && target > current_kbps {
            self.changed_at = Some(now);
            self.raised_at = Some(now);
            return Some(target);
        }
        if target > current_kbps && self.network_kbps.is_some() {
            let since = |at: Option<Instant>| at.map(|at| now.duration_since(at));
            if since(self.cut_at).is_some_and(|d| d < RAISE_HOLD)
                || since(self.raised_at).is_some_and(|d| d < RAISE_SPACING)
            {
                return None;
            }
            target = target.min(raise_step(current_kbps));
        }
        let diff = target.abs_diff(current_kbps);
        let worth = if reopens && target > current_kbps {
            diff * 4 >= current_kbps
                && self.changed_at.is_none_or(|at| now.duration_since(at) >= REOPEN_HOLD)
        } else {
            diff * 10 > current_kbps
        };
        worth.then(|| {
            self.changed_at = Some(now);
            if target < current_kbps {
                self.cut_at = Some(now);
            } else {
                self.raised_at = Some(now);
            }
            target
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIMITS: Limits = Limits { min_kbps: 300, start_kbps: 5000, max_kbps: 12_000 };

    fn adaptation() -> Adaptation {
        Adaptation::new(LIMITS, 128)
    }

    #[test]
    fn starts_a_measured_client_at_its_own_rate_and_the_next_at_the_configured_one() {
        // #243: a link measured at 1500 kbit/s starts there at once, not at 5000 and cut by
        // cut down to it (each cut a stall the player sees).
        let now = Instant::now();
        let mut a = adaptation();
        a.start_at(1147);
        assert_eq!(a.decide(5000, false, now), Some(1147));
        assert_eq!(a.told().0, 1147, "the player is told the rate it starts at");
        // Never past the floor or ceiling.
        a.start_at(50_000);
        assert_eq!(a.limits().start_kbps, LIMITS.max_kbps);
        // The next client is unknown again.
        a.reset();
        assert_eq!(a.limits(), LIMITS);
    }

    /// An estimate on its own: each well after the last, beyond the window, so it alone counts.
    fn alone(a: &mut Adaptation, bps: u64) {
        use std::sync::atomic::{AtomicU64, Ordering};
        static BASE: std::sync::LazyLock<Instant> = std::sync::LazyLock::new(Instant::now);
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        a.estimate(bps, *BASE + ESTIMATE_WINDOW * 5 * (n as u32 + 1));
    }

    #[test]
    fn a_falling_estimate_cuts_only_a_stream_that_is_over_it() {
        // A stream that starts at its ceiling, so the only way is down.
        let capped = Limits { min_kbps: 300, start_kbps: 5000, max_kbps: 5000 };
        let mut a = Adaptation::new(capped, 128);
        let t = Instant::now();
        let ms = |n: u64| t + Duration::from_millis(n);
        // A LAN: the estimate allows the start rate and more.
        for at in [0, 200, 400] {
            a.estimate(9_000_000, ms(at));
        }
        assert_eq!(a.decide(5000, false, ms(400)), None, "already at the start rate");
        // A quiet picture sends 600 kbit/s, and the estimate sags to 1.2 Mbit/s at a late
        // packet. The stream is under it: nothing is overrun, and nothing is cut.
        a.sending(Some(600));
        for at in [3000, 3200, 3400, 3600] {
            a.estimate(1_200_000, ms(at));
        }
        assert_eq!(a.decide(5000, false, ms(3600)), None);
        // The picture turns busy and the stream sends 4 Mbit/s against that estimate: now the
        // connection is being overrun, and the bitrate is cut at once.
        a.sending(Some(4000));
        a.trouble(ms(3700));
        a.estimate(1_200_000, ms(3800));
        let cut = a.decide(5000, false, ms(3800)).expect("cut for a connection overrun");
        assert_eq!(cut, 1700, "bounded: half of what the link is now taken to carry");
        // Before the send rate is known, an estimate is taken at its word, as it always was.
        let mut unknown = Adaptation::new(capped, 128);
        for at in [0, 200, 400] {
            unknown.estimate(1_200_000, ms(at));
        }
        assert!(unknown.decide(5000, false, ms(400)).is_some_and(|kbps| kbps < 1000));
    }

    #[test]
    fn the_send_rate_is_what_went_out_over_the_last_two_seconds() {
        let mut rate = SendRate::default();
        let t = Instant::now();
        let ms = |n: u64| t + Duration::from_millis(n);
        assert_eq!(rate.kbps(t), None);
        // 60 frames a second of 1250 bytes: 600 kbit/s.
        for n in 0..=180 {
            rate.sent(1250, ms(n * 1000 / 60));
            if n == 30 {
                assert_eq!(rate.kbps(ms(500)), None, "half a second is too little to say");
            }
        }
        let kbps = rate.kbps(ms(3000)).unwrap();
        assert!((590..=620).contains(&kbps), "{kbps}");
        rate.clear();
        assert_eq!(rate.kbps(ms(3000)), None);
    }

    /// A LAN's stream at its ceiling of 10 Mbit/s, sending all of it, with the estimate well
    /// above it.
    fn busy_on_a_lan(t: Instant) -> Adaptation {
        let limits = Limits { min_kbps: 500, start_kbps: 5000, max_kbps: 10_000 };
        let mut a = Adaptation::new(limits, 128);
        for at in [0, 200, 400] {
            a.estimate(16_000_000, t + Duration::from_millis(at));
        }
        a.sending(Some(10_000));
        a
    }

    #[test]
    fn a_fall_with_no_sign_of_trouble_is_asked_again_not_followed() {
        let t = Instant::now();
        let ms = |n: u64| t + Duration::from_millis(n);
        let mut a = busy_on_a_lan(t);
        assert_eq!(a.take_restart(), None);
        // A late packet over Wi-Fi: the estimator falls to under what is being delivered.
        // Nothing was lost and no keyframe asked for, so the bitrate stays and the estimator
        // is started again from where it was.
        for at in [5000, 5200] {
            a.estimate(8_500_000, ms(at));
        }
        assert_eq!(a.decide(10_000, false, ms(5200)), None, "not cut");
        assert_eq!(a.take_restart(), Some(16_000_000));
        assert_eq!(a.take_restart(), None, "taken once");
        assert_eq!(a.ceiling_kbps(), None);
        // The link has the room it had: the estimate comes back, and that was all.
        a.estimate(15_000_000, ms(6200));
        assert_eq!(a.decide(10_000, false, ms(6200)), None);
        // A fall long after is a new question, asked again in its turn.
        a.estimate(8_500_000, ms(20_000));
        assert_eq!(a.decide(10_000, false, ms(20_000)), None);
        assert_eq!(a.take_restart(), Some(15_000_000));
    }

    #[test]
    fn a_fall_that_comes_straight_back_is_believed_and_the_link_remembered() {
        let t = Instant::now();
        let ms = |n: u64| t + Duration::from_millis(n);
        let mut a = busy_on_a_lan(t);
        for at in [5000, 5200] {
            a.estimate(8_500_000, ms(at));
        }
        assert_eq!(a.take_restart(), Some(16_000_000), "asked again");
        // Started again, the estimator falls again within the second: the link is full.
        a.estimate(8_000_000, ms(5100)); // inside the grace: left alone
        for at in [5600, 5800] {
            a.estimate(8_000_000, ms(at));
        }
        assert_eq!(a.take_restart(), None, "not asked a second time");
        // 85% of 8 Mbit/s less the audio; and the link is taken to carry 85% of what failed.
        assert_eq!(a.decide(10_000, false, ms(5800)), Some(6672));
        assert_eq!(a.ceiling_kbps(), Some(8500));
        // The estimate reads high again, as it does between falls: the bitrate climbs back
        // to the ceiling and no further.
        a.sending(Some(6672));
        a.estimate(14_000_000, ms(9000));
        assert_eq!(a.decide(6672, false, ms(16_000)), Some(8340), "a quarter at a time");
        assert_eq!(a.decide(8340, false, ms(18_000)), None, "8500 is under a tenth more");
        // After half a minute without a fall the ceiling is tried a step higher.
        a.estimate(14_000_000, ms(36_000));
        assert_eq!(a.decide(8340, false, ms(36_000)), None, "8925 is under a tenth more");
        assert_eq!(a.ceiling_kbps(), Some(8925));
        // The try fails: back to the ceiling that stood, not under it, and the next try
        // waits twice as long.
        a.sending(Some(8900));
        a.trouble(ms(40_000));
        a.estimate(7_000_000, ms(40_100));
        assert_eq!(a.ceiling_kbps(), Some(8500));
        a.estimate(14_000_000, ms(75_000));
        let _ = a.decide(8500, false, ms(75_000));
        assert_eq!(a.ceiling_kbps(), Some(8500), "35 s on: not tried again yet");
        let _ = a.decide(8500, false, ms(101_000));
        assert_eq!(a.ceiling_kbps(), Some(8925), "a minute on: tried again");
        // A try that stands doubles the next one's step.
        let _ = a.decide(8500, false, ms(162_000));
        assert_eq!(a.ceiling_kbps(), Some(9817), "a tenth more, the try before having stood");
    }

    #[test]
    fn trouble_from_the_client_is_believed_at_once_and_bounds_the_cut() {
        let t = Instant::now();
        let ms = |n: u64| t + Duration::from_millis(n);
        // A keyframe asked for, then the estimate falls: no asking again.
        let mut a = busy_on_a_lan(t);
        a.trouble(ms(4900));
        // Packets lost: the estimator reads a fraction of what the link carries.
        for at in [5000, 5200, 5400] {
            a.estimate(700_000, ms(at));
        }
        assert_eq!(a.take_restart(), None);
        assert_eq!(a.ceiling_kbps(), Some(8500));
        assert_eq!(a.decide(10_000, false, ms(5400)), Some(4250), "half the ceiling, not 467");
        // The falls that follow in the same ten seconds are the same one: the ceiling stays.
        a.sending(Some(4250));
        a.trouble(ms(7000));
        a.estimate(600_000, ms(7600));
        assert_eq!(a.ceiling_kbps(), Some(8500));
        // One after that is a new one: what was being sent then was too much as well.
        a.trouble(ms(16_000));
        a.estimate(600_000, ms(16_100));
        assert_eq!(a.ceiling_kbps(), Some(3612));
    }

    #[test]
    fn packets_lost_after_a_pass_undo_it() {
        let t = Instant::now();
        let ms = |n: u64| t + Duration::from_millis(n);
        let mut a = busy_on_a_lan(t);
        assert_eq!(a.client(seen(4000, 60.0, 0, None), 10_000, ms(4000)), None);
        for at in [5000, 5200] {
            a.estimate(8_500_000, ms(at));
        }
        assert_eq!(a.take_restart(), Some(16_000_000), "a pass");
        // A handful lost of the 1,100 or so sent in a second: Wi-Fi does that. Nothing.
        assert_eq!(a.client(seen(5300, 60.0, 5, None), 10_000, ms(5300)), None);
        assert_eq!(a.ceiling_kbps(), None);
        // A twentieth lost: the link is full, whatever the estimator now says.
        assert_eq!(a.client(seen(6300, 60.0, 65, None), 10_000, ms(6300)), None);
        assert_eq!(a.ceiling_kbps(), Some(8500));
        assert_eq!(a.decide(10_000, false, ms(6300)), Some(8500));
    }

    /// A client's sample at `at_ms` into a 60 fps stream: the frames it decoded a second, the
    /// packets it has lost, and its round trip. Its decoder has time to spare.
    fn seen(at_ms: u64, fps: f64, lost: u64, rtt_ms: Option<f64>) -> ClientSample {
        ClientSample {
            fps: Some(fps),
            decode_ms: Some(3.0),
            frames_dropped: Some(0),
            frames_sent: at_ms * 60 / 1000,
            packets_lost: Some(lost),
            rtt_ms,
        }
    }

    #[test]
    fn a_client_that_reports_a_clean_link_overrules_the_estimator_every_time() {
        // The TV on a home network: the estimator falls again and again, seconds apart, and
        // the client loses nothing, drops nothing and decodes every frame.
        let t = Instant::now();
        let ms = |n: u64| t + Duration::from_millis(n);
        let mut a = busy_on_a_lan(t);
        let mut restarts = 0;
        for second in 1..=60u64 {
            let at = second * 1000;
            assert_eq!(a.client(seen(at, 60.0, 0, Some(8.0)), 10_000, ms(at)), None);
            // A fall every three seconds, lasting a second.
            if second % 3 == 0 {
                for step in [200, 400, 600, 800] {
                    a.estimate(6_000_000, ms(at + step));
                }
            } else {
                a.estimate(15_000_000, ms(at + 500));
            }
            restarts += u32::from(a.take_restart().is_some());
            assert_eq!(a.decide(10_000, false, ms(at + 900)), None, "never cut, at {second} s");
        }
        assert_eq!(a.ceiling_kbps(), None, "and no ceiling learnt from a link with room");
        assert!((15..=25).contains(&restarts), "the estimator is put back each time: {restarts}");
    }

    #[test]
    fn frames_arriving_short_twice_running_are_trouble_and_once_is_a_stall() {
        let t = Instant::now();
        let ms = |n: u64| t + Duration::from_millis(n);
        let mut a = busy_on_a_lan(t);
        for at in [1000, 2000] {
            assert_eq!(a.client(seen(at, 60.0, 0, Some(8.0)), 10_000, ms(at)), None);
        }
        // A Wi-Fi stall: six frames late in one second, made up in the next. The estimator
        // falls with it, and is overruled.
        assert_eq!(a.client(seen(3000, 54.0, 0, Some(8.0)), 10_000, ms(3000)), None);
        a.estimate(6_000_000, ms(3100));
        assert_eq!(a.decide(10_000, false, ms(3100)), None, "one short second is not believed");
        assert_eq!(a.client(seen(4000, 66.0, 0, Some(8.0)), 10_000, ms(4000)), None);
        assert_eq!(a.ceiling_kbps(), None);
        // A link that is full: frames keep arriving short, with nothing lost yet. The second
        // such sample is trouble, and undoes the pass the estimator's fall was given.
        for at in [5000, 6000, 7000, 8000] {
            assert_eq!(a.client(seen(at, 60.0, 0, Some(8.0)), 10_000, ms(at)), None);
        }
        a.estimate(15_000_000, ms(8000));
        assert_eq!(a.client(seen(9000, 54.0, 0, Some(8.0)), 10_000, ms(9000)), None);
        for at in [9100, 9300, 9500] {
            a.estimate(6_000_000, ms(at));
        }
        assert_eq!(a.decide(10_000, false, ms(9500)), None, "not yet");
        assert_eq!(a.client(seen(10_000, 54.0, 0, Some(8.0)), 10_000, ms(10_000)), None);
        assert_eq!(a.ceiling_kbps(), Some(8500));
        assert_eq!(a.decide(10_000, false, ms(10_000)), Some(8500));
        // And a fall that follows trouble is believed at once.
        a.sending(Some(8500));
        a.estimate(5_000_000, ms(10_500));
        assert_eq!(a.decide(8500, false, ms(10_500)), Some(4250), "bounded at half the ceiling");
    }

    #[test]
    fn a_round_trip_that_stays_long_is_trouble() {
        let t = Instant::now();
        let ms = |n: u64| t + Duration::from_millis(n);
        let mut a = busy_on_a_lan(t);
        for at in [1000, 2000, 3000] {
            assert_eq!(a.client(seen(at, 60.0, 0, Some(10.0)), 10_000, ms(at)), None);
        }
        // 30 ms on a best of 10 is Wi-Fi being Wi-Fi; 60 ms twice running is a queue.
        assert_eq!(a.client(seen(4000, 60.0, 0, Some(30.0)), 10_000, ms(4000)), None);
        assert_eq!(a.client(seen(5000, 60.0, 0, Some(60.0)), 10_000, ms(5000)), None);
        a.estimate(6_000_000, ms(5100));
        assert_eq!(a.decide(10_000, false, ms(5100)), None, "one long round trip: not yet");
        assert_eq!(a.client(seen(6000, 60.0, 0, Some(70.0)), 10_000, ms(6000)), None);
        assert_eq!(a.decide(10_000, false, ms(6000)), Some(8500));
    }

    #[test]
    fn the_estimators_first_word_leaves_the_starting_rate() {
        for (start, audio) in [(5000, 128), (3000, 96), (300, 128)] {
            let limits = Limits::new(1, start, 20_000);
            let mut a = Adaptation::new(limits, audio);
            let t = Instant::now();
            a.estimate(a.starting_estimate_kbps() * 1000, t);
            assert_eq!(a.decide(start, false, t), None, "{start} kbit/s is not cut at the start");
            // And it is the least that does: a little less would cut.
            let mut b = Adaptation::new(limits, audio);
            b.estimate((b.starting_estimate_kbps() - 3) * 1000, t);
            assert!(b.network_kbps.is_some_and(|kbps| kbps < start), "{start}");
        }
    }

    #[test]
    fn a_dropout_is_not_congestion() {
        let mut a = adaptation();
        let t = Instant::now();
        let ms = |n: u64| t + Duration::from_millis(n);
        // A steady 4.5 Mbit/s, then the link goes down: the estimate collapses and the stream
        // is cut at once, as for congestion.
        for at in [0, 200, 400] {
            a.estimate(4_500_000, ms(at));
        }
        let before = a.decide(5000, false, ms(400)).expect("down to what the network allows");
        for at in [600, 800, 1000, 1200] {
            a.estimate(400_000, ms(at));
        }
        let cut = a.decide(before, false, ms(1200)).expect("cut for the collapse");
        assert!(cut < 500, "{cut}");
        // The client is heard again at 2.6 s: the stream goes straight back to what it was
        // sent at before, with no ten-second hold and no stepping up.
        a.dropout(before, ms(2600));
        assert_eq!(a.decide(cut, false, ms(2600)), Some(before));
        // The estimator's first readings after the dropout still describe it: left alone.
        a.estimate(400_000, ms(2700));
        a.estimate(450_000, ms(3500));
        assert_eq!(a.decide(before, false, ms(3500)), None);
        // After the grace, the connection is judged again: real congestion still cuts.
        for at in [5700, 5900, 6100] {
            a.estimate(1_000_000, ms(at));
        }
        assert!(a.decide(before, false, ms(6100)).is_some_and(|kbps| kbps < before));
    }

    #[test]
    fn a_lone_dip_or_spike_moves_nothing_a_lasting_drop_does() {
        // The TV's session: 4.5 Mbit/s, then one reading of 0.9 and one of 17.8.
        let mut a = adaptation();
        let t = Instant::now();
        let ms = |n: u64| t + Duration::from_millis(n);
        for (at, bps) in [(0, 4_500_000), (200, 4_400_000), (400, 900_000), (600, 4_600_000)] {
            a.estimate(bps, ms(at));
        }
        assert_eq!(
            a.decide(5000, false, ms(600)),
            Some(3697),
            "the dip ignored: 85% of 4.5 Mbit/s, less the audio"
        );
        a.estimate(17_800_000, ms(800));
        assert_eq!(a.decide(3697, false, ms(800)), None, "the spike ignored");
        // Congestion that lasts: followed once it is most of the window (about a second).
        for at in [1000, 1200, 1400] {
            a.estimate(1_000_000, ms(at));
        }
        assert_eq!(
            a.decide(3697, false, ms(1400)),
            None,
            "not yet: still the median of mostly good"
        );
        for at in [1600, 1800] {
            a.estimate(1_000_000, ms(at));
        }
        assert_eq!(a.decide(3697, false, ms(1800)), Some(722), "a drop that lasts is followed");
        // Old estimates leave the window; the rise waits 10 s after the cut, then a quarter.
        a.estimate(6_000_000, ms(5000));
        assert_eq!(a.decide(722, false, ms(5000)), None, "too soon after the cut");
        assert_eq!(a.decide(722, false, ms(11_800)), Some(903), "then a quarter at a time");
    }

    /// A second of 60 frames sent, and what the client says of it.
    struct Client {
        now: Instant,
        sent: u64,
    }

    impl Client {
        fn new(a: &mut Adaptation) -> Self {
            let mut c = Self { now: Instant::now(), sent: 0 };
            // The first sample only sets the baseline.
            assert_eq!(c.second(a, 5000, Some(60.0), Some(4.0), Some(0)), None);
            c
        }

        fn second(
            &mut self,
            a: &mut Adaptation,
            current_kbps: u32,
            fps: Option<f64>,
            decode_ms: Option<f64>,
            frames_dropped: Option<u64>,
        ) -> Option<LimitChange> {
            self.now += Duration::from_secs(1);
            self.sent += 60;
            let sample = ClientSample {
                fps,
                decode_ms,
                frames_dropped,
                frames_sent: self.sent,
                packets_lost: None,
                rtt_ms: None,
            };
            a.client(sample, current_kbps, self.now)
        }

        fn well(&mut self, a: &mut Adaptation, current_kbps: u32) -> Option<LimitChange> {
            self.second(a, current_kbps, Some(60.0), Some(4.0), Some(0))
        }
    }

    #[test]
    fn limits_keep_the_starting_rate_inside_them() {
        assert_eq!(Limits::new(300, 5000, 12_000), LIMITS);
        let l = Limits::new(8000, 5000, 2500);
        assert_eq!((l.min_kbps, l.start_kbps, l.max_kbps), (5000, 5000, 5000));
    }

    #[test]
    fn a_stream_starts_at_the_starting_rate_then_follows_the_estimate() {
        let mut a = adaptation();
        let t = Instant::now();
        assert_eq!(a.decide(5000, false, t), None, "nothing estimated: stay at the start");
        // 85% of 10 Mbit/s, less the audio, is 8372: there is headroom, so the rate rises, a
        // quarter at a time.
        alone(&mut a, 10_000_000);
        assert_eq!(a.decide(5000, false, t), Some(6250));
        alone(&mut a, 100_000_000);
        let s = |n: u64| t + Duration::from_secs(n);
        assert_eq!(a.decide(6250, false, s(1)), None, "raises are 2 s apart");
        assert_eq!(a.decide(6250, false, s(2)), Some(7813));
        assert_eq!(a.decide(7813, false, s(4)), Some(9767));
        assert_eq!(a.decide(9767, false, s(6)), Some(12_000), "never above the ceiling");
        // Cuts are taken at once, all the way.
        alone(&mut a, 2_000_000);
        assert_eq!(a.decide(12_000, false, s(7)), Some(1572));
        alone(&mut a, 100_000);
        assert_eq!(a.decide(1572, false, s(8)), Some(300), "never below the floor");
    }

    #[test]
    fn small_changes_are_ignored() {
        let mut a = adaptation();
        alone(&mut a, 6_500_000); // 5397
        assert_eq!(a.decide(5000, false, Instant::now()), None);
    }

    #[test]
    fn an_encoder_that_reopens_is_raised_rarely_and_cut_at_once() {
        let mut a = adaptation();
        let t = Instant::now();
        alone(&mut a, 7_000_000); // 5822: a sixth more, not worth a reopen
        assert_eq!(a.decide(5000, true, t), None);
        assert_eq!(a.decide(5000, false, t), Some(5822), "worth it where a change is free");

        let mut a = adaptation();
        alone(&mut a, 4_000_000); // 3272
        assert_eq!(a.decide(5000, true, t), Some(3272), "a cut is not held back");
        alone(&mut a, 10_000_000); // 8372
        assert_eq!(a.decide(3272, true, t + Duration::from_secs(5)), None, "too soon to raise");
        alone(&mut a, 3_000_000); // 2422
        assert_eq!(a.decide(3272, true, t + Duration::from_secs(6)), Some(2422));
        alone(&mut a, 10_000_000);
        // 10 s after the last cut, a quarter more (always worth a reopen).
        assert_eq!(a.decide(2422, true, t + Duration::from_secs(16)), Some(3028));
    }

    #[test]
    fn a_client_that_falls_behind_is_given_less_then_more_again() {
        let mut a = adaptation();
        alone(&mut a, 100_000_000);
        let mut c = Client::new(&mut a);
        // One bad second is noise; two are a decoder that cannot keep up (14 ms of a 16.7 ms
        // frame: busy).
        assert_eq!(c.second(&mut a, 12_000, Some(40.0), Some(14.0), Some(0)), None);
        assert_eq!(
            c.second(&mut a, 12_000, Some(40.0), Some(14.0), Some(0)),
            Some(LimitChange::Cut { to_kbps: 8400, why: Struggle::Behind })
        );
        assert_eq!(a.decide(12_000, false, c.now), Some(8400), "under the network's 12000");

        // Well again for five seconds: a step back up, and in time no limit at all.
        for _ in 0..4 {
            assert_eq!(c.well(&mut a, 8400), None);
        }
        assert_eq!(c.well(&mut a, 8400), Some(LimitChange::Raised { to_kbps: 10_500 }));
        for _ in 0..4 {
            assert_eq!(c.well(&mut a, 10_500), None);
        }
        assert_eq!(c.well(&mut a, 10_500), Some(LimitChange::Lifted));
        assert_eq!(a.client_limit_kbps(), None);
        assert_eq!(a.decide(10_500, false, c.now), Some(12_000));
    }

    #[test]
    fn frames_lost_on_the_way_are_not_the_decoders_doing() {
        // The TV's case: 25 of 60 frames decoded, each in 4-7 ms. The frames never arrived (the
        // network estimate had just fallen): no client limit on top of the network's.
        let mut a = adaptation();
        alone(&mut a, 100_000_000);
        let mut c = Client::new(&mut a);
        for decode_ms in [4.0, 6.8, 5.5, 7.0] {
            assert_eq!(c.second(&mut a, 430, Some(25.0), Some(decode_ms), Some(0)), None);
        }
        assert_eq!(a.client_limit_kbps(), None);
        // A client that reports no decode time is believed when it says it is behind.
        assert_eq!(c.second(&mut a, 12_000, Some(30.0), None, Some(0)), None);
        assert!(matches!(
            c.second(&mut a, 12_000, Some(30.0), None, Some(0)),
            Some(LimitChange::Cut { why: Struggle::Behind, .. })
        ));
    }

    #[test]
    fn dropped_frames_and_a_climbing_decode_time_count_too() {
        let mut a = adaptation();
        let mut c = Client::new(&mut a);
        // 10 of 60 dropped in each second.
        c.second(&mut a, 5000, Some(60.0), Some(4.0), Some(10));
        assert_eq!(
            c.second(&mut a, 5000, Some(60.0), Some(4.0), Some(20)),
            Some(LimitChange::Cut { to_kbps: 3500, why: Struggle::Dropping })
        );

        let mut a = adaptation();
        let mut c = Client::new(&mut a);
        // 20 ms a frame where a frame lasts 16.7 ms and decoding took 4 ms at best.
        c.second(&mut a, 5000, Some(60.0), Some(20.0), Some(0));
        assert_eq!(
            c.second(&mut a, 5000, Some(60.0), Some(20.0), Some(0)),
            Some(LimitChange::Cut { to_kbps: 3500, why: Struggle::SlowDecode })
        );
    }

    #[test]
    fn a_decoder_that_is_always_slow_is_not_struggling() {
        // A TV's hardware decoder that takes 23 ms a frame at any bitrate, and keeps up.
        let mut a = adaptation();
        let mut c = Client { now: Instant::now(), sent: 0 };
        for _ in 0..10 {
            assert_eq!(c.second(&mut a, 5000, Some(60.0), Some(23.0), Some(0)), None);
        }
        assert_eq!(a.client_limit_kbps(), None);
    }

    #[test]
    fn a_paused_game_says_nothing_about_the_client() {
        let mut a = adaptation();
        let mut c = Client::new(&mut a);
        // No frames were sent, so none were decoded: not the client's doing.
        for _ in 0..5 {
            c.now += Duration::from_secs(1);
            let sample = ClientSample { fps: Some(0.0), frames_sent: c.sent, ..Default::default() };
            assert_eq!(a.client(sample, 5000, c.now), None);
        }
        assert_eq!(a.client_limit_kbps(), None);
    }

    #[test]
    fn a_limit_lapses_when_the_stats_stop() {
        let mut a = adaptation();
        alone(&mut a, 100_000_000);
        let mut c = Client::new(&mut a);
        c.second(&mut a, 12_000, Some(30.0), None, None);
        c.second(&mut a, 12_000, Some(30.0), None, None);
        assert_eq!(a.client_limit_kbps(), Some(8400));
        // The limit lapses: back towards the network's 12000, a quarter at a time.
        assert_eq!(a.decide(8400, false, c.now + Duration::from_secs(11)), Some(10_500));
        assert_eq!(a.client_limit_kbps(), None);
    }

    #[test]
    fn the_player_is_told_of_changes_worth_a_notice_and_not_too_often() {
        let mut a = adaptation();
        let t = Instant::now();
        assert_eq!(a.told(), (5000, LimitedBy::Nothing));
        assert_eq!(a.tell(5000, t), (5000, LimitedBy::Nothing));
        // A tenth less is not worth a notice, however long ago the last one was.
        assert_eq!(a.tell(4500, t + Duration::from_secs(60)), (5000, LimitedBy::Nothing));
        // A third less is, but not seconds after the last notice.
        assert_eq!(a.tell(3300, t + Duration::from_secs(5)), (5000, LimitedBy::Nothing));
        assert_eq!(a.tell(3300, t + Duration::from_secs(15)), (3300, LimitedBy::Connection));
        assert_eq!(a.told(), (3300, LimitedBy::Connection));
        assert_eq!(a.tell(8000, t + Duration::from_secs(30)), (8000, LimitedBy::Nothing));
    }

    #[test]
    fn a_fall_for_the_client_says_so() {
        let mut a = adaptation();
        alone(&mut a, 100_000_000);
        let mut c = Client::new(&mut a);
        a.tell(12_000, c.now);
        c.second(&mut a, 12_000, Some(30.0), None, None);
        c.second(&mut a, 12_000, Some(30.0), None, None);
        let later = c.now + Duration::from_secs(15);
        assert_eq!(a.tell(8400, later), (8400, LimitedBy::Client));
    }

    #[test]
    fn a_wavering_connection_is_followed_down_at_once_and_up_slowly() {
        let mut a = adaptation();
        let t = Instant::now();
        let s = |n: u64| t + Duration::from_secs(n);
        alone(&mut a, 7_000_000); // 5822
        assert_eq!(a.decide(5000, false, s(0)), Some(5822));
        // Down (2 Mbit/s: 1572), straight back up (7 Mbit/s): held, so no whiplash.
        alone(&mut a, 2_000_000);
        assert_eq!(a.decide(5822, false, s(1)), Some(1572), "down at once");
        alone(&mut a, 7_000_000);
        for n in [2, 4, 6, 8, 10] {
            assert_eq!(a.decide(1572, false, s(n)), None, "held at {n} s");
        }
        // Then up in steps, 2 s apart: 1965, 2457, 3072, 3840, 4800, 5822.
        let mut kbps = 1572;
        let mut steps = Vec::new();
        for n in (11..30).step_by(2) {
            if let Some(next) = a.decide(kbps, false, s(n)) {
                steps.push(next);
                kbps = next;
            }
        }
        assert_eq!(steps, [1965, 2457, 3072, 3840, 4800, 5822]);
    }

    #[test]
    fn a_new_client_starts_afresh() {
        let mut a = adaptation();
        alone(&mut a, 2_000_000);
        a.reset();
        assert_eq!(a.decide(1572, false, Instant::now()), Some(5000));
    }
}
