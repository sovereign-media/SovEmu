//! Run-ahead: hides a game's own input lag.
//!
//! Most games act on a button a frame or more after reading it, so even on real hardware the
//! picture answers late. An emulator can hide that: after each real frame, save the state, run
//! N more frames with the same input, show the last one's picture, and put the state back. The
//! picture then answers a press up to N frames sooner, while the game's real timeline (and its
//! sound, which comes from the real frame) is untouched. This is RetroArch's single-instance
//! run-ahead; its second-instance mode needs two cores in one process, which SovEmu never runs.
//!
//! Each frame costs N + 1 emulated frames plus a save and a load, so it is for the systems that
//! are cheap to emulate. If a tick keeps overrunning the frame period it runs fewer frames ahead.

use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use sovemu_retro_host::{Core, JoypadButtons, VideoFrame};

use sovemu_protocol::ipc::MAX_RUN_AHEAD as MAX_FRAMES;

/// Ticks in a row over budget before running one frame fewer ahead: long enough that one slow
/// frame (a level loading) does not count, short enough that a slow core stops stuttering soon.
const SLOW_TICKS: u32 = 30;

/// The share of the frame period run-ahead may take, leaving the rest for encoding and sending.
const BUDGET: f64 = 0.75;

pub(crate) struct RunAhead {
    frames: u32,
    budget: Duration,
    slow_ticks: u32,
}

impl RunAhead {
    pub(crate) fn new(frames: u32, fps: f64) -> Self {
        let frames = frames.min(MAX_FRAMES);
        let budget = Duration::from_secs_f64(BUDGET / fps.clamp(1.0, 1000.0));
        if frames > 0 {
            tracing::info!(frames, budget_ms = budget.as_secs_f64() * 1000.0, "run-ahead on");
        }
        Self { frames, budget, slow_ticks: 0 }
    }

    /// Frames it is running ahead now (it runs fewer if it cannot keep up).
    pub(crate) fn frames(&self) -> u32 {
        self.frames
    }

    /// From the real frame the core has just run, runs ahead with the same `input`, writes the
    /// last picture drawn into `picture` and returns its size, and puts the core back. `None` when
    /// it is off, or no frame ahead drew a picture (the real frame's then stands). Their sound
    /// is dropped: what is heard is the real timeline's.
    pub(crate) fn picture(
        &mut self,
        core: &mut Core,
        input: &[JoypadButtons],
        picture: &mut VideoFrame,
    ) -> Result<Option<(u32, u32)>> {
        if self.frames == 0 {
            return Ok(None);
        }
        let started = Instant::now();
        let state = match core.serialize() {
            Ok(state) => state,
            Err(e) => {
                tracing::warn!(error = %e, "run-ahead off: the core cannot save its state");
                self.frames = 0;
                return Ok(None);
            }
        };
        let mut shown = None;
        for _ in 0..self.frames {
            let out = core.run_frame(input)?;
            if out.video_updated && out.video.width > 0 {
                crate::video::keep_picture(out.video, picture);
                shown = Some((out.video.width, out.video.height));
            }
        }
        core.unserialize(&state).context("run-ahead: restoring the real frame")?;
        self.keep_up(started.elapsed());
        Ok(shown)
    }

    /// Runs a frame fewer ahead once ticks keep taking more than their share of the period.
    fn keep_up(&mut self, took: Duration) {
        if took <= self.budget {
            self.slow_ticks = 0;
            return;
        }
        self.slow_ticks += 1;
        if self.slow_ticks >= SLOW_TICKS {
            self.frames -= 1;
            self.slow_ticks = 0;
            tracing::warn!(
                frames = self.frames,
                took_ms = took.as_secs_f64() * 1000.0,
                budget_ms = self.budget.as_secs_f64() * 1000.0,
                "run-ahead cannot keep up; running fewer frames ahead"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_ahead(frames: u32) -> RunAhead {
        RunAhead { frames, budget: Duration::from_millis(10), slow_ticks: 0 }
    }

    #[test]
    fn a_tick_over_budget_now_and_then_is_forgiven() {
        let mut r = run_ahead(2);
        for _ in 0..SLOW_TICKS * 3 {
            r.keep_up(Duration::from_millis(15));
            r.keep_up(Duration::from_millis(5));
        }
        assert_eq!(r.frames, 2);
    }

    #[test]
    fn ticks_that_keep_overrunning_run_fewer_frames_ahead_down_to_none() {
        let mut r = run_ahead(2);
        for _ in 0..SLOW_TICKS {
            r.keep_up(Duration::from_millis(15));
        }
        assert_eq!(r.frames, 1);
        for _ in 0..SLOW_TICKS {
            r.keep_up(Duration::from_millis(15));
        }
        assert_eq!(r.frames, 0, "off once even one frame ahead is too slow");
    }

    #[test]
    fn asks_for_no_more_than_the_maximum() {
        assert_eq!(RunAhead::new(MAX_FRAMES + 3, 60.0).frames, MAX_FRAMES);
    }
}
