//! Keeping stdout for the frontend.
//!
//! Some cores print straight to stdout (PCSX-ReARMed's Lightrec logs its memory maps with
//! `printf`), and the worker's stdout is a protocol channel: JSON for `probe` and `gpu`, JSON
//! lines to the server for `session`. [`take_stdout`] keeps the real stdout for the frontend
//! and points file descriptor 1 at stderr, where the logs go, before any core is loaded.

use std::fs::File;
use std::io::{self, Write};
use std::os::fd::{AsFd, OwnedFd};

/// Returns the process's stdout as a file only the caller writes to, and redirects descriptor
/// 1 (and so `print!` and any core's `printf`) to stderr. Call once, before loading a core.
pub fn take_stdout() -> io::Result<File> {
    io::stdout().flush()?;
    let kept: OwnedFd = io::stdout().as_fd().try_clone_to_owned()?;
    // SAFETY: dup2 takes two descriptors this process has open (stderr and stdout) and only
    // replaces descriptor 1; no Rust object owns descriptor 1 exclusively (std's stdout
    // handle writes to it by number, and now reaches stderr).
    if unsafe { libc::dup2(libc::STDERR_FILENO, libc::STDOUT_FILENO) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(File::from(kept))
}
