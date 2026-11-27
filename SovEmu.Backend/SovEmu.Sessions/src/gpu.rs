//! Which GPU the 3D systems render on, decided once at startup.
//!
//! As Sovereign does for its codecs, a device's presence is not proof that it works: the server
//! runs `sovemu-worker gpu`, which makes a GL context on every device EGL lists, draws a frame
//! and reads it back, and only devices that pass are used. (The server never loads GL or a
//! core itself; the probe runs in a worker like everything else that does.) `SovEmu__Rendering__Gpu`
//! chooses, and anything that fails falls back to Mesa's software renderer, then to having no
//! 3D systems at all.

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// One device the worker tried.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Device {
    /// The render node (`/dev/dri/renderD128`); none for the software renderer.
    pub node: Option<String>,
    pub driver: Option<String>,
    /// `hardware`, `virtual` (a hypervisor's adapter) or `software`.
    pub kind: String,
    pub vendor: Option<String>,
    pub renderer: Option<String>,
    pub version: Option<String>,
    pub works: bool,
    pub error: Option<String>,
}

/// What `sovemu-worker gpu` prints.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Probe {
    pub error: Option<String>,
    #[serde(default)]
    pub devices: Vec<Device>,
    /// What the worker would pick: a render node, `software` or none.
    pub best: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// 3D systems render on a GPU.
    Gpu,
    /// 3D systems render on the CPU (llvmpipe): they work, slowly.
    Software,
    /// No 3D systems.
    Off,
}

/// The decision, kept for the life of the process and shown to admins.
#[derive(Debug, Clone, Serialize)]
pub struct GpuStatus {
    /// `SovEmu__Rendering__Gpu` as configured.
    pub setting: String,
    pub mode: Mode,
    /// What workers get as `--gpu`.
    #[serde(skip)]
    pub worker_arg: String,
    /// The renderer 3D games draw with (`AMD Radeon RX 6600 (radeonsi …)`, `llvmpipe …`).
    pub renderer: Option<String>,
    pub devices: Vec<Device>,
    /// Why the setting could not be honoured, or why nothing could be probed.
    pub note: Option<String>,
    /// When the probe ran (once, at startup); none for a decision made without one.
    #[serde(skip)]
    pub probed_at: Option<std::time::SystemTime>,
}

impl GpuStatus {
    pub fn off(setting: &str, note: Option<String>) -> Self {
        Self {
            setting: setting.to_owned(),
            mode: Mode::Off,
            worker_arg: "off".into(),
            renderer: None,
            devices: Vec::new(),
            note,
            probed_at: None,
        }
    }

    /// Whether workers render on `device`: the one `worker_arg` names (`auto`, a GPU without a
    /// render node, is the first working hardware device, as workers pick it).
    pub fn chosen(&self, device: &Device) -> bool {
        if !device.works {
            return false;
        }
        match self.mode {
            Mode::Off => false,
            Mode::Software => device.kind == "software",
            Mode::Gpu if self.worker_arg == "auto" => self
                .devices
                .iter()
                .find(|d| d.works && d.kind == "hardware")
                .is_some_and(|d| d == device),
            Mode::Gpu => device.node.as_deref() == Some(self.worker_arg.as_str()),
        }
    }

    /// Whether workers render on a hardware GPU, integrated or discrete: not on the software
    /// renderer, and not on a hypervisor's adapter (`virtual`), which passes the work to its
    /// host slowly. The 3D systems' enhancements (`System::gpu_options`) apply only then.
    pub fn renders_on_hardware(&self) -> bool {
        self.mode == Mode::Gpu
            && self.devices.iter().any(|d| self.chosen(d) && d.kind == "hardware")
    }

    /// One line per device and a verdict, for the startup log.
    pub fn summary(&self) -> String {
        let mut lines = Vec::new();
        for d in &self.devices {
            let name = d.node.as_deref().unwrap_or("software renderer");
            let what = [d.driver.as_deref(), Some(d.kind.as_str())]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(", ");
            let result = match (&d.renderer, &d.error) {
                (Some(r), _) if d.works => format!("works: {r}"),
                (_, Some(e)) => format!("failed: {e}"),
                _ => "failed".into(),
            };
            lines.push(format!("  {name} ({what}): {result}"));
        }
        let verdict = match self.mode {
            Mode::Gpu => format!(
                "3D systems render on the GPU: {}",
                self.renderer.as_deref().unwrap_or(&self.worker_arg)
            ),
            Mode::Software => "3D systems render in software (llvmpipe): playable but slow; \
                               pass a GPU through for full speed"
                .to_owned(),
            Mode::Off => "3D systems are unavailable".to_owned(),
        };
        lines.push(verdict);
        if let Some(note) = &self.note {
            lines.push(format!("  ({note})"));
        }
        lines.join("\n")
    }
}

fn renderer_of(devices: &[Device], node: Option<&str>) -> Option<String> {
    devices.iter().find(|d| d.works && d.node.as_deref() == node).and_then(|d| d.renderer.clone())
}

/// Turns `SovEmu__Rendering__Gpu` and a probe into a decision.
pub(crate) fn decide(setting: &str, probe: Probe) -> GpuStatus {
    let setting = setting.trim();
    if let Some(error) = probe.error {
        return GpuStatus::off(setting, Some(error));
    }
    let software = probe.devices.iter().find(|d| d.works && d.kind == "software");
    let with = |mode, arg: String, renderer, note| GpuStatus {
        setting: setting.to_owned(),
        mode,
        worker_arg: arg,
        renderer,
        devices: probe.devices.clone(),
        note,
        probed_at: None,
    };
    let fall_back = |note: String| match software {
        Some(d) => with(Mode::Software, "software".into(), d.renderer.clone(), Some(note)),
        None => GpuStatus { devices: probe.devices.clone(), ..GpuStatus::off(setting, Some(note)) },
    };
    match setting {
        "software" => match software {
            Some(d) => with(Mode::Software, "software".into(), d.renderer.clone(), None),
            None => fall_back("no working software renderer".into()),
        },
        "" | "auto" => match probe.best.as_deref() {
            Some("software") => fall_back("no working GPU".into()),
            // A working GPU with no render node (NVIDIA without nvidia-drm): workers pick it
            // with `auto` too, hardware before software.
            Some("auto") => {
                let gpu = probe.devices.iter().find(|d| d.works && d.kind == "hardware");
                with(Mode::Gpu, "auto".into(), gpu.and_then(|d| d.renderer.clone()), None)
            }
            Some(node) => {
                with(Mode::Gpu, node.to_owned(), renderer_of(&probe.devices, Some(node)), None)
            }
            None => fall_back("no working GPU or software renderer".into()),
        },
        node => match probe.devices.iter().find(|d| d.node.as_deref() == Some(node)) {
            Some(d) if d.works && d.kind != "software" => {
                with(Mode::Gpu, node.to_owned(), d.renderer.clone(), None)
            }
            Some(d) => fall_back(format!(
                "{node} failed: {}",
                d.error.as_deref().unwrap_or("it renders in software")
            )),
            None => fall_back(format!("{node} is not visible to the container")),
        },
    }
}

/// Runs the probe in a worker (with the workers' cleared environment) and decides.
pub async fn detect(worker: &Path, setting: &str, env: &[(String, String)]) -> GpuStatus {
    let mut status = probe_and_decide(worker, setting, env).await;
    if setting.trim() != "off" {
        status.probed_at = Some(std::time::SystemTime::now());
    }
    status
}

async fn probe_and_decide(worker: &Path, setting: &str, env: &[(String, String)]) -> GpuStatus {
    if setting.trim() == "off" {
        return GpuStatus::off(setting, Some("SovEmu__Rendering__Gpu=off".into()));
    }
    let mut command = tokio::process::Command::new(worker);
    command
        .arg("gpu")
        .env_clear()
        .envs(env.iter().cloned())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let output = match tokio::time::timeout(Duration::from_secs(60), command.output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(e)) => return GpuStatus::off(setting, Some(format!("running the GPU probe: {e}"))),
        Err(_) => return GpuStatus::off(setting, Some("the GPU probe timed out".into())),
    };
    match serde_json::from_slice::<Probe>(&output.stdout) {
        Ok(probe) => decide(setting, probe),
        Err(e) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let detail = stderr.lines().last().unwrap_or_default().to_owned();
            GpuStatus::off(setting, Some(format!("the GPU probe failed ({e}): {detail}")))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(node: Option<&str>, kind: &str, works: bool) -> Device {
        Device {
            node: node.map(str::to_owned),
            driver: None,
            kind: kind.into(),
            vendor: None,
            renderer: Some(format!("{kind} renderer")),
            version: None,
            works,
            error: (!works).then(|| "boom".into()),
        }
    }

    fn probe(devices: Vec<Device>, best: Option<&str>) -> Probe {
        Probe { error: None, devices, best: best.map(str::to_owned) }
    }

    #[test]
    fn auto_takes_the_gpu_then_software_then_nothing() {
        let gpu = device(Some("/dev/dri/renderD128"), "hardware", true);
        let soft = device(None, "software", true);
        let s = decide("auto", probe(vec![gpu.clone(), soft.clone()], Some("/dev/dri/renderD128")));
        assert_eq!((s.mode, s.worker_arg.as_str()), (Mode::Gpu, "/dev/dri/renderD128"));
        assert_eq!(s.renderer.as_deref(), Some("hardware renderer"));

        let s = decide("auto", probe(vec![soft.clone()], Some("software")));
        assert_eq!((s.mode, s.worker_arg.as_str()), (Mode::Software, "software"));

        let s = decide("auto", probe(vec![], None));
        assert_eq!((s.mode, s.worker_arg.as_str()), (Mode::Off, "off"));
    }

    #[test]
    fn the_chosen_device_is_the_one_workers_use() {
        let gpu = device(Some("/dev/dri/renderD128"), "hardware", true);
        let other = device(Some("/dev/dri/renderD129"), "hardware", true);
        let soft = device(None, "software", true);
        let all = vec![gpu.clone(), other.clone(), soft.clone()];
        let s = decide("/dev/dri/renderD129", probe(all.clone(), Some("/dev/dri/renderD128")));
        assert_eq!(s.devices.iter().map(|d| s.chosen(d)).collect::<Vec<_>>(), [false, true, false]);
        let s = decide("software", probe(all.clone(), Some("/dev/dri/renderD128")));
        assert_eq!(s.devices.iter().map(|d| s.chosen(d)).collect::<Vec<_>>(), [false, false, true]);
        let nodeless = device(None, "hardware", true);
        let s = decide("auto", probe(vec![soft.clone(), nodeless], Some("auto")));
        assert_eq!(s.devices.iter().map(|d| s.chosen(d)).collect::<Vec<_>>(), [false, true]);
        let s = GpuStatus::off("off", None);
        assert!(!s.chosen(&gpu));
    }

    #[test]
    fn only_a_hardware_gpu_counts_as_rendering_on_hardware() {
        // An integrated GPU is a hardware device like any other (i915 on Intel).
        let intel = device(Some("/dev/dri/renderD128"), "hardware", true);
        let vm = device(Some("/dev/dri/renderD129"), "virtual", true);
        let soft = device(None, "software", true);
        let all = vec![intel, vm, soft.clone()];
        let on = |setting, best| decide(setting, probe(all.clone(), best)).renders_on_hardware();
        assert!(on("auto", Some("/dev/dri/renderD128")));
        assert!(on("/dev/dri/renderD128", Some("/dev/dri/renderD128")));
        assert!(!on("/dev/dri/renderD129", Some("/dev/dri/renderD128")), "a hypervisor's");
        assert!(!on("software", Some("/dev/dri/renderD128")));
        let nodeless = device(None, "hardware", true);
        let s = decide("auto", probe(vec![soft.clone(), nodeless], Some("auto")));
        assert!(s.renders_on_hardware(), "NVIDIA without nvidia-drm");
        let s = decide("auto", probe(vec![soft], Some("software")));
        assert!(!s.renders_on_hardware());
        assert!(!GpuStatus::off("off", None).renders_on_hardware());
    }

    #[test]
    fn a_gpu_without_a_render_node_is_still_used() {
        // NVIDIA's driver without nvidia-drm: an EGL device with no DRM node.
        let nvidia = device(None, "hardware", true);
        let soft = device(None, "software", true);
        let s = decide("auto", probe(vec![soft, nvidia], Some("auto")));
        assert_eq!((s.mode, s.worker_arg.as_str()), (Mode::Gpu, "auto"));
        assert_eq!(s.renderer.as_deref(), Some("hardware renderer"));
    }

    #[test]
    fn a_named_device_that_fails_falls_back_to_software() {
        let broken = device(Some("/dev/dri/renderD129"), "hardware", false);
        let soft = device(None, "software", true);
        let s = decide("/dev/dri/renderD129", probe(vec![broken, soft.clone()], Some("software")));
        assert_eq!(s.mode, Mode::Software);
        assert!(s.note.unwrap().contains("renderD129 failed: boom"));
        let s = decide("/dev/dri/renderD200", probe(vec![soft], Some("software")));
        assert!(s.note.unwrap().contains("not visible"));
    }

    #[test]
    fn software_and_off_and_no_egl() {
        let gpu = device(Some("/dev/dri/renderD128"), "hardware", true);
        let soft = device(None, "software", true);
        let s = decide("software", probe(vec![gpu.clone(), soft], Some("/dev/dri/renderD128")));
        assert_eq!(s.mode, Mode::Software);
        let s = decide("software", probe(vec![gpu], Some("/dev/dri/renderD128")));
        assert_eq!(s.mode, Mode::Off, "no software renderer to use");
        let s = decide("auto", Probe { error: Some("no libEGL".into()), ..Probe::default() });
        assert_eq!((s.mode, s.note.as_deref()), (Mode::Off, Some("no libEGL")));
        assert!(s.summary().contains("unavailable"));
    }
}
