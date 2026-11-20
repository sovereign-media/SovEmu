//! Real-time frame pacing.
//!
//! Each frame has an absolute deadline, `start + n / fps`, so sleep overshoot on one frame is
//! paid back on the next instead of accumulating into drift. If the loop falls more than
//! [`MAX_LAG_FRAMES`] behind (a stall, a slow core), it resynchronises rather than running a
//! burst of frames to catch up, which a player would see as fast-forward.

use std::time::{Duration, Instant};

const MAX_LAG_FRAMES: u32 = 3;

#[derive(Debug)]
pub(crate) struct Pacer {
    period: Duration,
    origin: Instant,
    frame: u32,
    /// Times the pacer gave up catching up and resynchronised.
    pub resyncs: u64,
}

impl Pacer {
    pub(crate) fn new(fps: f64, now: Instant) -> Self {
        let period = Duration::from_secs_f64(1.0 / fps.clamp(1.0, 1000.0));
        Self { period, origin: now, frame: 0, resyncs: 0 }
    }

    /// Advances one frame and returns when that frame is due.
    /// Resynchronises first if `now` is too far past the schedule.
    pub(crate) fn next_deadline(&mut self, now: Instant) -> Instant {
        self.frame += 1;
        let deadline = self.origin + self.period * self.frame;
        if now > deadline + self.period * MAX_LAG_FRAMES {
            self.origin = now;
            self.frame = 0;
            self.resyncs += 1;
            return now;
        }
        deadline
    }

    /// Sleeps until the next frame is due.
    pub(crate) fn wait(&mut self) {
        let deadline = self.next_deadline(Instant::now());
        if let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
            std::thread::sleep(remaining);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deadlines_are_absolute() {
        let t0 = Instant::now();
        let mut pacer = Pacer::new(50.0, t0); // 20 ms frames
        assert_eq!(pacer.next_deadline(t0), t0 + Duration::from_millis(20));
        // Running a little late does not push later deadlines back.
        let late = t0 + Duration::from_millis(25);
        assert_eq!(pacer.next_deadline(late), t0 + Duration::from_millis(40));
        assert_eq!(pacer.resyncs, 0);
    }

    #[test]
    fn resyncs_after_a_long_stall() {
        let t0 = Instant::now();
        let mut pacer = Pacer::new(50.0, t0);
        let stalled = t0 + Duration::from_secs(1);
        assert_eq!(pacer.next_deadline(stalled), stalled);
        assert_eq!(pacer.resyncs, 1);
        assert_eq!(pacer.next_deadline(stalled), stalled + Duration::from_millis(20));
    }
}
