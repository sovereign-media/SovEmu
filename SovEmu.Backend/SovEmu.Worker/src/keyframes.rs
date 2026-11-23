//! Which of a client's keyframe requests are answered (sovereign-media/SovEmu#189, part 2).
//!
//! A client that loses packets it cannot get resent asks for a keyframe (a PLI or FIR), and
//! during a burst of loss it can ask several times within one round trip. Every keyframe is
//! several times an ordinary frame, so each one answered is another spike on a link that is
//! already struggling, and the frames behind it arrive late. A request that arrives before the
//! last keyframe could have reached the client says nothing new: the client had not seen that
//! keyframe when it asked. Such requests are ignored, as Sunshine ignores IDR requests that
//! come before its last IDR could have landed.

use std::time::{Duration, Instant};

/// The round trip assumed before a client has reported one.
const DEFAULT_RTT: Duration = Duration::from_millis(100);
/// The shortest and longest a keyframe is taken to need to reach the client and its next
/// request to come back: a reported round trip of a millisecond still has a frame's encode,
/// send and decode around it, and a wild one must not leave a broken picture up for long.
const HOLD_RANGE: (Duration, Duration) = (Duration::from_millis(50), Duration::from_millis(500));

/// Decides which keyframe requests to answer.
#[derive(Debug)]
pub(crate) struct Gate {
    /// When the last keyframe was sent, whatever caused it.
    sent_at: Option<Instant>,
    /// When a request was answered whose keyframe has not been encoded yet.
    pending_since: Option<Instant>,
    /// The client's network round trip, as it last reported it.
    rtt: Option<Duration>,
}

impl Gate {
    pub(crate) fn new() -> Self {
        Self { sent_at: None, pending_since: None, rtt: None }
    }

    /// Takes the client's reported network round trip, in milliseconds.
    pub(crate) fn round_trip(&mut self, ms: f64) {
        if ms.is_finite() && ms >= 0.0 {
            self.rtt = Some(Duration::from_secs_f64(ms / 1000.0));
        }
    }

    /// A new client: nothing is known of its round trip, and its first keyframe is its own.
    pub(crate) fn new_client(&mut self) {
        *self = Self::new();
    }

    /// How long after a keyframe a request is taken to have been sent before it arrived.
    fn hold(&self) -> Duration {
        self.rtt.unwrap_or(DEFAULT_RTT).clamp(HOLD_RANGE.0, HOLD_RANGE.1)
    }

    /// A client asked for a keyframe at `now`. True: make one. False: one is already on its
    /// way to be encoded, or the last one cannot have reached the client yet.
    pub(crate) fn asked(&mut self, now: Instant) -> bool {
        // An answered request waits for the next frame to be encoded. Should that keyframe
        // never come (the encoder dropped the frame, or the game is paused), the wait ends
        // after the longest hold, so a client is never left asking for good.
        let waiting = |since: Instant, wait: Duration| now.saturating_duration_since(since) < wait;
        if self.pending_since.is_some_and(|since| waiting(since, HOLD_RANGE.1)) {
            return false;
        }
        if self.sent_at.is_some_and(|at| waiting(at, self.hold())) {
            return false;
        }
        self.pending_since = Some(now);
        true
    }

    /// A keyframe was encoded and sent at `now` (asked for, or the encoder's own).
    pub(crate) fn sent(&mut self, now: Instant) {
        self.sent_at = Some(now);
        self.pending_since = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: Duration = Duration::from_millis(1);

    #[test]
    fn a_burst_of_requests_makes_one_keyframe() {
        let mut gate = Gate::new();
        let t = Instant::now();
        // Three PLIs in a burst, before the encoder has run: one keyframe.
        assert!(gate.asked(t));
        assert!(!gate.asked(t + MS));
        assert!(!gate.asked(t + 2 * MS));
        gate.sent(t + 5 * MS);
        // More arrive before that keyframe could have reached the client (no round trip
        // reported yet: 100 ms is assumed).
        assert!(!gate.asked(t + 30 * MS));
        assert!(!gate.asked(t + 104 * MS));
        // One sent after it had landed is a new loss.
        assert!(gate.asked(t + 105 * MS));
    }

    #[test]
    fn the_wait_follows_the_clients_round_trip_within_limits() {
        let t = Instant::now();
        let waits = |rtt_ms: f64, after_ms: u64| {
            let mut gate = Gate::new();
            gate.round_trip(rtt_ms);
            gate.sent(t);
            gate.asked(t + Duration::from_millis(after_ms))
        };
        // A LAN's millisecond still waits out a frame's encode, send and decode: 50 ms.
        assert!(!waits(1.0, 49));
        assert!(waits(1.0, 50));
        // A remote link's 180 ms is waited out in full.
        assert!(!waits(180.0, 179));
        assert!(waits(180.0, 180));
        // A wild report never holds a broken picture for more than half a second.
        assert!(waits(30_000.0, 500));
        // A report that is not a time changes nothing.
        assert!(!waits(f64::NAN, 99));
        assert!(waits(-5.0, 100));
    }

    #[test]
    fn the_encoders_own_keyframes_count_and_a_new_client_starts_over() {
        let mut gate = Gate::new();
        let t = Instant::now();
        // A keyframe nobody asked for (a resize, a new client) also makes a request moot.
        gate.sent(t);
        assert!(!gate.asked(t + 10 * MS));
        // A request answered but not yet encoded does not pile up more...
        assert!(gate.asked(t + 200 * MS));
        assert!(!gate.asked(t + 600 * MS));
        // ...but if its keyframe never comes (a dropped frame, a paused game), the client is
        // answered again after half a second.
        assert!(gate.asked(t + 700 * MS));
        assert!(!gate.asked(t + 701 * MS));
        gate.new_client();
        assert!(gate.asked(t + 702 * MS));
    }
}
