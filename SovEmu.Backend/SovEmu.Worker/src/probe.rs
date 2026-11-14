//! `sovemu-worker probe`: load each core, report what it says about itself, unload it.
//!
//! The container build runs this over every core it ships, so a core that cannot be loaded
//! (a missing shared library, a glibc-only build on musl) fails the build instead of a session.
//! Cores are probed one after another, which the one-core-per-process rule allows.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Serialize;
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
    /// The core options it declares, in its order, with their choices.
    variables: Vec<ProbeVariable>,
    /// The groups the core puts its options in ("Video", "Audio", …), if it does.
    categories: Vec<ProbeCategory>,
}

#[derive(Debug, Serialize)]
struct ProbeVariable {
    key: String,
    description: String,
    /// What the option does, where the core explains it.
    #[serde(skip_serializing_if = "String::is_empty")]
    info: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    category: Option<String>,
    choices: Vec<String>,
    /// A display label per choice.
    labels: Vec<String>,
    default: String,
}

#[derive(Debug, Serialize)]
struct ProbeCategory {
    key: String,
    description: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    info: String,
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
        // Loading is enough to probe; no game, so no context is ever made.
        gpu: Some(sovemu_retro_host::GpuChoice::Auto),
    };
    match Core::load(path, options) {
        Ok(core) => {
            let info = core.system_info();
            result.name = Some(info.library_name.clone());
            result.version = Some(info.library_version.clone());
            result.valid_extensions = info.valid_extensions.clone();
            result.need_fullpath = info.need_fullpath;
            result.supports_no_game = core.supports_no_game();
            result.variables = core
                .variables()
                .into_iter()
                .map(|v| ProbeVariable {
                    key: v.key,
                    description: v.description,
                    info: v.info,
                    category: v.category,
                    choices: v.options,
                    labels: v.labels,
                    default: v.default,
                })
                .collect();
            result.categories = core
                .option_categories()
                .into_iter()
                .map(|c| ProbeCategory { key: c.key, description: c.description, info: c.info })
                .collect();
        }
        Err(err) => result.error = Some(format!("{err:#}")),
    }
    result
}

pub fn run(args: ProbeArgs) -> Result<()> {
    let cores = expand(&args.cores)?;
    if cores.is_empty() {
        bail!("no cores found in {:?}", args.cores);
    }
    let scratch = std::env::temp_dir().join(format!("sovemu-probe-{}", std::process::id()));
    std::fs::create_dir_all(&scratch)?;

    let results: Vec<ProbeResult> = cores.iter().map(|core| probe_one(core, &scratch)).collect();
    std::fs::remove_dir_all(&scratch).ok();

    println!("{}", serde_json::to_string_pretty(&results)?);
    let failed: Vec<_> = results.iter().filter(|r| r.error.is_some()).collect();
    for r in &failed {
        tracing::error!("{}: {}", r.path.display(), r.error.as_deref().unwrap_or_default());
    }
    if !failed.is_empty() {
        bail!("{} of {} cores failed to load", failed.len(), results.len());
    }
    Ok(())
}
