//! The server's resources, for the admin Resources page: what the host is ([`host`]), and how
//! much of its CPU and memory the container is using ([`cgroup`]), as Sovereign's System
//! Resources.
//!
//! One sampler ([`Monitor::tick`], run by `Jobs::sampler` every `SovEmu__SystemResources__SampleSeconds`)
//! keeps the last hour of readings in memory and, each minute, writes that minute's average and
//! highest figure per resource to `resource_samples`, which maintenance prunes after
//! `SovEmu__SystemResources__RetentionDays`. The live figures and the hour's chart read the memory, the
//! longer charts the table: no request ever reads `/sys`.

pub(crate) mod cgroup;
pub(crate) mod host;

use std::collections::{BTreeMap, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use sqlx::SqlitePool;

pub(crate) use host::HostInfo;

/// How far back the in-memory readings reach.
const RING_SPAN: Duration = Duration::from_secs(3600);

/// How often a minute's figures are written to the database. Part of the table's meaning (each
/// row is one minute), so not a setting.
const ROLLUP: Duration = Duration::from_secs(60);

/// How many points a chart aims for, whatever its window.
const TARGET_POINTS: u64 = 200;

/// What a series measures. Stored in `resource_samples`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Resource {
    Cpu,
    Memory,
}

impl Resource {
    fn as_str(self) -> &'static str {
        match self {
            Resource::Cpu => "cpu",
            Resource::Memory => "memory",
        }
    }

    fn from_str(s: &str) -> Option<Resource> {
        match s {
            "cpu" => Some(Resource::Cpu),
            "memory" => Some(Resource::Memory),
            _ => None,
        }
    }
}

/// The container's use at one reading.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Usage {
    /// Over the last interval, as a share of the allowed cores, 0–100. None on the first
    /// reading, which has no interval yet.
    pub(crate) cpu_percent: Option<f64>,
    pub(crate) allowed_cores: f64,
    pub(crate) cpu_limited: bool,
    pub(crate) memory_used: u64,
    pub(crate) memory_limit: u64,
    pub(crate) memory_limited: bool,
}

impl Usage {
    /// Memory used as a share of the limit, 0–100; none if the limit is unknown.
    pub(crate) fn memory_percent(&self) -> Option<f64> {
        (self.memory_limit > 0)
            .then(|| (100.0 * self.memory_used as f64 / self.memory_limit as f64).clamp(0.0, 100.0))
    }

    fn value(&self, resource: Resource) -> Option<f64> {
        match resource {
            Resource::Cpu => self.cpu_percent,
            Resource::Memory => self.memory_percent(),
        }
    }
}

/// One reading. `usage` is none when the container's counters cannot be read.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Reading {
    /// Unix seconds.
    pub(crate) at: f64,
    pub(crate) usage: Option<Usage>,
}

/// A chart's window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Range {
    /// From the readings in memory.
    Hour,
    /// The rest from the minute rows.
    Day,
    Week,
    Month,
}

impl Range {
    fn span(self) -> Duration {
        Duration::from_secs(match self {
            Range::Hour => 3600,
            Range::Day => 86_400,
            Range::Week => 7 * 86_400,
            Range::Month => 30 * 86_400,
        })
    }
}

/// A resource's mean and highest figure over a span (a minute's rollup).
type Figure = (Resource, f64, f64);

/// One bucket of a series: its mean and its highest figure, 0–100.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Point {
    /// The bucket's start, Unix seconds.
    pub(crate) at: f64,
    pub(crate) average: f64,
    pub(crate) max: f64,
}

/// A window of history, about [`TARGET_POINTS`] buckets per resource. A bucket with no
/// readings (the server was down) is absent, never filled in.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct History {
    pub(crate) from: f64,
    pub(crate) to: f64,
    pub(crate) bucket_seconds: u64,
    pub(crate) series: Vec<(Resource, Vec<Point>)>,
}

#[derive(Debug, Default)]
struct State {
    ring: VecDeque<Reading>,
    last_cpu: Option<(u64, Instant)>,
    /// When the minute being rolled up began (Unix seconds).
    minute_start: Option<f64>,
}

/// The facts and the readings (see the module's notes).
#[derive(Debug)]
pub(crate) struct Monitor {
    pub(crate) host: HostInfo,
    /// Seconds between readings; 0: not sampled.
    pub(crate) sample_seconds: u32,
    state: Mutex<State>,
}

fn now_unix() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or_default()
}

impl Monitor {
    pub(crate) fn new(host: HostInfo, sample_seconds: u32) -> Self {
        Self { host, sample_seconds, state: Mutex::default() }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The newest reading, if there has been one.
    pub(crate) fn latest(&self) -> Option<Reading> {
        self.state().ring.back().copied()
    }

    /// Takes one reading (from the real cgroup), and writes the minute's rollup when a minute
    /// has passed.
    pub(crate) async fn tick(&self, db: &SqlitePool) -> anyhow::Result<()> {
        let counters = tokio::task::spawn_blocking(cgroup::read_system).await?;
        if let Some(rollup) = self.record(counters, now_unix(), Instant::now()) {
            write_rollup(db, rollup.0, &rollup.1).await?;
        }
        Ok(())
    }

    /// Adds a reading of `counters` taken at `at`, and returns the minute's rollup (its time
    /// and figures) once a minute has passed since the last.
    fn record(
        &self,
        counters: Option<cgroup::Counters>,
        at: f64,
        now: Instant,
    ) -> Option<(f64, Vec<Figure>)> {
        let mut state = self.state();
        let usage = counters.map(|c| {
            let cpu_percent = state.last_cpu.and_then(|(before, then)| {
                cpu_percent(before, c.cpu_usage_micros, now.duration_since(then), c.allowed_cores)
            });
            state.last_cpu = Some((c.cpu_usage_micros, now));
            Usage {
                cpu_percent,
                allowed_cores: c.allowed_cores,
                cpu_limited: c.cpu_limited,
                memory_used: c.memory_used,
                memory_limit: c.memory_limit,
                memory_limited: c.memory_limited,
            }
        });
        state.ring.push_back(Reading { at, usage });
        while state.ring.front().is_some_and(|r| at - r.at > RING_SPAN.as_secs_f64()) {
            state.ring.pop_front();
        }
        let start = *state.minute_start.get_or_insert(at);
        if at - start < ROLLUP.as_secs_f64() {
            return None;
        }
        state.minute_start = Some(at);
        let minute: Vec<Reading> =
            state.ring.iter().filter(|r| r.at > start && r.at <= at).copied().collect();
        let figures = rollup(&minute);
        (!figures.is_empty()).then_some((at, figures))
    }

    /// A window of history: the hour from memory, longer windows from the minute rows.
    pub(crate) async fn history(&self, db: &SqlitePool, range: Range) -> sqlx::Result<History> {
        let to = now_unix();
        let from = to - range.span().as_secs_f64();
        let values: Vec<(Resource, f64, f64, f64)> = if range == Range::Hour {
            let resolution = u64::from(self.sample_seconds.max(1));
            let values = self
                .state()
                .ring
                .iter()
                .flat_map(|r| {
                    [Resource::Cpu, Resource::Memory].into_iter().filter_map(move |res| {
                        let v = r.usage?.value(res)?;
                        Some((res, r.at, v, v))
                    })
                })
                .collect();
            return Ok(bucket(values, from, to, bucket_width(range.span(), resolution)));
        } else {
            let rows: Vec<(String, i64, f64, f64)> = sqlx::query_as(
                "SELECT resource, at, average, max FROM resource_samples WHERE at >= ? ORDER BY at",
            )
            .bind(from as i64)
            .fetch_all(db)
            .await?;
            rows.into_iter()
                .filter_map(|(r, at, avg, max)| {
                    Some((Resource::from_str(&r)?, at as f64, avg, max))
                })
                .collect()
        };
        Ok(bucket(values, from, to, bucket_width(range.span(), ROLLUP.as_secs())))
    }
}

/// CPU use between two counter readings as a share of the allowed cores, 0–100. None if no time
/// passed or the counter went backwards (the cgroup was recreated).
fn cpu_percent(before: u64, now: u64, elapsed: Duration, cores: f64) -> Option<f64> {
    if elapsed.is_zero() || cores <= 0.0 || now < before {
        return None;
    }
    let used = (now - before) as f64;
    Some((100.0 * used / (elapsed.as_micros() as f64 * cores)).clamp(0.0, 100.0))
}

/// Each resource's mean and highest figure over some readings. A reading without a figure adds
/// nothing (never a zero).
fn rollup(readings: &[Reading]) -> Vec<Figure> {
    [Resource::Cpu, Resource::Memory]
        .into_iter()
        .filter_map(|res| {
            let values: Vec<f64> = readings.iter().filter_map(|r| r.usage?.value(res)).collect();
            if values.is_empty() {
                return None;
            }
            let mean = values.iter().sum::<f64>() / values.len() as f64;
            Some((res, mean, values.iter().copied().fold(0.0, f64::max)))
        })
        .collect()
}

async fn write_rollup(db: &SqlitePool, at: f64, figures: &[Figure]) -> sqlx::Result<()> {
    let mut tx = crate::db::write_tx(db).await?;
    for (resource, average, max) in figures {
        sqlx::query(
            "INSERT INTO resource_samples (resource, at, average, max) VALUES (?, ?, ?, ?)",
        )
        .bind(resource.as_str())
        .bind(at as i64)
        .bind(average)
        .bind(max)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await
}

/// The bucket width that spreads `window` over [`TARGET_POINTS`], never narrower than
/// `resolution` (the readings' own spacing), in seconds.
fn bucket_width(window: Duration, resolution: u64) -> u64 {
    window.as_secs().div_ceil(TARGET_POINTS).max(resolution).max(1)
}

/// Buckets `(resource, at, average, max)` values from `from` to `to`: per resource, each
/// bucket's mean of averages and highest maximum. Empty buckets are left out.
fn bucket(values: Vec<(Resource, f64, f64, f64)>, from: f64, to: f64, width: u64) -> History {
    let mut sums: BTreeMap<(Resource, u64), (f64, u32, f64)> = BTreeMap::new();
    for (res, at, avg, max) in values {
        if at < from || at > to {
            continue;
        }
        let b = ((at - from) / width as f64) as u64;
        let e = sums.entry((res, b)).or_insert((0.0, 0, 0.0));
        e.0 += avg;
        e.1 += 1;
        e.2 = e.2.max(max);
    }
    let mut series: Vec<(Resource, Vec<Point>)> = Vec::new();
    for ((res, b), (sum, n, max)) in sums {
        let point = Point { at: from + (b * width) as f64, average: sum / f64::from(n), max };
        match series.last_mut() {
            Some((r, points)) if *r == res => points.push(point),
            _ => series.push((res, vec![point])),
        }
    }
    History { from, to, bucket_seconds: width, series }
}

/// Deletes minute rows older than `days` (0: kept). How many.
pub(crate) async fn prune(db: &SqlitePool, days: u32) -> sqlx::Result<u64> {
    if days == 0 {
        return Ok(0);
    }
    let cutoff = now_unix() as i64 - i64::from(days) * 86_400;
    let r =
        sqlx::query("DELETE FROM resource_samples WHERE at < ?").bind(cutoff).execute(db).await?;
    Ok(r.rows_affected())
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIB: u64 = 1024 * 1024 * 1024;

    fn counters(usage_micros: u64, used: u64) -> cgroup::Counters {
        cgroup::Counters {
            cpu_usage_micros: usage_micros,
            allowed_cores: 2.0,
            cpu_limited: true,
            memory_used: used,
            memory_limit: 4 * GIB,
            memory_limited: true,
        }
    }

    #[test]
    fn cpu_is_a_share_of_the_allowed_cores() {
        // One core busy for a second, of two allowed: half.
        assert_eq!(cpu_percent(0, 1_000_000, Duration::from_secs(1), 2.0), Some(50.0));
        assert_eq!(cpu_percent(5, 1, Duration::from_secs(1), 2.0), None, "counter went back");
        assert_eq!(cpu_percent(0, 1, Duration::ZERO, 2.0), None);
        assert_eq!(cpu_percent(0, 9_000_000, Duration::from_secs(1), 2.0), Some(100.0));
    }

    #[test]
    fn readings_roll_up_each_minute() {
        let m = Monitor::new(HostInfo::default(), 5);
        let t0 = Instant::now();
        // The first reading has no CPU figure yet; memory is a quarter of the limit.
        assert_eq!(m.record(Some(counters(0, GIB)), 1000.0, t0), None);
        assert_eq!(m.latest().unwrap().usage.unwrap().cpu_percent, None);
        assert_eq!(
            m.record(Some(counters(500_000, GIB)), 1030.0, t0 + Duration::from_secs(1)),
            None
        );
        assert_eq!(m.latest().unwrap().usage.unwrap().cpu_percent, Some(25.0));
        let (at, figures) = m
            .record(Some(counters(2_500_000, 3 * GIB)), 1060.0, t0 + Duration::from_secs(2))
            .expect("a minute passed");
        assert_eq!(at, 1060.0);
        // CPU 25 then 100; memory 25 then 75 (the first reading began the minute).
        assert_eq!(figures, [(Resource::Cpu, 62.5, 100.0), (Resource::Memory, 50.0, 75.0)]);
    }

    #[test]
    fn no_cgroup_is_a_reading_with_no_figures() {
        let m = Monitor::new(HostInfo::default(), 5);
        assert_eq!(m.record(None, 1000.0, Instant::now()), None);
        assert_eq!(m.latest(), Some(Reading { at: 1000.0, usage: None }));
        assert_eq!(m.record(None, 1070.0, Instant::now()), None, "nothing to write");
    }

    #[test]
    fn buckets_spread_a_window_and_leave_gaps() {
        assert_eq!(bucket_width(Duration::from_secs(3600), 5), 18);
        assert_eq!(bucket_width(Duration::from_secs(600), 5), 5, "never under the resolution");
        let h = bucket(
            vec![
                (Resource::Cpu, 0.0, 10.0, 20.0),
                (Resource::Cpu, 5.0, 30.0, 40.0),
                (Resource::Cpu, 25.0, 50.0, 50.0),
                (Resource::Memory, 1.0, 60.0, 60.0),
                (Resource::Cpu, 999.0, 1.0, 1.0),
            ],
            0.0,
            100.0,
            10,
        );
        assert_eq!(h.series.len(), 2);
        assert_eq!(
            h.series[0],
            (
                Resource::Cpu,
                vec![
                    Point { at: 0.0, average: 20.0, max: 40.0 },
                    // 10–20 had nothing: no point, not a zero.
                    Point { at: 20.0, average: 50.0, max: 50.0 },
                ]
            )
        );
        assert_eq!(h.series[1].1, [Point { at: 0.0, average: 60.0, max: 60.0 }]);
    }

    #[tokio::test]
    async fn the_day_comes_from_the_minute_rows_and_old_ones_are_pruned() {
        let dir = std::env::temp_dir().join(format!("sovemu-resources-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = crate::db::open(&dir.join("r.db")).await.unwrap();
        let now = now_unix();
        write_rollup(
            &db,
            now - 120.0,
            &[(Resource::Cpu, 10.0, 30.0), (Resource::Memory, 40.0, 41.0)],
        )
        .await
        .unwrap();
        write_rollup(&db, now - 40.0 * 86_400.0, &[(Resource::Cpu, 99.0, 99.0)]).await.unwrap();
        let m = Monitor::new(HostInfo::default(), 5);
        let day = m.history(&db, Range::Day).await.unwrap();
        assert_eq!(day.bucket_seconds, 432);
        assert_eq!(day.series.len(), 2);
        assert_eq!((day.series[0].1[0].average, day.series[0].1[0].max), (10.0, 30.0));
        assert_eq!(prune(&db, 30).await.unwrap(), 1);
        assert_eq!(prune(&db, 0).await.unwrap(), 0, "0 keeps everything");
        std::fs::remove_dir_all(&dir).ok();
    }
}
