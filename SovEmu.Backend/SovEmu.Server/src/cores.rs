//! The admin's view of the cores: each installed (or shipped) core, its version and whether it
//! loads, the systems it plays, and its licence (`sovemu_library::cores`).

use std::path::{Path, PathBuf};

use sovemu_library::cores::{self, CoreLicence};
use sovemu_library::systems::{self, System};

use crate::AppState;

/// A system a core plays.
pub(crate) struct CoreSystem {
    pub(crate) system: &'static System,
    /// The core this system's games start with (SovEmu's choice, or `SOVEMU_SYSTEMS__<id>__CORE`).
    pub(crate) chosen: bool,
}

pub(crate) struct CoreReport {
    /// Its file name without `_libretro.<ext>`.
    pub(crate) name: String,
    /// Its file in the cores folder; none when it is shipped but missing here.
    pub(crate) file: Option<PathBuf>,
    /// What the core says it is, and its version (from the image's build-time probe, else
    /// probed now).
    pub(crate) label: Option<String>,
    pub(crate) version: Option<String>,
    /// It loads: the probe read it.
    pub(crate) loaded: bool,
    pub(crate) error: Option<String>,
    pub(crate) systems: Vec<CoreSystem>,
    /// None for a core the image doesn't ship (added to the cores folder by hand).
    pub(crate) licence: Option<&'static CoreLicence>,
}

/// `fceumm` from `…/fceumm_libretro.so`.
fn core_name(file: &Path) -> Option<String> {
    let name = file.file_name()?.to_str()?;
    let (stem, ext) = name.rsplit_once('.')?;
    matches!(ext, "so" | "dylib" | "dll").then_some(())?;
    stem.strip_suffix("_libretro").map(str::to_owned)
}

/// The core libraries in the cores folder, by name.
fn installed(dir: &Path) -> Vec<(String, PathBuf)> {
    let mut found: Vec<(String, PathBuf)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| Some((core_name(&e.path())?, e.path())))
        .collect();
    found.sort();
    found
}

/// Every shipped core (in `scripts/cores.list`'s order), then any other found in the cores
/// folder.
pub(crate) async fn report(state: &AppState) -> Vec<CoreReport> {
    let mut files = installed(&state.config.cores_dir);
    let mut names: Vec<String> = cores::CORES.iter().map(|c| c.name.to_owned()).collect();
    names.extend(files.iter().map(|(n, _)| n.clone()).filter(|n| cores::licence(n).is_none()));
    let worker = state.workers.worker_bin();
    let mut out = Vec::new();
    for name in names {
        let file = files.iter().position(|(n, _)| *n == name).map(|i| files.remove(i).1);
        let (label, version, loaded, error) = match &file {
            None => (None, None, false, None),
            Some(path) => match state.core_info.get(&worker, path).await {
                Ok(info) => (info.name.clone(), info.version.clone(), true, None),
                Err(e) => (None, None, false, Some(e.message)),
            },
        };
        let systems = systems::SYSTEMS
            .iter()
            .filter(|s| {
                let configured = state.config.systems.get(s.id).and_then(|c| c.core.as_deref());
                s.cores.contains(&name.as_str()) || configured == Some(name.as_str())
            })
            .map(|system| CoreSystem {
                system,
                chosen: crate::api::find_core(state, system)
                    .is_some_and(|p| core_name(&p).as_deref() == Some(name.as_str())),
            })
            .collect();
        out.push(CoreReport {
            licence: cores::licence(&name),
            name,
            file,
            label,
            version,
            loaded,
            error,
            systems,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_names_come_from_library_file_names() {
        assert_eq!(core_name(Path::new("/c/fceumm_libretro.so")).as_deref(), Some("fceumm"));
        assert_eq!(core_name(Path::new("snes9x_libretro.dylib")).as_deref(), Some("snes9x"));
        assert_eq!(core_name(Path::new("/c/manifest.json")), None);
        assert_eq!(core_name(Path::new("/c/readme_libretro.txt")), None);
    }
}
