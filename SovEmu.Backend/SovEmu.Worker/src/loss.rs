//! A simulated lossy, slow link, for testing how the stream recovers (sovereign-media/SovEmu#189).
//!
//! `--drop-video-percent` makes the worker throw away that share of the video packets it would
//! send, retransmissions included, as a lossy link would. `--delay-video-ms` holds each video
//! packet that long before it goes, as a distant client's link would: a retransmission then
//! arrives late, which is when a client gives up on it and asks for a keyframe.
//! `--drop-video-burst-ms` cuts the link both ways for that long once every [`BURST_EVERY`],
//! as a Wi-Fi dropout does: nothing goes out and nothing the client sends is heard.
//! `--limit-video-kbps` makes the link carry no more video than that, as a slow internet
//! connection does: packets leave one after another at that rate, wait in a queue of up to
//! [`QUEUE`] when they come faster, and are thrown away when the queue is full.
//! `--jitter-video-ms` holds each video packet a different time, from nothing up to that
//! long, without putting any out of order, as Wi-Fi and busy routes do. Outside
//! a dropout, audio, RTCP
//! and the connection's own packets (STUN, DTLS) always go at once, so the client still
//! connects, still hears the game and can still ask for what it lost. All are off (0) outside
//! tests and measurements.
//!
//! Which packets go is decided by a fixed sequence, so two runs at the same share lose the same
//! packets and can be compared.

use std::collections::VecDeque;
use std::net::{SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

/// How often a burst of loss comes, when there is one.
pub(crate) const BURST_EVERY: Duration = Duration::from_secs(10);

/// The longest a video packet waits for a limited link before it is thrown away instead: a
/// home router's buffer.
pub(crate) const QUEUE: Duration = Duration::from_millis(300);

/// Drops a share of outgoing video packets.
#[derive(Debug)]
pub(crate) struct Loss {
    /// The share to drop, 0 to 1.
    share: f64,
    /// How long each video packet is held before it is sent.
    delay: Duration,
    /// How long every video packet is dropped for, once every [`BURST_EVERY`].
    burst: Duration,
    /// When the first video packet went: bursts are timed from it.
    started: Option<Instant>,
    /// The most video the link carries, bits a second; 0 for no limit.
    limit_bps: u64,
    /// The most a video packet is held on top of the delay; each is held a different time.
    jitter: Duration,
    /// When a limited link has sent everything given to it so far.
    link_free: Option<Instant>,
    /// When the last packet held may go: none goes before one sent ahead of it.
    last_due: Option<Instant>,
    /// Video packets thrown away because a limited link's queue was full.
    pub(crate) overflowed: u64,
    /// Video packets waiting out the delay, oldest first, with when each may go.
    held: VecDeque<(Instant, Vec<u8>, SocketAddr)>,
    /// The sequence's state (xorshift64*); never zero.
    state: u64,
    /// Video packets seen and dropped, for the log.
    pub(crate) seen: u64,
    pub(crate) dropped: u64,
}

impl Loss {
    /// `percent` of video packets are dropped (0, or anything not a share, drops none) and
    /// the rest are held for `delay_ms`.
    pub(crate) fn new(percent: f64, delay_ms: u32, burst_ms: u32) -> Self {
        let share = if percent.is_finite() { (percent / 100.0).clamp(0.0, 1.0) } else { 0.0 };
        Self {
            share,
            delay: Duration::from_millis(u64::from(delay_ms)),
            burst: Duration::from_millis(u64::from(burst_ms)).min(BURST_EVERY),
            started: None,
            limit_bps: 0,
            jitter: Duration::ZERO,
            link_free: None,
            last_due: None,
            overflowed: 0,
            held: VecDeque::new(),
            state: 0x9E37_79B9_7F4A_7C15,
            seen: 0,
            dropped: 0,
        }
    }

    /// The same link, carrying at most `limit_kbps` of video (0: no limit) and holding each
    /// video packet up to `jitter_ms` more than the delay.
    pub(crate) fn shaped(mut self, limit_kbps: u32, jitter_ms: u32) -> Self {
        self.limit_bps = u64::from(limit_kbps) * 1000;
        self.jitter = Duration::from_millis(u64::from(jitter_ms));
        self
    }

    /// Whether any packet can be dropped or held.
    pub(crate) fn active(&self) -> bool {
        self.share > 0.0 || !self.burst.is_zero() || self.holds()
    }

    /// Whether video packets are held before they go.
    fn holds(&self) -> bool {
        !self.delay.is_zero() || !self.jitter.is_zero() || self.limit_bps > 0
    }

    /// The link's limit, kbit/s (0: none), and the most it varies a packet's delay by, ms.
    pub(crate) fn shape(&self) -> (u64, u64) {
        (self.limit_bps / 1000, self.jitter.as_millis() as u64)
    }

    /// The next number of the sequence, from 0 up to (not including) 1.
    fn roll(&mut self) -> f64 {
        // xorshift64*: plenty for choosing packets, and the same every run.
        self.state ^= self.state >> 12;
        self.state ^= self.state << 25;
        self.state ^= self.state >> 27;
        (self.state.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Whether `now` falls in a burst: the last [`Self::burst`] of every [`BURST_EVERY`]
    /// since the first video packet, so a stream always starts cleanly.
    fn in_burst(&mut self, now: Instant) -> bool {
        if self.burst.is_zero() {
            return false;
        }
        let since = now.saturating_duration_since(*self.started.get_or_insert(now));
        let into = Duration::from_nanos((since.as_nanos() % BURST_EVERY.as_nanos()) as u64);
        into >= BURST_EVERY - self.burst
    }

    /// The delay each video packet is held for, in milliseconds.
    pub(crate) fn delay_ms(&self) -> u64 {
        self.delay.as_millis() as u64
    }

    /// Takes an outgoing datagram at `now`. `Some`: send it now. `None`: it was dropped, or it
    /// is held and [`Self::due`] gives it back when its delay is up.
    pub(crate) fn outgoing(
        &mut self,
        datagram: &[u8],
        destination: SocketAddr,
        audio: Option<u8>,
        now: Instant,
    ) -> Option<()> {
        if self.drops(datagram, audio) {
            return None;
        }
        // A dropout takes everything, audio and the connection's own packets included.
        let video = is_video_rtp(datagram, audio);
        if (video || self.started.is_some()) && self.in_burst(now) {
            self.seen += u64::from(video && self.share <= 0.0);
            self.dropped += u64::from(video);
            return None;
        }
        if !video || !self.holds() {
            return Some(());
        }
        // Counted here unless the share's roll has counted it already.
        self.seen += u64::from(self.share <= 0.0);
        // A limited link sends one packet after another, each taking its size's time.
        let mut leaves = now;
        let bits = datagram.len() as u64 * 8;
        if let Some(nanos) = (bits * 1_000_000_000).checked_div(self.limit_bps) {
            let starts = self.link_free.map_or(now, |free| free.max(now));
            if starts.saturating_duration_since(now) > QUEUE {
                self.overflowed += 1;
                return None;
            }
            leaves = starts + Duration::from_nanos(nanos);
            self.link_free = Some(leaves);
        }
        let jitter = self.jitter.mul_f64(if self.jitter.is_zero() { 0.0 } else { self.roll() });
        // Never ahead of a packet sent before it: a link varies its delay, not its order.
        let due = (leaves + self.delay + jitter).max(self.last_due.unwrap_or(now));
        self.last_due = Some(due);
        self.held.push_back((due, datagram.to_vec(), destination));
        None
    }

    /// Whether a datagram arriving from the client at `now` is lost to a dropout.
    pub(crate) fn incoming_lost(&mut self, now: Instant) -> bool {
        self.started.is_some() && self.in_burst(now)
    }

    /// The held packets whose delay is up at `now`, oldest first.
    pub(crate) fn due(&mut self, now: Instant) -> impl Iterator<Item = (Vec<u8>, SocketAddr)> + '_ {
        std::iter::from_fn(move || {
            let (at, ..) = self.held.front()?;
            if *at > now {
                return None;
            }
            self.held.pop_front().map(|(_, datagram, destination)| (datagram, destination))
        })
    }

    /// When the next held packet may go.
    pub(crate) fn next_due(&self) -> Option<Instant> {
        self.held.front().map(|(at, ..)| *at)
    }

    /// Sends every held packet that is due.
    pub(crate) fn flush(&mut self, socket: &UdpSocket, now: Instant) {
        for (datagram, destination) in self.due(now).collect::<Vec<_>>() {
            let _ = socket.send_to(&datagram, destination);
        }
    }

    /// Whether to throw this outgoing datagram away. `audio` is the audio's RTP payload type,
    /// once negotiated: its packets always go.
    pub(crate) fn drops(&mut self, datagram: &[u8], audio: Option<u8>) -> bool {
        if self.share <= 0.0 || !is_video_rtp(datagram, audio) {
            return false;
        }
        self.seen += 1;
        let drop = self.roll() < self.share;
        self.dropped += u64::from(drop);
        drop
    }
}

/// Whether a datagram is an RTP packet that is not audio: video, or a video retransmission.
/// RTP and RTCP both start with version 2; RTCP's packet types (192 to 223) sit where RTP has
/// its marker bit and payload type, and STUN and DTLS start with other bytes (RFC 7983).
fn is_video_rtp(datagram: &[u8], audio: Option<u8>) -> bool {
    let [first, second, ..] = datagram else { return false };
    if first >> 6 != 2 || (192..=223).contains(second) {
        return false;
    }
    Some(second & 0x7F) != audio
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An RTP packet's first bytes, with this payload type.
    fn rtp(payload_type: u8) -> [u8; 12] {
        [0x80, payload_type, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1]
    }

    #[test]
    fn only_video_packets_are_ever_dropped() {
        let mut loss = Loss::new(100.0, 0, 0);
        assert!(loss.drops(&rtp(96), Some(111)), "video");
        assert!(loss.drops(&rtp(96 | 0x80), Some(111)), "video with its marker bit");
        assert!(loss.drops(&rtp(97), Some(111)), "a video retransmission");
        assert!(!loss.drops(&rtp(111), Some(111)), "audio");
        assert!(!loss.drops(&[0x80, 200, 0, 6], Some(111)), "an RTCP sender report");
        assert!(!loss.drops(&[0x81, 206, 0, 2], Some(111)), "an RTCP feedback packet");
        assert!(!loss.drops(&[0x00, 0x01, 0, 0], Some(111)), "STUN");
        assert!(!loss.drops(&[22, 0xFE, 0xFD, 0], Some(111)), "DTLS");
        assert!(!loss.drops(&[], Some(111)));
        assert_eq!((loss.seen, loss.dropped), (3, 3));
    }

    #[test]
    fn the_share_dropped_is_the_share_asked_for_and_the_same_every_run() {
        let run = |percent| {
            let mut loss = Loss::new(percent, 0, 0);
            let dropped: Vec<bool> = (0..20_000).map(|_| loss.drops(&rtp(96), None)).collect();
            (loss.dropped, dropped)
        };
        let (one, first) = run(1.0);
        assert!((150..=250).contains(&one), "1% of 20000 is about 200: {one}");
        assert_eq!(run(1.0).1, first, "the same packets every run");
        let (five, _) = run(5.0);
        assert!((900..=1100).contains(&five), "{five}");
    }

    #[test]
    fn video_packets_are_held_for_the_delay_and_everything_else_goes_at_once() {
        let to: SocketAddr = "127.0.0.1:50000".parse().unwrap();
        let t = Instant::now();
        let ms = Duration::from_millis;
        let mut link = Loss::new(0.0, 80, 0);
        assert!(link.active());
        // Audio and RTCP are not held: the client hears the game and its requests are answered.
        assert_eq!(link.outgoing(&rtp(111), to, Some(111), t), Some(()));
        assert_eq!(link.outgoing(&[0x80, 200, 0, 6], to, Some(111), t), Some(()));
        // Two video packets, 10 ms apart, each go 80 ms after they were to.
        assert_eq!(link.outgoing(&rtp(96), to, Some(111), t), None);
        assert_eq!(link.outgoing(&rtp(97), to, Some(111), t + ms(10)), None);
        assert_eq!(link.next_due(), Some(t + ms(80)));
        assert_eq!(link.due(t + ms(79)).count(), 0);
        let first: Vec<_> = link.due(t + ms(80)).collect();
        assert_eq!(first, [(rtp(96).to_vec(), to)]);
        assert_eq!(link.next_due(), Some(t + ms(90)));
        assert_eq!(link.due(t + ms(200)).count(), 1);
        assert_eq!(link.next_due(), None);
        // With no delay nothing is held.
        let mut plain = Loss::new(0.0, 0, 0);
        assert_eq!(plain.outgoing(&rtp(96), to, Some(111), t), Some(()));
        assert_eq!(plain.next_due(), None);
    }

    #[test]
    fn a_burst_drops_every_video_packet_for_its_length_once_a_period() {
        let to: SocketAddr = "127.0.0.1:50000".parse().unwrap();
        let t = Instant::now();
        let ms = Duration::from_millis;
        let mut link = Loss::new(0.0, 0, 200);
        assert!(link.active());
        // The stream starts cleanly: the burst is the last 200 ms of each 10 seconds.
        assert_eq!(link.outgoing(&rtp(96), to, Some(111), t), Some(()));
        assert_eq!(link.outgoing(&rtp(96), to, Some(111), t + ms(9_799)), Some(()));
        assert_eq!(link.outgoing(&rtp(96), to, Some(111), t + ms(9_800)), None);
        assert_eq!(
            link.outgoing(&rtp(97), to, Some(111), t + ms(9_999)),
            None,
            "retransmissions too"
        );
        // A dropout cuts the link both ways: audio does not go, and the client is not heard.
        assert_eq!(link.outgoing(&rtp(111), to, Some(111), t + ms(9_900)), None, "audio too");
        assert!(link.incoming_lost(t + ms(9_900)));
        assert!(!link.incoming_lost(t + ms(9_700)));
        assert_eq!(link.outgoing(&rtp(96), to, Some(111), t + ms(10_000)), Some(()));
        assert!(!link.incoming_lost(t + ms(10_000)));
        assert_eq!(link.outgoing(&rtp(96), to, Some(111), t + ms(19_850)), None, "and again");
        assert_eq!(link.dropped, 3, "video packets are what is counted");
        // Before any video has gone there is no dropout: the client can connect.
        let mut fresh = Loss::new(0.0, 0, 200);
        assert!(!fresh.incoming_lost(t));
        assert_eq!(fresh.outgoing(&rtp(111), to, Some(111), t), Some(()));
    }

    #[test]
    fn a_limited_link_sends_at_its_rate_queues_what_comes_faster_and_drops_the_overflow() {
        let to: SocketAddr = "127.0.0.1:50000".parse().unwrap();
        let t = Instant::now();
        let ms = Duration::from_millis;
        // 1,000 kbit/s: a 1,250-byte packet (10,000 bits) takes 10 ms.
        let packet = {
            let mut packet = vec![0u8; 1250];
            packet[..12].copy_from_slice(&rtp(96));
            packet
        };
        let mut link = Loss::new(0.0, 0, 0).shaped(1000, 0);
        assert!(link.active());
        assert_eq!(link.shape(), (1000, 0));
        // A frame of five packets at once leaves over 50 ms, in order.
        for _ in 0..5 {
            assert_eq!(link.outgoing(&packet, to, Some(111), t), None);
        }
        assert_eq!(link.next_due(), Some(t + ms(10)));
        assert_eq!(link.due(t + ms(30)).count(), 3);
        assert_eq!(link.due(t + ms(50)).count(), 2);
        // An idle link sends the next packet as soon as it has been given whole.
        assert_eq!(link.outgoing(&packet, to, Some(111), t + ms(100)), None);
        assert_eq!(link.next_due(), Some(t + ms(110)));
        assert_eq!(link.due(t + ms(110)).count(), 1);
        // Audio is not limited.
        assert_eq!(link.outgoing(&rtp(111), to, Some(111), t + ms(100)), Some(()));
        // Far more than the link carries: the queue holds 300 ms of it and the rest is lost.
        let burst = t + ms(1000);
        for _ in 0..100 {
            let _ = link.outgoing(&packet, to, Some(111), burst);
        }
        assert_eq!(link.overflowed, 69, "31 packets fit: the last starts 300 ms in");
        assert_eq!(link.seen, 106, "every video packet given to the link is counted");
        assert_eq!(link.due(burst + ms(310)).count(), 31);
        assert_eq!(link.next_due(), None);
        // The delay comes on top of the wait for the link.
        let mut far = Loss::new(0.0, 80, 0).shaped(1000, 0);
        assert_eq!(far.outgoing(&packet, to, Some(111), t), None);
        assert_eq!(far.next_due(), Some(t + ms(90)));
    }

    #[test]
    fn jitter_holds_each_packet_a_different_time_and_keeps_their_order() {
        let to: SocketAddr = "127.0.0.1:50000".parse().unwrap();
        let t = Instant::now();
        let ms = Duration::from_millis;
        let run = || {
            let mut link = Loss::new(0.0, 20, 0).shaped(0, 30);
            assert!(link.active());
            let mut waits = Vec::new();
            for n in 0..500u64 {
                let sent = t + ms(n * 5);
                let mut packet = rtp(96);
                packet[2..4].copy_from_slice(&(n as u16).to_be_bytes());
                assert_eq!(link.outgoing(&packet, to, None, sent), None);
                let due = link.held.back().unwrap().0;
                waits.push(due.duration_since(sent));
            }
            let order: Vec<Vec<u8>> = link.due(t + ms(10_000)).map(|(packet, _)| packet).collect();
            (waits, order)
        };
        let (waits, order) = run();
        // The delay at least, and no more than the delay and the jitter, but for a packet
        // kept behind the one before it (sent 5 ms earlier).
        assert!(waits.iter().all(|wait| *wait >= ms(20) && *wait < ms(50)), "{waits:?}");
        let (least, most) = (waits.iter().min().unwrap(), waits.iter().max().unwrap());
        assert!(*least < ms(25) && *most > ms(45), "the whole range is used: {least:?} {most:?}");
        let numbers: Vec<u16> = order.iter().map(|p| u16::from_be_bytes([p[2], p[3]])).collect();
        assert!(numbers.is_sorted(), "packets leave in the order they were sent");
        assert_eq!(numbers.len(), 500);
        assert_eq!(run().0, waits, "the same every run");
    }

    #[test]
    fn nothing_is_dropped_unless_asked() {
        for percent in [0.0, -3.0, f64::NAN] {
            let mut loss = Loss::new(percent, 0, 0);
            assert!(!loss.active());
            assert!((0..1000).all(|_| !loss.drops(&rtp(96), None)));
            assert_eq!(loss.seen, 0);
        }
    }
}
