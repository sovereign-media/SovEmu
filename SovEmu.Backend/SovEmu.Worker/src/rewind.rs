//! Rewind: a ring of save states taken every few frames, capped by memory.
//!
//! While rewind is held the worker steps back one snapshot per frame, so rewinding runs at
//! [`INTERVAL`]× speed. How far back it reaches is capped twice (both set by the server): by
//! seconds of history and by memory. An NES state is a few KiB (the seconds cap applies), a SNES
//! state about 400 KiB (the memory cap applies first: ~5 s in 64 MiB).
//!
//! Rewind turns itself off where it cannot pay for itself: a PS2 state is tens of megabytes, so
//! taking one every other frame cost more than a whole frame period (the game ran at half speed)
//! and kept a fraction of a second. A snapshot must fit a share of the frame period
//! ([`BUDGET`]), and the history must be able to hold at least [`MIN_STATES`].

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// The share of a frame period one snapshot may take.
const BUDGET: f64 = 0.25;
/// Snapshots in a row over budget before rewind turns itself off (one slow one is a hiccup).
const SLOW_SNAPSHOTS: u32 = 3;
/// The fewest snapshots worth keeping: under a quarter of a second at 60 fps is no rewind.
const MIN_STATES: usize = 8;

/// Snapshot every this many frames.
pub(crate) const INTERVAL: u32 = 2;

#[derive(Debug)]
pub(crate) struct Rewind {
    states: VecDeque<Vec<u8>>,
    bytes: usize,
    max_bytes: usize,
    max_states: usize,
    counter: u32,
    /// Set if the core cannot serialize, or its states cost too much; stops trying every frame.
    unsupported: bool,
    /// How long one snapshot may take.
    budget: Duration,
    slow: u32,
}

impl Rewind {
    pub(crate) fn new(max_bytes: usize, max_states: usize, fps: f64) -> Self {
        Self {
            budget: Duration::from_secs_f64(BUDGET / fps.clamp(1.0, 1000.0)),
            slow: 0,
            states: VecDeque::new(),
            bytes: 0,
            max_bytes,
            max_states,
            counter: 0,
            unsupported: false,
        }
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.states.len()
    }

    /// Called once per emulated frame; snapshots every [`INTERVAL`] frames.
    pub(crate) fn record<E>(&mut self, serialize: impl FnOnce() -> Result<Vec<u8>, E>) {
        if self.unsupported || self.max_states == 0 || self.max_bytes == 0 {
            return;
        }
        self.counter += 1;
        if self.counter < INTERVAL {
            return;
        }
        self.counter = 0;
        self.snapshot(serialize);
    }

    /// Settles now, before play, whether this core's states are affordable: snapshots until one
    /// fits the budget (kept as the first of the history) or rewind turns itself off. A game
    /// continued from a save does this out of sight, where `record` would decide it over the
    /// first frames the player sees: on a PS2 that was three stalls of 50 ms or more.
    pub(crate) fn trial<E>(&mut self, mut serialize: impl FnMut() -> Result<Vec<u8>, E>) {
        for _ in 0..SLOW_SNAPSHOTS {
            if self.unsupported || self.max_states == 0 || self.max_bytes == 0 {
                return;
            }
            self.snapshot(&mut serialize);
            if self.slow == 0 {
                break;
            }
        }
        self.counter = 0;
    }

    /// Takes one snapshot, timing it: turns rewind off if this one, or the run it ends,
    /// costs too much.
    fn snapshot<E>(&mut self, serialize: impl FnOnce() -> Result<Vec<u8>, E>) {
        let started = Instant::now();
        let state = serialize();
        let took = started.elapsed();
        if let Ok(state) = &state {
            self.slow = if took > self.budget { self.slow + 1 } else { 0 };
            if self.slow >= SLOW_SNAPSHOTS || state.len() * MIN_STATES > self.max_bytes {
                tracing::info!(
                    state_mb = state.len() as f64 / (1024.0 * 1024.0),
                    snapshot_ms = took.as_secs_f64() * 1000.0,
                    budget_ms = self.budget.as_secs_f64() * 1000.0,
                    "rewind is off: this core's states are too large or too slow to take every other frame"
                );
                self.unsupported = true;
                self.clear();
                return;
            }
        }
        match state {
            Ok(state) if state.len() <= self.max_bytes => {
                self.bytes += state.len();
                self.states.push_back(state);
                while self.bytes > self.max_bytes || self.states.len() > self.max_states {
                    let Some(old) = self.states.pop_front() else { break };
                    self.bytes -= old.len();
                }
            }
            Ok(_) => self.unsupported = true,
            Err(_) => {
                tracing::info!("this core cannot save states; rewind is off");
                self.unsupported = true;
            }
        }
    }

    /// Restores the newest snapshot and drops it. Returns false when there is nothing left.
    pub(crate) fn step_back<E>(
        &mut self,
        unserialize: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> bool {
        let Some(state) = self.states.pop_back() else { return false };
        self.bytes -= state.len();
        self.counter = 0;
        unserialize(&state).is_ok()
    }

    /// Forgets everything (after loading a state or a reset, history no longer applies).
    pub(crate) fn clear(&mut self) {
        self.states.clear();
        self.bytes = 0;
        self.counter = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshots_every_interval_and_steps_back_newest_first() {
        let mut r = Rewind::new(1024, 100, 60.0);
        for frame in 0u8..6 {
            r.record(|| Ok::<_, ()>(vec![frame; 4]));
        }
        assert_eq!(r.len(), 3, "every {INTERVAL} frames");
        let mut seen = Vec::new();
        while r.step_back(|s| {
            seen.push(s[0]);
            Ok::<_, ()>(())
        }) {}
        assert_eq!(seen, [5, 3, 1]);
    }

    #[test]
    fn evicts_the_oldest_past_either_cap() {
        let mut r = Rewind::new(40, 100, 60.0);
        for frame in 0u8..20 {
            r.record(|| Ok::<_, ()>(vec![frame; 4]));
        }
        assert_eq!(r.len(), 10, "40 bytes hold ten 4-byte states");
        let mut r = Rewind::new(1 << 20, 3, 60.0);
        for frame in 0u8..20 {
            r.record(|| Ok::<_, ()>(vec![frame; 4]));
        }
        assert_eq!(r.len(), 3);
    }

    #[test]
    fn turns_itself_off_when_states_are_too_large_or_too_slow() {
        // A state an eighth of the memory cap would keep fewer than MIN_STATES: not worth taking.
        let mut r = Rewind::new(64, 100, 60.0);
        let mut calls = 0;
        for _ in 0..20 {
            r.record(|| {
                calls += 1;
                Ok::<_, ()>(vec![0; 16])
            });
        }
        assert_eq!((calls, r.len()), (1, 0));

        // Small but slow: three snapshots in a row over a quarter of the frame period.
        let mut r = Rewind::new(1 << 20, 100, 60.0);
        let mut calls = 0;
        for _ in 0..20 {
            r.record(|| {
                calls += 1;
                std::thread::sleep(Duration::from_millis(6));
                Ok::<_, ()>(vec![0; 4])
            });
        }
        assert_eq!((calls, r.len()), (3, 0));
    }

    #[test]
    fn a_trial_settles_a_slow_core_before_play() {
        let mut r = Rewind::new(1 << 20, 100, 60.0);
        let mut calls = 0;
        r.trial(|| {
            calls += 1;
            std::thread::sleep(Duration::from_millis(6));
            Ok::<_, ()>(vec![0; 4])
        });
        assert_eq!((calls, r.len()), (SLOW_SNAPSHOTS, 0), "off before the first frame");
        r.record(|| -> Result<Vec<u8>, ()> { panic!("rewind is off: no more snapshots") });
        r.record(|| -> Result<Vec<u8>, ()> { panic!("rewind is off: no more snapshots") });
    }

    #[test]
    fn a_trial_keeps_the_first_snapshot_that_fits() {
        let mut r = Rewind::new(1 << 20, 100, 60.0);
        let mut calls = 0;
        r.trial(|| {
            calls += 1;
            Ok::<_, ()>(vec![7; 4])
        });
        assert_eq!((calls, r.len()), (1, 1));
        // The history goes on from there at the usual interval.
        r.record(|| Ok::<_, ()>(vec![8; 4]));
        assert_eq!(r.len(), 1);
        r.record(|| Ok::<_, ()>(vec![9; 4]));
        assert_eq!(r.len(), 2);
    }

    #[test]
    fn gives_up_when_the_core_cannot_serialize() {
        let mut r = Rewind::new(1024, 100, 60.0);
        let mut calls = 0;
        for _ in 0..10 {
            r.record(|| {
                calls += 1;
                Err::<Vec<u8>, _>("no")
            });
        }
        assert_eq!(calls, 1);
        assert_eq!(r.len(), 0);
        assert!(!r.step_back(|_| Ok::<_, ()>(())));
    }
}
