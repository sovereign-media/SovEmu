//! The host's CPU and memory, and their history.

use super::*;

/// What the server's host is. Read once at startup; anything unreadable is null.
#[derive(SimpleObject)]
pub(super) struct HostFacts {
    /// `AMD Ryzen 7 5800U with Radeon Graphics`.
    pub(super) cpu_model: Option<String>,
    /// `x86_64`, `aarch64`.
    pub(super) arch: String,
    /// Logical CPUs on the host.
    pub(super) host_cpus: Option<i32>,
    /// Logical CPUs the server may run on (its affinity; a quota is in `usage.allowedCores`).
    pub(super) usable_cpus: i32,
    /// The CPU's highest clock, MHz.
    pub(super) max_mhz: Option<i32>,
    /// Instruction sets some cores need or run better with: `sse4_1`, `sse4_2`, `avx`, `avx2`,
    /// `avx512f` (x86 only).
    pub(super) features: Vec<String>,
    /// The host's memory, bytes.
    pub(super) memory_bytes: Option<f64>,
    /// The kernel's release.
    pub(super) kernel: Option<String>,
}

/// How much of its CPU and memory the container (server and game sessions) is using.
#[derive(SimpleObject)]
pub(super) struct ContainerUsage {
    /// When it was read (Unix seconds).
    pub(super) sampled_at: f64,
    /// Over the last interval, a share (0–100) of `allowedCores`; null on the first reading.
    pub(super) cpu_percent: Option<f64>,
    /// The cores the container may use: its quota, or the host's count.
    pub(super) allowed_cores: f64,
    /// Whether a CPU quota is set (else `allowedCores` is the host's count).
    pub(super) cpu_limited: bool,
    /// In use, less page cache it can drop.
    pub(super) memory_used_bytes: f64,
    /// The container's limit, or the host's memory when it has none.
    pub(super) memory_limit_bytes: f64,
    pub(super) memory_limited: bool,
    /// `memoryUsedBytes` as a share (0–100) of `memoryLimitBytes`.
    pub(super) memory_percent: Option<f64>,
}

/// The admin Resources page: the host, and the container's latest use.
#[derive(SimpleObject)]
pub(super) struct ServerResources {
    pub(super) host: HostFacts,
    /// Seconds between readings (`SOVEMU_RESOURCE_SAMPLE_SECONDS`; 0: not read).
    pub(super) sample_seconds: i32,
    /// The latest reading; null if there is none (see `usageNote`).
    pub(super) usage: Option<ContainerUsage>,
    /// Why there is no reading: sampling is off, it has not run yet, or there is no cgroup to
    /// read (outside a container, or on macOS).
    pub(super) usage_note: Option<String>,
    /// When this server process started (Unix seconds): its uptime.
    pub(super) started_at: f64,
}

/// A chart's window.
#[derive(Enum, Clone, Copy, PartialEq, Eq)]
pub(super) enum ResourceRange {
    Hour,
    Day,
    Week,
    Month,
}

/// What a series measures.
#[derive(Enum, Clone, Copy, PartialEq, Eq)]
pub(super) enum ResourceKind {
    /// A share of the allowed cores.
    Cpu,
    /// A share of the memory limit.
    Memory,
}

/// One bucket of a series, as shares (0–100).
#[derive(SimpleObject)]
pub(super) struct ResourcePoint {
    /// The bucket's start (Unix seconds).
    pub(super) at: f64,
    pub(super) average: f64,
    pub(super) max: f64,
}

#[derive(SimpleObject)]
pub(super) struct ResourceSeries {
    pub(super) resource: ResourceKind,
    /// Oldest first. A bucket with no readings (the server was down) is absent.
    pub(super) points: Vec<ResourcePoint>,
}

/// A window of CPU and memory history, about 200 buckets per series.
#[derive(SimpleObject)]
pub(super) struct ResourceHistory {
    /// Unix seconds.
    pub(super) from: f64,
    pub(super) to: f64,
    /// A bucket's width: a gap wider than this between two points had no readings.
    pub(super) bucket_seconds: i32,
    pub(super) series: Vec<ResourceSeries>,
}

#[derive(Default)]
pub(super) struct ResourcesQuery;

#[Object]
impl ResourcesQuery {
    /// Admins: the server's host and how much of its CPU and memory is in use.
    async fn server_resources(&self, ctx: &Context<'_>) -> Result<ServerResources> {
        admin(ctx).await?;
        let r = &state(ctx).resources;
        let h = &r.host;
        let latest = r.latest();
        let usage_note = match latest {
            _ if r.sample_seconds == 0 => {
                Some("Not read: SOVEMU_RESOURCE_SAMPLE_SECONDS is 0.".to_owned())
            }
            None => Some("Not read yet.".to_owned()),
            Some(reading) if reading.usage.is_none() => Some(
                "No cgroup to read: the server is not running in a container (or this is macOS)."
                    .to_owned(),
            ),
            Some(_) => None,
        };
        Ok(ServerResources {
            host: HostFacts {
                cpu_model: h.cpu_model.clone(),
                arch: h.arch.to_owned(),
                host_cpus: h.host_cpus.map(|n| n as i32),
                usable_cpus: h.usable_cpus as i32,
                max_mhz: h.max_mhz.map(|m| m as i32),
                features: h.features.iter().map(|f| (*f).to_owned()).collect(),
                memory_bytes: h.memory_bytes.map(|b| b as f64),
                kernel: h.kernel.clone(),
            },
            sample_seconds: r.sample_seconds as i32,
            usage: latest.and_then(|reading| {
                let u = reading.usage?;
                Some(ContainerUsage {
                    sampled_at: reading.at,
                    cpu_percent: u.cpu_percent,
                    allowed_cores: u.allowed_cores,
                    cpu_limited: u.cpu_limited,
                    memory_used_bytes: u.memory_used as f64,
                    memory_limit_bytes: u.memory_limit as f64,
                    memory_limited: u.memory_limited,
                    memory_percent: u.memory_percent(),
                })
            }),
            usage_note,
            started_at: state(ctx).started_at,
        })
    }

    /// Admins: the container's CPU and memory over a window: the last hour from the readings
    /// in memory, longer windows from a row per minute.
    async fn resource_history(
        &self,
        ctx: &Context<'_>,
        range: ResourceRange,
    ) -> Result<ResourceHistory> {
        use crate::resources::{Range, Resource};
        admin(ctx).await?;
        let state = state(ctx);
        let range = match range {
            ResourceRange::Hour => Range::Hour,
            ResourceRange::Day => Range::Day,
            ResourceRange::Week => Range::Week,
            ResourceRange::Month => Range::Month,
        };
        let h = state.resources.history(&state.db, range).await.map_err(|e| gql(e.into()))?;
        Ok(ResourceHistory {
            from: h.from,
            to: h.to,
            bucket_seconds: h.bucket_seconds as i32,
            series: h
                .series
                .into_iter()
                .map(|(resource, points)| ResourceSeries {
                    resource: match resource {
                        Resource::Cpu => ResourceKind::Cpu,
                        Resource::Memory => ResourceKind::Memory,
                    },
                    points: points
                        .into_iter()
                        .map(|p| ResourcePoint { at: p.at, average: p.average, max: p.max })
                        .collect(),
                })
                .collect(),
        })
    }
}
