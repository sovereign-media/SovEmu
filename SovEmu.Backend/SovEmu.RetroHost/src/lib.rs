//! A safe libretro frontend.
//!
//! [`Core`] loads a libretro core (a `.so`/`.dylib`/`.dll`), loads content into it and runs it
//! one frame at a time, handing back the frame's video, audio and timing.
//!
//! # One core per process
//!
//! libretro cores keep their state in global variables, and the callbacks a frontend gives them
//! are bare C function pointers with no user-data argument. So the callback state here is
//! process-global too, and only one [`Core`] may be loaded per process at a time:
//! [`Core::load`] returns [`Error::AlreadyLoaded`] otherwise. SovEmu runs one worker process
//! per game session for exactly this reason (and so a crashing core takes down only its own
//! session).

mod callbacks;
mod core;
mod error;
mod input;
mod video;

pub use crate::core::{AvInfo, Core, CoreOptions, CoreVariable, FrameOutput, SystemInfo};
pub use crate::error::Error;
pub use crate::input::{JoypadButtons, ParseButtonsError};
pub use crate::video::{PixelFormat, VideoFrame};

/// The highest controller port SovEmu exposes to a core (ports `0..MAX_PORTS`).
pub const MAX_PORTS: usize = 4;
