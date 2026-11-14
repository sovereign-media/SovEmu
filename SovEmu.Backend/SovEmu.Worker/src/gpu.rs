//! `sovemu-worker gpu`: which GPUs this process can render with.
//!
//! The server runs this once at startup (cores and GL drivers only ever load in workers) and
//! keeps the answer. A device counts only if it drew a frame and read it back.

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
    /// What `auto` picks: a render node, `software`, or null if nothing works.
    best: Option<String>,
}

pub fn run() -> Result<()> {
    let report = match gl::probe() {
        Ok(found) => {
            let best =
                gl::best(&found).map(|r| r.node.clone().unwrap_or_else(|| "software".to_owned()));
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
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
