//! Real-time frame pacing.
//!
//! Each frame has an absolute deadline, `start + n / fps`, so sleep overshoot on one frame is
//! paid back on the next instead of accumulating into drift. If the loop falls more than
//! [`MAX_LAG_FRAMES`] behind (a stall, a slow core), it resynchronises rather than running a
//! burst of frames to catch up, which a player would see as fast-forward.

use std::time::{Duration, Instant};

const MAX_LAG_FRAMES: u32 = 3;

/// Asks the kernel to wake this thread, and every thread it starts from now on, when its timers
/// say rather than up to 50 µs later (Linux's default timer slack), which it allows itself to
/// batch wake-ups. A frame's wait is a few milliseconds: the 50 µs came off every one of them.
/// Called first thing, before any thread starts.
pub(crate) fn precise_timers() {
    #[cfg(target_os = "linux")]
    if let Err(e) = rustix::thread::set_current_timer_slack(std::num::NonZeroU64::new(1)) {
        tracing::debug!(error = %e, "cannot tighten the timer slack");
    }
}

#[derive(Debug)]
pub(crate) struct Pacer {
    fps: f64,
    period: Duration,
    origin: Instant,
    frame: u32,
    /// Times the pacer gave up catching up and resynchronised.
    pub resyncs: u64,
}

impl Pacer {
    pub(crate) fn new(fps: f64, now: Instant) -> Self {
        Self { fps, period: period(fps), origin: now, frame: 0, resyncs: 0 }
    }

    /// The rate it paces at.
    pub(crate) fn fps(&self) -> f64 {
        self.fps
    }

    /// Paces at `fps` from the next frame on (the core changed its timing): the schedule goes
    /// on from the last deadline, at the new period, with no jump and no catching up.
    pub(crate) fn set_fps(&mut self, fps: f64) {
        self.origin += self.period * self.frame;
        self.frame = 0;
        self.fps = fps;
        self.period = period(fps);
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

fn period(fps: f64) -> Duration {
    Duration::from_secs_f64(1.0 / fps.clamp(1.0, 1000.0))
}

/// The stream's RTP clock (90 kHz), advanced a frame at a time at the core's current rate.
/// A core that changes its frame rate (as PS2 games do while they boot) changes how fast the
/// clock runs from then on, never where it is: a jump would make the client's jitter buffer hold
/// or drop frames to catch up, which a player sees as lag.
#[derive(Debug)]
pub(crate) struct MediaClock {
    fps: f64,
    /// The frame the current rate started at, and the clock's reading there.
    base_frame: u64,
    base_time: f64,
}

impl MediaClock {
    pub(crate) fn new(fps: f64) -> Self {
        Self { fps: fps.clamp(1.0, 1000.0), base_frame: 0, base_time: 0.0 }
    }

    /// Runs at `fps` after `frame` (which keeps the time the old rate gives it).
    pub(crate) fn set_fps(&mut self, frame: u64, fps: f64) {
        self.base_time = self.time(frame);
        self.base_frame = frame;
        self.fps = fps.clamp(1.0, 1000.0);
    }

    /// The 90 kHz timestamp of `frame`.
    pub(crate) fn at(&self, frame: u64) -> u64 {
        self.time(frame) as u64
    }

    fn time(&self, frame: u64) -> f64 {
        self.base_time + frame.saturating_sub(self.base_frame) as f64 * 90_000.0 / self.fps
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_frame_rate_takes_over_from_the_last_deadline() {
        let t0 = Instant::now();
        let mut pacer = Pacer::new(50.0, t0); // 20 ms frames
        pacer.next_deadline(t0);
        let second = pacer.next_deadline(t0); // t0 + 40 ms
        pacer.set_fps(100.0); // 10 ms frames from here
        assert_eq!(pacer.fps(), 100.0);
        assert_eq!(pacer.next_deadline(second), second + Duration::from_millis(10));
        assert_eq!(pacer.next_deadline(second), second + Duration::from_millis(20));
        assert_eq!(pacer.resyncs, 0);
    }

    #[test]
    fn the_media_clock_never_jumps_when_the_rate_changes() {
        let mut clock = MediaClock::new(60.0);
        assert_eq!(clock.at(300), 450_000); // 5 s
        // The game switches to 50 fps at frame 300: it stays at 5 s, then runs at 1,800 a frame.
        clock.set_fps(300, 50.0);
        assert_eq!(clock.at(300), 450_000);
        assert_eq!(clock.at(301), 451_800);
        assert_eq!(clock.at(350), 540_000); // one second of 50 fps later
        // Recomputing from frame 0 at the new rate would have jumped to 540,000 at once.
        assert_eq!(MediaClock::new(50.0).at(300), 540_000);
    }

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
