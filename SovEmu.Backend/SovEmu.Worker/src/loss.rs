//! Simulated packet loss, for testing how the stream recovers (sovereign-media/SovEmu#189).
//!
//! `--drop-video-percent` makes the worker throw away that share of the video packets it would
//! send, retransmissions included, as a lossy link would. Audio, RTCP and the connection's own
//! packets (STUN, DTLS) always go, so the client still connects, still hears the game and can
//! still ask for what it lost. Off (0) outside tests and measurements.
//!
//! Which packets go is decided by a fixed sequence, so two runs at the same share lose the same
//! packets and can be compared.

/// Drops a share of outgoing video packets.
#[derive(Debug)]
pub(crate) struct Loss {
    /// The share to drop, 0 to 1.
    share: f64,
    /// The sequence's state (xorshift64*); never zero.
    state: u64,
    /// Video packets seen and dropped, for the log.
    pub(crate) seen: u64,
    pub(crate) dropped: u64,
}

impl Loss {
    /// `percent` of video packets are dropped: 0 (or anything not a share) drops none.
    pub(crate) fn new(percent: f64) -> Self {
        let share = if percent.is_finite() { (percent / 100.0).clamp(0.0, 1.0) } else { 0.0 };
        Self { share, state: 0x9E37_79B9_7F4A_7C15, seen: 0, dropped: 0 }
    }

    /// Whether any packet can be dropped.
    pub(crate) fn active(&self) -> bool {
        self.share > 0.0
    }

    /// Whether to throw this outgoing datagram away. `audio` is the audio's RTP payload type,
    /// once negotiated: its packets always go.
    pub(crate) fn drops(&mut self, datagram: &[u8], audio: Option<u8>) -> bool {
        if !self.active() || !is_video_rtp(datagram, audio) {
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
        let mut loss = Loss::new(100.0);
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
            let mut loss = Loss::new(percent);
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
    fn nothing_is_dropped_unless_asked() {
        for percent in [0.0, -3.0, f64::NAN] {
            let mut loss = Loss::new(percent);
            assert!(!loss.active());
            assert!((0..1000).all(|_| !loss.drops(&rtp(96), None)));
            assert_eq!(loss.seen, 0);
        }
    }
}
