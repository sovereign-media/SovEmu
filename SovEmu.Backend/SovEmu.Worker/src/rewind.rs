//! Rewind: a ring of save states taken every few frames, capped by memory.
//!
//! While rewind is held the worker steps back one snapshot per frame, so rewinding runs at
//! [`INTERVAL`]× speed. How far back it reaches is capped twice (both set by the server): by
//! seconds of history and by memory. An NES state is a few KiB (the seconds cap applies), a SNES
//! state about 400 KiB (the memory cap applies first: ~5 s in 64 MiB).

use std::collections::VecDeque;

/// Snapshot every this many frames.
pub const INTERVAL: u32 = 2;

#[derive(Debug)]
pub struct Rewind {
    states: VecDeque<Vec<u8>>,
    bytes: usize,
    max_bytes: usize,
    max_states: usize,
    counter: u32,
    /// Set if the core cannot serialize; stops trying every frame.
    unsupported: bool,
}

impl Rewind {
    pub fn new(max_bytes: usize, max_states: usize) -> Self {
        Self {
            states: VecDeque::new(),
            bytes: 0,
            max_bytes,
            max_states,
            counter: 0,
            unsupported: false,
        }
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.states.len()
    }

    /// Called once per emulated frame; snapshots every [`INTERVAL`] frames.
    pub fn record<E>(&mut self, serialize: impl FnOnce() -> Result<Vec<u8>, E>) {
        if self.unsupported || self.max_states == 0 || self.max_bytes == 0 {
            return;
        }
        self.counter += 1;
        if self.counter < INTERVAL {
            return;
        }
        self.counter = 0;
        match serialize() {
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
    pub fn step_back<E>(&mut self, unserialize: impl FnOnce(&[u8]) -> Result<(), E>) -> bool {
        let Some(state) = self.states.pop_back() else { return false };
        self.bytes -= state.len();
        self.counter = 0;
        unserialize(&state).is_ok()
    }

    /// Forgets everything (after loading a state or a reset, history no longer applies).
    pub fn clear(&mut self) {
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
        let mut r = Rewind::new(1024, 100);
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
        let mut r = Rewind::new(10, 100);
        for frame in 0u8..20 {
            r.record(|| Ok::<_, ()>(vec![frame; 4]));
        }
        assert_eq!(r.len(), 2, "10 bytes hold two 4-byte states");
        let mut r = Rewind::new(1 << 20, 3);
        for frame in 0u8..20 {
            r.record(|| Ok::<_, ()>(vec![frame; 4]));
        }
        assert_eq!(r.len(), 3);
    }

    #[test]
    fn gives_up_when_the_core_cannot_serialize() {
        let mut r = Rewind::new(1024, 100);
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
