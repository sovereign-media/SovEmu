//! Battery saves and save states on disk.
//!
//! - **Battery RAM** (`RETRO_MEMORY_SAVE_RAM`) is restored from `<save_dir>/<game>.srm` when the
//!   game loads, written back whenever it has changed (checked every [`FLUSH_INTERVAL`]), and on
//!   exit. Same name and format as RetroArch, so existing `.srm` files carry over.
//! - **Save states** live in the state directory as `slotN.state` with a `slotN.png` screenshot.
//!   Slot 0 is the auto state, written when the session ends so a player can continue later.
//!
//! Every write goes to a temporary file that is then renamed over the target, so a crash never
//! leaves a half-written save.

use std::path::{Path, PathBuf};
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

#[derive(Debug)]
pub(crate) struct Persist {
    srm: Option<PathBuf>,
    state_dir: Option<PathBuf>,
    /// The battery RAM as last written, to skip writes when nothing changed.
    written: Vec<u8>,
    next_check: Instant,
}

/// Writes `bytes` to `path` through a temporary file and a rename.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("replacing {}", path.display()))?;
    Ok(())
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
        }
    }

    /// Loads the `.srm` into the core's battery RAM, if both exist.
    pub(crate) fn restore(&mut self, core: &mut Core) {
        let (Some(path), Some(ram)) = (&self.srm, core.save_ram()) else { return };
        match std::fs::read(path) {
            Ok(bytes) => {
                if bytes.len() != ram.len() {
                    tracing::warn!(
                        "{} is {} bytes but the game has {}; loading what fits",
                        path.display(),
                        bytes.len(),
                        ram.len()
                    );
                }
                let n = bytes.len().min(ram.len());
                ram[..n].copy_from_slice(&bytes[..n]);
                tracing::info!("restored battery save {}", path.display());
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => tracing::warn!("cannot read {}: {e}", path.display()),
        }
        self.written = ram.to_vec();
    }

    /// Writes the battery RAM if it changed. Returns whether it wrote.
    pub(crate) fn flush(&mut self, core: &mut Core) -> Result<bool> {
        let (Some(path), Some(ram)) = (&self.srm, core.save_ram()) else { return Ok(false) };
        if ram == self.written.as_slice() {
            return Ok(false);
        }
        write_atomic(path, ram)?;
        self.written = ram.to_vec();
        tracing::debug!("wrote battery save {}", path.display());
        Ok(true)
    }

    /// Flushes the battery RAM if [`FLUSH_INTERVAL`] has passed since the last check.
    pub(crate) fn tick(&mut self, core: &mut Core, now: Instant) {
        if now < self.next_check {
            return;
        }
        self.next_check = now + FLUSH_INTERVAL;
        if let Err(e) = self.flush(core) {
            tracing::warn!("battery save failed: {e:#}");
        }
    }

    fn slot_paths(&self, slot: u32) -> Result<(PathBuf, PathBuf)> {
        if slot > MAX_SLOT {
            bail!("slot {slot} is out of range (0-{MAX_SLOT})");
        }
        let Some(dir) = &self.state_dir else { bail!("this session has no state directory") };
        Ok((dir.join(format!("slot{slot}.state")), dir.join(format!("slot{slot}.png"))))
    }

    pub(crate) fn save_state(
        &self,
        core: &mut Core,
        slot: u32,
        shot: Option<Screenshot<'_>>,
    ) -> Result<()> {
        let (state, png) = self.slot_paths(slot)?;
        let bytes = core.serialize().context("the core cannot save states")?;
        write_atomic(&state, &bytes)?;
        match shot.filter(|s| s.width > 0 && s.rgba.len() == (s.width * s.height * 4) as usize) {
            Some(shot) => write_atomic(&png, &encode_png(&shot)?)?,
            None => {
                let _ = std::fs::remove_file(&png);
            }
        }
        tracing::info!("saved state to slot {slot}");
        Ok(())
    }

    pub(crate) fn load_state(&self, core: &mut Core, slot: u32) -> Result<()> {
        let (state, _) = self.slot_paths(slot)?;
        let bytes = std::fs::read(&state).with_context(|| format!("slot {slot} is empty"))?;
        core.unserialize(&bytes).context("the core rejected the state")?;
        tracing::info!("loaded state from slot {slot}");
        Ok(())
    }

    /// End of session: battery RAM, then the auto state (if the game was played at all).
    pub(crate) fn finish(&mut self, core: &mut Core, played: bool, shot: Option<Screenshot<'_>>) {
        if let Err(e) = self.flush(core) {
            tracing::warn!("battery save failed: {e:#}");
        }
        if played
            && self.state_dir.is_some()
            && let Err(e) = self.save_state(core, AUTO_SLOT, shot)
        {
            tracing::warn!("auto state failed: {e:#}");
        }
    }
}
