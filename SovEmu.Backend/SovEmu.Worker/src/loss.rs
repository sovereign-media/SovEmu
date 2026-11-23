//! A simulated lossy, slow link, for testing how the stream recovers (sovereign-media/SovEmu#189).
//!
//! `--drop-video-percent` makes the worker throw away that share of the video packets it would
//! send, retransmissions included, as a lossy link would. `--delay-video-ms` holds each video
//! packet that long before it goes, as a distant client's link would: a retransmission then
//! arrives late, which is when a client gives up on it and asks for a keyframe.
//! `--drop-video-burst-ms` throws away every video packet for that long once every
//! [`BURST_EVERY`], as a Wi-Fi dropout would. Audio, RTCP
//! and the connection's own packets (STUN, DTLS) always go at once, so the client still
//! connects, still hears the game and can still ask for what it lost. Both are off (0) outside
//! tests and measurements.
//!
//! Which packets go is decided by a fixed sequence, so two runs at the same share lose the same
//! packets and can be compared.

use std::collections::VecDeque;
use std::net::{SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

/// How often a burst of loss comes, when there is one.
pub(crate) const BURST_EVERY: Duration = Duration::from_secs(10);

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
            held: VecDeque::new(),
            state: 0x9E37_79B9_7F4A_7C15,
            seen: 0,
            dropped: 0,
        }
    }

    /// Whether any packet can be dropped or held.
    pub(crate) fn active(&self) -> bool {
        self.share > 0.0 || !self.delay.is_zero() || !self.burst.is_zero()
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
        if is_video_rtp(datagram, audio) && self.in_burst(now) {
            self.seen += u64::from(self.share <= 0.0);
            self.dropped += 1;
            return None;
        }
        if self.delay.is_zero() || !is_video_rtp(datagram, audio) {
            return Some(());
        }
        self.held.push_back((now + self.delay, datagram.to_vec(), destination));
        None
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
        // xorshift64*: plenty for choosing packets, and the same every run.
        self.state ^= self.state >> 12;
        self.state ^= self.state << 25;
        self.state ^= self.state >> 27;
        let roll =
            (self.state.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / (1u64 << 53) as f64;
        let drop = roll < self.share;
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
        assert_eq!(
            link.outgoing(&rtp(111), to, Some(111), t + ms(9_900)),
            Some(()),
            "audio still goes"
        );
        assert_eq!(link.outgoing(&rtp(96), to, Some(111), t + ms(10_000)), Some(()));
        assert_eq!(link.outgoing(&rtp(96), to, Some(111), t + ms(19_850)), None, "and again");
        assert_eq!(link.dropped, 3);
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
