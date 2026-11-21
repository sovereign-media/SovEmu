//! `sovemu-worker probe`: load each core, report what it says about itself, unload it.
//!
//! The container build runs this over every core it ships, so a core that cannot be loaded
//! (a missing shared library, a build for another platform) fails the build instead of a session.
//! Cores are probed one after another, which the one-core-per-process rule allows.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Serialize;
use sovemu_protocol::ipc::{CoreCategory, CoreOption};
use sovemu_retro_host::{Core, CoreOptions};

use crate::ProbeArgs;

#[derive(Debug, Serialize)]
struct ProbeResult {
    path: PathBuf,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<String>,
    valid_extensions: Vec<String>,
    need_fullpath: bool,
    supports_no_game: bool,
    /// The core options it declares without a game, in its order, with their choices.
    variables: Vec<CoreOption>,
    /// The groups the core puts its options in ("Video", "Audio", …), if it does.
    categories: Vec<CoreCategory>,
}

/// Whether `path` looks like a shared library on this platform.
fn is_core_library(path: &Path) -> bool {
    path.extension().is_some_and(|e| e == std::env::consts::DLL_EXTENSION)
}

/// Expands directories into the core libraries directly inside them, sorted.
fn expand(paths: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut cores = Vec::new();
    for path in paths {
        if path.is_dir() {
            let mut found: Vec<PathBuf> = std::fs::read_dir(path)
                .with_context(|| format!("reading {}", path.display()))?
                .filter_map(|entry| entry.ok().map(|e| e.path()))
                .filter(|p| is_core_library(p))
                .collect();
            found.sort();
            cores.extend(found);
        } else {
            cores.push(path.clone());
        }
    }
    Ok(cores)
}

fn probe_one(path: &Path, scratch: &Path) -> ProbeResult {
    let mut result = ProbeResult {
        path: path.to_owned(),
        error: None,
        name: None,
        version: None,
        valid_extensions: Vec::new(),
        need_fullpath: false,
        supports_no_game: false,
        variables: Vec::new(),
        categories: Vec::new(),
    };
    let options = CoreOptions {
        system_dir: scratch.to_owned(),
        save_dir: scratch.to_owned(),
        variables: Vec::new(),
        // A stand-in game (see `standin`) makes a GPU core create its context, so allow one.
        gpu: Some(sovemu_retro_host::GpuChoice::Auto),
    };
    match Core::load(path, options) {
        Ok(mut core) => {
            let info = core.system_info();
            result.name = Some(info.library_name.clone());
            result.version = Some(info.library_version.clone());
            result.valid_extensions = info.valid_extensions.clone();
            result.need_fullpath = info.need_fullpath;
            result.supports_no_game = core.supports_no_game();
            (result.variables, result.categories) = crate::stream::declared_options(&core);
            // Cores that declare their options only with a game loaded: load a stand-in.
            if result.variables.is_empty()
                && let Some((ext, bytes)) = crate::standin::for_extensions(&result.valid_extensions)
            {
                let file = scratch.join(format!("standin.{ext}"));
                match std::fs::write(&file, bytes).map_err(|e| e.to_string()).and_then(|()| {
                    core.load_game(Some(&file)).map(|_| ()).map_err(|e| format!("{e:#}"))
                }) {
                    Ok(()) => {
                        (result.variables, result.categories) =
                            crate::stream::declared_options(&core);
                    }
                    Err(e) => {
                        tracing::warn!(path = %path.display(), error = %e, "stand-in game did not load")
                    }
                }
            }
        }
        Err(err) => result.error = Some(format!("{err:#}")),
    }
    result
}

pub(crate) fn run(args: ProbeArgs) -> Result<()> {
    // Cores may print to stdout; the JSON report must be the only thing on it.
    let mut out = sovemu_retro_host::take_stdout().context("keeping stdout for the report")?;
    let cores = expand(&args.cores)?;
    if cores.is_empty() {
        bail!("no cores found in {:?}", args.cores);
    }
    let scratch = std::env::temp_dir().join(format!("sovemu-probe-{}", std::process::id()));
    std::fs::create_dir_all(&scratch)?;

    let results: Vec<ProbeResult> = cores.iter().map(|core| probe_one(core, &scratch)).collect();
    std::fs::remove_dir_all(&scratch).ok();

    writeln!(out, "{}", serde_json::to_string_pretty(&results)?)?;
    let failed: Vec<_> = results.iter().filter(|r| r.error.is_some()).collect();
    for r in &failed {
        tracing::error!(
            path = %r.path.display(),
            error = r.error.as_deref().unwrap_or_default(),
            "core failed to load"
        );
    }
    if !failed.is_empty() {
        bail!("{} of {} cores failed to load", failed.len(), results.len());
    }
    Ok(())
}
