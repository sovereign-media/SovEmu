//! `sovemu-worker gpu`: which GPUs this process can render with.
//!
//! The server runs this once at startup (cores and GL drivers only ever load in workers) and
//! keeps the answer. A device counts only if it drew a frame and read it back.

use std::io::Write;

use anyhow::Result;
use serde::Serialize;
use sovemu_retro_host::gl;

#[derive(Debug, Serialize)]
struct Device {
    /// The render node, or null for the software renderer.
    node: Option<String>,
    driver: Option<String>,
    /// `hardware`, `virtual` or `software`.
    kind: &'static str,
    vendor: Option<String>,
    renderer: Option<String>,
    version: Option<String>,
    works: bool,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct Report {
    /// Why nothing could be probed (no libEGL).
    error: Option<String>,
    devices: Vec<Device>,
    /// What `auto` picks: a render node, `software`, `auto` (a GPU with no render node), or
    /// null if nothing works.
    best: Option<String>,
}

pub fn run() -> Result<()> {
    // GL drivers may print to stdout; the JSON report must be the only thing on it.
    let mut out = sovemu_retro_host::take_stdout()?;
    let report = match gl::probe() {
        Ok(found) => {
            // A GPU with no DRM node (NVIDIA's driver without nvidia-drm) is still a GPU:
            // workers find it again with `auto`, which tries hardware before software.
            let best = gl::best(&found).map(|r| match (&r.node, r.kind) {
                (Some(node), _) => node.clone(),
                (None, "software") => "software".to_owned(),
                (None, _) => "auto".to_owned(),
            });
            let devices = found
                .into_iter()
                .map(|r| Device {
                    vendor: r.renderer.as_ref().map(|i| i.vendor.clone()),
                    version: r.renderer.as_ref().map(|i| i.version.clone()),
                    renderer: r.renderer.map(|i| i.renderer),
                    node: r.node,
                    driver: r.driver,
                    kind: r.kind,
                    works: r.works,
                    error: r.error,
                })
                .collect();
            Report { error: None, devices, best }
        }
        Err(e) => Report { error: Some(e), devices: Vec::new(), best: None },
    };
    writeln!(out, "{}", serde_json::to_string_pretty(&report)?)?;
    Ok(())
}
