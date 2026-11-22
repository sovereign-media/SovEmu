//! What the machine is: its CPU, how many cores, the instruction sets cores care about, its
//! memory and kernel. Read once at startup; none of it changes while the server runs.
//!
//! On Linux from `/proc` and `/sys` (a container sees the host's CPU and kernel); on macOS, for
//! development, from `sysctl`. Anything that cannot be read is left out, never guessed.

/// The instruction sets worth showing: some cores need them (PCSX2 needs SSE4.1, and runs
/// better with AVX2), and they tell an old CPU from a new one at a glance.
const FEATURES: &[&str] = &["sse4_1", "sse4_2", "avx", "avx2", "avx512f"];

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct HostInfo {
    /// `AMD Ryzen 7 5800U with Radeon Graphics`.
    pub(crate) cpu_model: Option<String>,
    /// `x86_64`, `aarch64`.
    pub(crate) arch: &'static str,
    /// Logical CPUs on the host.
    pub(crate) host_cpus: Option<usize>,
    /// Logical CPUs this process may run on (its affinity mask; a quota is the cgroup's).
    pub(crate) usable_cpus: usize,
    /// The CPU's highest clock, MHz.
    pub(crate) max_mhz: Option<u32>,
    /// Which of [`FEATURES`] the CPU has (x86 only).
    pub(crate) features: Vec<&'static str>,
    /// The host's memory, bytes.
    pub(crate) memory_bytes: Option<u64>,
    /// The kernel's release (`6.12.48+deb13-amd64`; `24.1.0` for Darwin).
    pub(crate) kernel: Option<String>,
}

/// The facts from the real files (or `sysctl`).
pub(crate) fn read_system() -> HostInfo {
    let usable = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
    if cfg!(target_os = "macos") {
        return from_sysctl(
            &|name| {
                let out = std::process::Command::new("sysctl").args(["-n", name]).output().ok()?;
                out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
            },
            usable,
        );
    }
    from_files(&|path| std::fs::read_to_string(path).ok(), usable)
}

/// Linux: `/proc/cpuinfo`, `/proc/meminfo`, the kernel's release, cpufreq.
pub(crate) fn from_files(read: &dyn Fn(&str) -> Option<String>, usable: usize) -> HostInfo {
    let cpuinfo = read("/proc/cpuinfo").unwrap_or_default();
    let field = |name: &str| {
        cpuinfo.lines().find_map(|l| {
            let (k, v) = l.split_once(':')?;
            (k.trim() == name).then(|| v.trim().to_owned())
        })
    };
    let host_cpus =
        cpuinfo.lines().filter(|l| l.split(':').next().map(str::trim) == Some("processor")).count();
    let flags = field("flags").unwrap_or_default();
    let flags: Vec<&str> = flags.split_whitespace().collect();
    // kHz in cpufreq; "cpu MHz" in cpuinfo is the current clock, not the highest, so it is
    // not used.
    let max_mhz = read("/sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_max_freq")
        .and_then(|k| k.trim().parse::<u32>().ok())
        .map(|khz| khz / 1000);
    HostInfo {
        cpu_model: field("model name").filter(|m| !m.is_empty()),
        arch: std::env::consts::ARCH,
        host_cpus: (host_cpus > 0).then_some(host_cpus),
        usable_cpus: usable,
        max_mhz,
        features: FEATURES.iter().copied().filter(|f| flags.contains(f)).collect(),
        memory_bytes: super::cgroup::host_memory(read),
        kernel: read("/proc/sys/kernel/osrelease")
            .map(|k| k.trim().to_owned())
            .filter(|k| !k.is_empty()),
    }
}

/// macOS: `sysctl` names, for development.
pub(crate) fn from_sysctl(sysctl: &dyn Fn(&str) -> Option<String>, usable: usize) -> HostInfo {
    let number = |name: &str| sysctl(name).and_then(|v| v.parse::<u64>().ok());
    let features = sysctl("machdep.cpu.features")
        .map(|f| {
            let f = f.to_lowercase().replace('.', "_");
            let leaf7 = sysctl("machdep.cpu.leaf7_features").unwrap_or_default().to_lowercase();
            let all: Vec<&str> = f.split_whitespace().chain(leaf7.split_whitespace()).collect();
            fn mac(name: &str) -> &str {
                if name == "avx" { "avx1_0" } else { name }
            }
            FEATURES.iter().copied().filter(|n| all.contains(&mac(n))).collect()
        })
        .unwrap_or_default();
    HostInfo {
        cpu_model: sysctl("machdep.cpu.brand_string").filter(|m| !m.is_empty()),
        arch: std::env::consts::ARCH,
        host_cpus: number("hw.logicalcpu").map(|n| n as usize),
        usable_cpus: usable,
        // Intel Macs only; Apple's own chips do not say.
        max_mhz: number("hw.cpufrequency_max").map(|hz| (hz / 1_000_000) as u32),
        features,
        memory_bytes: number("hw.memsize"),
        kernel: sysctl("kern.osrelease"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_facts_from_proc_and_sys() {
        let read = |path: &str| {
            Some(
                match path {
                    "/proc/cpuinfo" => {
                        "processor\t: 0\nmodel name\t: AMD Ryzen 7 5800U with Radeon Graphics\n\
                         flags\t\t: fpu sse4_1 sse4_2 avx avx2 bmi2\n\nprocessor\t: 1\n\
                         model name\t: AMD Ryzen 7 5800U with Radeon Graphics\n"
                    }
                    "/proc/meminfo" => "MemTotal:       32768000 kB\n",
                    "/proc/sys/kernel/osrelease" => "6.12.48+deb13-amd64\n",
                    "/sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_max_freq" => "4507000\n",
                    _ => return None,
                }
                .to_owned(),
            )
        };
        let h = from_files(&read, 2);
        assert_eq!(h.cpu_model.as_deref(), Some("AMD Ryzen 7 5800U with Radeon Graphics"));
        assert_eq!((h.host_cpus, h.usable_cpus, h.max_mhz), (Some(2), 2, Some(4507)));
        assert_eq!(h.features, ["sse4_1", "sse4_2", "avx", "avx2"]);
        assert_eq!(h.memory_bytes, Some(32_768_000 * 1024));
        assert_eq!(h.kernel.as_deref(), Some("6.12.48+deb13-amd64"));
    }

    #[test]
    fn what_cannot_be_read_is_left_out() {
        let h = from_files(&|_| None, 4);
        assert_eq!(
            h,
            HostInfo { arch: std::env::consts::ARCH, usable_cpus: 4, ..HostInfo::default() }
        );
    }

    #[test]
    fn mac_facts_from_sysctl() {
        let sysctl = |name: &str| {
            Some(
                match name {
                    "machdep.cpu.brand_string" => "Intel(R) Core(TM) i9-9880H CPU @ 2.30GHz",
                    "hw.logicalcpu" => "16",
                    "hw.memsize" => "17179869184",
                    "hw.cpufrequency_max" => "2300000000",
                    "kern.osrelease" => "24.1.0",
                    "machdep.cpu.features" => "FPU SSE4.1 SSE4.2 AVX1.0",
                    "machdep.cpu.leaf7_features" => "BMI1 AVX2",
                    _ => return None,
                }
                .to_owned(),
            )
        };
        let h = from_sysctl(&sysctl, 16);
        assert_eq!(h.host_cpus, Some(16));
        assert_eq!(h.max_mhz, Some(2300));
        assert_eq!(h.features, ["sse4_1", "sse4_2", "avx", "avx2"]);
        assert_eq!(h.memory_bytes, Some(17_179_869_184));
    }
}
