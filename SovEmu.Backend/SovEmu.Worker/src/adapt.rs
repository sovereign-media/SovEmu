//! Picks the video bitrate through a stream: it starts at the configured rate and then follows
//! two things, staying within the configured floor and ceiling.
//!
//! - **The connection**: the send-bandwidth estimate (transport feedback from the client's
//!   WebRTC), which rises while there is headroom and falls on congestion. Taken as the median
//!   of the last [`ESTIMATE_WINDOW`]: on a home network the estimate jumps (0.9 to 17.8 Mbit/s
//!   within seconds, measured to a TV while a PS2 game ran slow and its frames went out in
//!   bursts), and following each jump moved the quality up and down every few seconds. A dip or
//!   a spike alone now moves nothing; a drop that lasts is followed within half the window.
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

/// The most one raise takes the bitrate to: a quarter more (rounded up, so it is always worth
/// an encoder's reopen).
fn raise_step(kbps: u32) -> u32 {
    kbps.saturating_add(kbps.div_ceil(4))
}

/// The estimates the connection is judged on: those of this long past, by their median.
const ESTIMATE_WINDOW: Duration = Duration::from_secs(2);
/// At most this many estimates are kept (str0m reports several a second).
const ESTIMATES_KEPT: usize = 64;

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
    audio_kbps: u32,
    /// What the estimate leaves for video; none before the first one.
    network_kbps: Option<u32>,
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
    /// When the bitrate was last cut, and last raised.
    cut_at: Option<Instant>,
    raised_at: Option<Instant>,
    /// The bitrate the player was last told of, why, and when; none before the first.
    told: Option<(u32, LimitedBy, Instant)>,
}

impl Adaptation {
    pub(crate) fn new(limits: Limits, audio_kbps: u32) -> Self {
        Self {
            limits,
            audio_kbps,
            network_kbps: None,
            estimates: VecDeque::new(),
            client_kbps: None,
            bad: 0,
            good: 0,
            best_decode_ms: None,
            last: None,
            changed_at: None,
            cut_at: None,
            raised_at: None,
            told: None,
        }
    }

    pub(crate) fn limits(&self) -> Limits {
        self.limits
    }

    /// A new client: nothing is known about its connection or its decoder.
    pub(crate) fn reset(&mut self) {
        *self = Self::new(self.limits, self.audio_kbps);
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
    pub(crate) fn estimate(&mut self, estimate_bps: u64, now: Instant) {
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
        let Limits { min_kbps, max_kbps, .. } = self.limits;
        self.network_kbps = Some(usable.clamp(i64::from(min_kbps), i64::from(max_kbps)) as u32);
    }

    /// Takes a client's stats, with the bitrate in use while it measured them.
    pub(crate) fn client(
        &mut self,
        sample: ClientSample,
        current_kbps: u32,
        now: Instant,
    ) -> Option<LimitChange> {
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
        let network = self.network_kbps.unwrap_or(self.limits.start_kbps);
        self.client_kbps.map_or(network, |limit| network.min(limit))
    }

    /// The bitrate to change to, if the change is worth making. `reopens`: the encoder in use
    /// reopens (with a keyframe) to change its rate.
    pub(crate) fn decide(&mut self, current_kbps: u32, reopens: bool, now: Instant) -> Option<u32> {
        let mut target = self.target(now);
        // Before the first estimate the target is the starting rate, taken at once (a new client
        // starts afresh); after it, raises are held and stepped.
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

    /// An estimate on its own: each well after the last, beyond the window, so it alone counts.
    fn alone(a: &mut Adaptation, bps: u64) {
        use std::sync::atomic::{AtomicU64, Ordering};
        static BASE: std::sync::LazyLock<Instant> = std::sync::LazyLock::new(Instant::now);
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        a.estimate(bps, *BASE + ESTIMATE_WINDOW * 5 * (n as u32 + 1));
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
            let sample = ClientSample { fps, decode_ms, frames_dropped, frames_sent: self.sent };
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
