//! Battery saves and save states on disk.
//!
//! - **Battery RAM** (`RETRO_MEMORY_SAVE_RAM`) is restored from `<save_dir>/<game>.srm` when the
//!   game loads, written back whenever it has changed (checked every [`FLUSH_INTERVAL`]), and on
//!   exit. Same name and format as RetroArch, so existing `.srm` files carry over.
//! - **Save states** live in the state directory as `slotN.state` with a `slotN.png` screenshot.
//!   Slot 0 is the auto state, written when the session ends so a player can continue later.
//!
//! Every write goes to a temporary file, flushed to the disk, that is then renamed over the
//! target, and the rename is flushed too: neither a crash nor a power cut leaves a half-written
//! save, or an empty one (a rename can reach the disk before the data it names).
//!
//! Writing, and a screenshot's PNG, happen on a thread of their own ([`Saver`]), in the order
//! asked: flushing to a slow disk takes tens of milliseconds, which on the stream's thread
//! would hold a frame back. What is taken from the core (its RAM, its state) is copied first,
//! on the stream's thread. A save state's reply is sent once it is on the disk; reading a state
//! back, and the end of the session, wait for every write asked before.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use sovemu_retro_host::Core;

pub(crate) const FLUSH_INTERVAL: Duration = Duration::from_secs(10);
/// The auto state's slot, written when a session ends.
pub(crate) const AUTO_SLOT: u32 = 0;
pub(crate) const MAX_SLOT: u32 = 9;

/// The last frame shown, for save-state screenshots.
#[derive(Debug, Default)]
pub(crate) struct Screenshot<'a> {
    pub rgba: &'a [u8],
    pub width: u32,
    pub height: u32,
}

/// Where a save state's outcome goes: a control request's reply.
pub(crate) type Reply = Sender<Result<(), String>>;

#[derive(Debug)]
pub(crate) struct Persist {
    srm: Option<PathBuf>,
    state_dir: Option<PathBuf>,
    /// The battery RAM as last written, to skip writes when nothing changed.
    written: Vec<u8>,
    next_check: Instant,
    saver: Saver,
}

/// Writes `bytes` to `path` through a temporary file and a rename, each flushed to the disk.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path.parent().filter(|d| !d.as_os_str().is_empty());
    if let Some(dir) = dir {
        std::fs::create_dir_all(dir)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    let written = std::fs::File::create(&tmp)
        .and_then(|mut file| {
            file.write_all(bytes)?;
            file.sync_all()
        })
        .with_context(|| format!("writing {}", tmp.display()));
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    std::fs::rename(&tmp, path).with_context(|| format!("replacing {}", path.display()))?;
    // The rename is the directory's: flushed, the new name survives a power cut.
    if let Some(dir) = dir
        && let Err(e) = std::fs::File::open(dir).and_then(|d| d.sync_all())
    {
        tracing::debug!(dir = %dir.display(), error = %e, "cannot flush the directory");
    }
    Ok(())
}

/// The thread saves are written on, in the order they are asked for.
#[derive(Debug)]
struct Saver {
    jobs: Option<Sender<Box<dyn FnOnce() + Send>>>,
    thread: Option<JoinHandle<()>>,
}

impl Saver {
    fn start() -> Self {
        let (jobs, queue) = mpsc::channel::<Box<dyn FnOnce() + Send>>();
        let span = tracing::Span::current();
        let thread = std::thread::Builder::new()
            .name("saves".into())
            .spawn(move || {
                let _span = span.enter();
                for job in queue {
                    job();
                }
            })
            .map_err(|e| tracing::warn!(error = %e, "no thread for saves: written as asked"))
            .ok();
        Self { jobs: thread.is_some().then_some(jobs), thread }
    }

    /// Runs `job` on the thread, after everything asked before it (here and now if there is
    /// no thread).
    fn run(&self, job: impl FnOnce() + Send + 'static) {
        match &self.jobs {
            Some(jobs) => {
                if let Err(mpsc::SendError(job)) = jobs.send(Box::new(job)) {
                    job();
                }
            }
            None => job(),
        }
    }

    /// Waits until everything asked so far is written.
    fn wait(&self) {
        let (done, written) = mpsc::channel();
        self.run(move || {
            let _ = done.send(());
        });
        let _ = written.recv();
    }
}

impl Drop for Saver {
    /// Writes everything still asked for before the session goes.
    fn drop(&mut self) {
        self.jobs = None;
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn encode_png(shot: &Screenshot<'_>) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, shot.width, shot.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(shot.rgba)?;
    Ok(out)
}

impl Persist {
    /// `save_name` is the game's file stem (`<save_dir>/<save_name>.srm`).
    pub(crate) fn new(
        save_dir: &Path,
        save_name: Option<&str>,
        state_dir: Option<PathBuf>,
    ) -> Self {
        Self {
            srm: save_name.map(|name| save_dir.join(format!("{name}.srm"))),
            state_dir,
            written: Vec::new(),
            next_check: Instant::now() + FLUSH_INTERVAL,
            saver: Saver::start(),
        }
    }

    /// Loads the `.srm` into the core's battery RAM, if both exist.
    pub(crate) fn restore(&mut self, core: &mut Core) {
        let (Some(path), Some(ram)) = (&self.srm, core.save_ram()) else { return };
        match std::fs::read(path) {
            Ok(bytes) => {
                if bytes.len() != ram.len() {
                    tracing::warn!(
                        path = %path.display(),
                        bytes = bytes.len(),
                        expected_bytes = ram.len(),
                        "battery save size differs from the game's; loading what fits"
                    );
                }
                let n = bytes.len().min(ram.len());
                ram[..n].copy_from_slice(&bytes[..n]);
                tracing::info!(path = %path.display(), "restored battery save");
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "cannot read battery save")
            }
        }
        self.written = ram.to_vec();
    }

    /// Writes the battery RAM if it changed (on the saver's thread). Returns whether it did.
    pub(crate) fn flush(&mut self, core: &mut Core) -> bool {
        let (Some(path), Some(ram)) = (&self.srm, core.save_ram()) else { return false };
        if ram == self.written.as_slice() {
            return false;
        }
        self.written.clear();
        self.written.extend_from_slice(ram);
        let (path, bytes) = (path.clone(), self.written.clone());
        self.saver.run(move || match write_atomic(&path, &bytes) {
            Ok(()) => tracing::debug!(path = %path.display(), "wrote battery save"),
            Err(e) => tracing::warn!(error = %format!("{e:#}"), "battery save failed"),
        });
        true
    }

    /// Flushes the battery RAM if [`FLUSH_INTERVAL`] has passed since the last check.
    pub(crate) fn tick(&mut self, core: &mut Core, now: Instant) {
        if now < self.next_check {
            return;
        }
        self.next_check = now + FLUSH_INTERVAL;
        self.flush(core);
    }

    fn slot_paths(&self, slot: u32) -> Result<(PathBuf, PathBuf)> {
        if slot > MAX_SLOT {
            bail!("slot {slot} is out of range (0-{MAX_SLOT})");
        }
        let Some(dir) = &self.state_dir else { bail!("this session has no state directory") };
        Ok((dir.join(format!("slot{slot}.state")), dir.join(format!("slot{slot}.png"))))
    }

    /// Takes a state of the game into `slot`, with `shot` as its screenshot, and writes them on
    /// the saver's thread. `reply` hears how it went once the files are on the disk (or at once,
    /// if the core could not save a state).
    pub(crate) fn save_state(
        &self,
        core: &mut Core,
        slot: u32,
        shot: Option<Screenshot<'_>>,
        reply: Option<Reply>,
    ) {
        let taken = self.slot_paths(slot).and_then(|paths| {
            Ok((paths, core.serialize().context("the core cannot save states")?))
        });
        let ((state, png), bytes) = match taken {
            Ok(taken) => taken,
            Err(e) => {
                tracing::warn!(slot, error = %format!("{e:#}"), "state not saved");
                if let Some(reply) = reply {
                    let _ = reply.send(Err(format!("{e:#}")));
                }
                return;
            }
        };
        let shot = shot
            .filter(|s| s.width > 0 && s.rgba.len() == (s.width * s.height * 4) as usize)
            .map(|s| (s.rgba.to_vec(), s.width, s.height));
        self.saver.run(move || {
            let written = write_atomic(&state, &bytes).and_then(|()| match shot {
                Some((rgba, width, height)) => {
                    let png_bytes = encode_png(&Screenshot { rgba: &rgba, width, height })?;
                    write_atomic(&png, &png_bytes)
                }
                None => {
                    let _ = std::fs::remove_file(&png);
                    Ok(())
                }
            });
            match &written {
                Ok(()) => tracing::info!(slot, "saved state"),
                Err(e) => tracing::warn!(slot, error = %format!("{e:#}"), "state not saved"),
            }
            if let Some(reply) = reply {
                let _ = reply.send(written.map_err(|e| format!("{e:#}")));
            }
        });
    }

    /// A slot's state, as saved (once any save asked for before is written).
    pub(crate) fn read_state(&self, slot: u32) -> Result<Vec<u8>> {
        let (state, _) = self.slot_paths(slot)?;
        self.saver.wait();
        std::fs::read(&state).with_context(|| format!("slot {slot} is empty"))
    }

    /// Puts a state read with [`Persist::read_state`] into the core.
    pub(crate) fn apply_state(core: &mut Core, slot: u32, bytes: &[u8]) -> Result<()> {
        core.unserialize(bytes).context("the core rejected the state")?;
        tracing::info!(slot, "loaded state");
        Ok(())
    }

    /// End of session: battery RAM, then the auto state (if the game was played at all), and
    /// back only once both are on the disk.
    pub(crate) fn finish(&mut self, core: &mut Core, played: bool, shot: Option<Screenshot<'_>>) {
        self.flush(core);
        if played && self.state_dir.is_some() {
            self.save_state(core, AUTO_SLOT, shot, None);
        }
        self.saver.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("sovemu-persist-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_write_replaces_the_file_and_leaves_no_temporary_behind() {
        let dir = dir("atomic");
        let path = dir.join("states/slot1.state");
        write_atomic(&path, b"first").unwrap();
        write_atomic(&path, b"second").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"second");
        let names: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(names, ["slot1.state"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn saves_are_written_in_order_and_waited_for() {
        let dir = dir("saver");
        let path = dir.join("game.srm");
        let saver = Saver::start();
        assert!(saver.thread.is_some(), "saves have a thread of their own");
        for n in 0..50u32 {
            let path = path.clone();
            saver.run(move || write_atomic(&path, &n.to_le_bytes()).unwrap());
        }
        saver.wait();
        assert_eq!(std::fs::read(&path).unwrap(), 49u32.to_le_bytes(), "the last asked wins");

        // Dropped, it writes what is still asked for before it goes.
        let (unblock, blocked) = mpsc::channel::<()>();
        saver.run(move || {
            let _ = blocked.recv();
        });
        let late = path.clone();
        saver.run(move || write_atomic(&late, b"late").unwrap());
        drop(unblock);
        drop(saver);
        assert_eq!(std::fs::read(&path).unwrap(), b"late");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
