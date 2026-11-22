//! How a session is running, in the log: a `session health` line every interval while a
//! client is connected, and a `session summary` when the worker exits.
//!
//! The client's stats overlay shows the same things live, but only on the device and only while
//! it is open. These lines are what is left afterwards, so "it stuttered around eight" can be
//! looked up by session id: whether the worker kept up (ticks that overran the frame period,
//! ticks that started late), what encoding cost, what was sent against what the network allowed,
//! whether input kept arriving, and how far run-ahead was running.

use std::time::{Duration, Instant};

/// What one interval (or the whole session) added up to.
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct Stats {
    /// Frames emulated and sent.
    pub(crate) ticks: u64,
    /// Ticks whose work took longer than the frame period: the worker fell behind.
    pub(crate) overran: u64,
    /// Ticks that started more than half a period after they were due (the host was busy).
    pub(crate) started_late: u64,
    pub(crate) tick_time: Duration,
    pub(crate) worst_tick: Duration,
    pub(crate) encoded: u64,
    pub(crate) encode_time: Duration,
    pub(crate) worst_encode: Duration,
    pub(crate) video_bytes: u64,
    /// Controller packets applied (newer than the last one).
    pub(crate) inputs: u64,
    /// Ticks held because the client's menu was open.
    pub(crate) paused: u64,
    /// How long these stats cover.
    pub(crate) span: Duration,
}

impl Stats {
    fn add(&mut self, other: &Stats) {
        self.ticks += other.ticks;
        self.overran += other.overran;
        self.started_late += other.started_late;
        self.tick_time += other.tick_time;
        self.worst_tick = self.worst_tick.max(other.worst_tick);
        self.encoded += other.encoded;
        self.encode_time += other.encode_time;
        self.worst_encode = self.worst_encode.max(other.worst_encode);
        self.video_bytes += other.video_bytes;
        self.inputs += other.inputs;
        self.paused += other.paused;
        self.span += other.span;
    }

    pub(crate) fn fps(&self) -> f64 {
        per_second(self.ticks as f64, self.span)
    }

    pub(crate) fn video_kbps(&self) -> f64 {
        per_second(self.video_bytes as f64 * 8.0 / 1000.0, self.span)
    }

    pub(crate) fn mean_tick_ms(&self) -> f64 {
        mean_ms(self.tick_time, self.ticks)
    }

    pub(crate) fn mean_encode_ms(&self) -> f64 {
        mean_ms(self.encode_time, self.encoded)
    }
}

fn per_second(amount: f64, span: Duration) -> f64 {
    if span.is_zero() { 0.0 } else { round1(amount / span.as_secs_f64()) }
}

fn mean_ms(total: Duration, count: u64) -> f64 {
    if count == 0 { 0.0 } else { round1(total.as_secs_f64() * 1000.0 / count as f64) }
}

fn ms(d: Duration) -> f64 {
    round1(d.as_secs_f64() * 1000.0)
}

fn round1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

/// What the session knows at report time, beyond the counts.
pub(crate) struct Now {
    pub(crate) video_kbps_target: u32,
    /// The video encoder in use (`vaapi`, `x264`, `libvpx`…), once one has opened.
    pub(crate) video_encoder: Option<&'static str>,
    pub(crate) estimate_kbps: Option<u64>,
    pub(crate) run_ahead: u32,
}

pub(crate) struct Health {
    /// `None`: no periodic lines (the summary is still written).
    interval: Option<Duration>,
    period: Duration,
    window: Stats,
    total: Stats,
    /// When the current window began; `None` while no client is connected.
    window_from: Option<Instant>,
}

impl Health {
    /// `interval_secs` 0 writes no periodic lines.
    pub(crate) fn new(interval_secs: u32, fps: f64) -> Self {
        Self {
            interval: (interval_secs > 0).then(|| Duration::from_secs(interval_secs.into())),
            period: period(fps),
            window: Stats::default(),
            total: Stats::default(),
            window_from: None,
        }
    }

    pub(crate) fn set_fps(&mut self, fps: f64) {
        self.period = period(fps);
    }

    /// A client is connected and frames are about to run: a window starts.
    pub(crate) fn resume(&mut self, now: Instant) {
        self.window_from = Some(now);
    }

    /// The client has gone: what the window has so far joins the total, without a line.
    pub(crate) fn suspend(&mut self, now: Instant) {
        self.close_window(now);
        self.window_from = None;
    }

    /// A tick ran: `behind` is how late it started, `took` how long its work took.
    pub(crate) fn ticked(&mut self, behind: Duration, took: Duration) {
        let w = &mut self.window;
        w.ticks += 1;
        w.tick_time += took;
        w.worst_tick = w.worst_tick.max(took);
        if took > self.period {
            w.overran += 1;
        }
        if behind > self.period / 2 {
            w.started_late += 1;
        }
    }

    pub(crate) fn paused(&mut self) {
        self.window.paused += 1;
    }

    pub(crate) fn encoded(&mut self, took: Duration, bytes: usize) {
        let w = &mut self.window;
        w.encoded += 1;
        w.encode_time += took;
        w.worst_encode = w.worst_encode.max(took);
        w.video_bytes += bytes as u64;
    }

    pub(crate) fn input(&mut self) {
        self.window.inputs += 1;
    }

    /// Writes a `session health` line once the interval has passed, and starts the next.
    pub(crate) fn report_if_due(&mut self, now: Instant, at: &Now) {
        let (Some(interval), Some(from)) = (self.interval, self.window_from) else { return };
        if now.duration_since(from) < interval {
            return;
        }
        let window = self.close_window(now);
        self.window_from = Some(now);
        log_health(&window, at);
    }

    /// Writes the `session summary`: the whole session, the open window included.
    pub(crate) fn summary(&mut self, now: Instant, at: &Now) {
        self.close_window(now);
        let t = &self.total;
        tracing::info!(
            connected_s = t.span.as_secs(),
            ticks = t.ticks,
            fps = t.fps(),
            overran = t.overran,
            started_late = t.started_late,
            tick_mean_ms = t.mean_tick_ms(),
            tick_worst_ms = ms(t.worst_tick),
            encode_mean_ms = t.mean_encode_ms(),
            encode_worst_ms = ms(t.worst_encode),
            video_mb = round1(t.video_bytes as f64 / 1_000_000.0),
            video_kbps = t.video_kbps(),
            inputs = t.inputs,
            paused_ticks = t.paused,
            run_ahead = at.run_ahead,
            "session summary"
        );
    }

    /// Ends the window at `now`: its counts join the total and it starts empty.
    fn close_window(&mut self, now: Instant) -> Stats {
        let mut window = std::mem::take(&mut self.window);
        if let Some(from) = self.window_from {
            window.span = now.duration_since(from);
        }
        self.total.add(&window);
        window
    }

    #[cfg(test)]
    fn total(&self) -> &Stats {
        &self.total
    }
}

fn period(fps: f64) -> Duration {
    Duration::from_secs_f64(1.0 / fps.clamp(1.0, 1000.0))
}

/// One interval at info; at warn when the worker could not keep up, so a struggling session
/// stands out at a glance.
fn log_health(w: &Stats, at: &Now) {
    let struggling = w.overran * 20 > w.ticks || w.started_late * 20 > w.ticks;
    macro_rules! health {
        ($level:ident) => {
            tracing::$level!(
                interval_s = w.span.as_secs(),
                fps = w.fps(),
                ticks = w.ticks,
                overran = w.overran,
                started_late = w.started_late,
                tick_mean_ms = w.mean_tick_ms(),
                tick_worst_ms = ms(w.worst_tick),
                encode_mean_ms = w.mean_encode_ms(),
                encode_worst_ms = ms(w.worst_encode),
                video_kbps = w.video_kbps(),
                video_kbps_target = at.video_kbps_target,
                video_encoder = at.video_encoder,
                estimate_kbps = at.estimate_kbps,
                inputs = w.inputs,
                paused_ticks = w.paused,
                run_ahead = at.run_ahead,
                "session health"
            )
        };
    }
    if struggling {
        health!(warn);
    } else {
        health!(info);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: Duration = Duration::from_millis(1);

    fn now() -> Now {
        Now {
            video_kbps_target: 2500,
            video_encoder: Some("x264"),
            estimate_kbps: Some(4000),
            run_ahead: 1,
        }
    }

    #[test]
    fn counts_ticks_that_overran_or_started_late() {
        let mut h = Health::new(10, 60.0);
        let t = Instant::now();
        h.resume(t);
        h.ticked(Duration::ZERO, 5 * MS);
        h.ticked(Duration::ZERO, 20 * MS); // over the 16.7 ms period
        h.ticked(10 * MS, 5 * MS); // more than half a period late
        h.encoded(2 * MS, 1000);
        h.encoded(4 * MS, 3000);
        h.input();
        h.summary(t + Duration::from_secs(1), &now());
        let s = h.total();
        assert_eq!((s.ticks, s.overran, s.started_late), (3, 1, 1));
        assert_eq!(s.worst_tick, 20 * MS);
        assert_eq!(s.mean_tick_ms(), 10.0);
        assert_eq!(s.mean_encode_ms(), 3.0);
        assert_eq!(s.video_kbps(), 32.0, "4000 bytes in a second");
        assert_eq!(s.inputs, 1);
    }

    #[test]
    fn a_window_is_reported_once_its_interval_has_passed_and_joins_the_total() {
        let mut h = Health::new(10, 60.0);
        let t = Instant::now();
        h.resume(t);
        h.ticked(Duration::ZERO, MS);
        h.report_if_due(t + Duration::from_secs(5), &now());
        assert_eq!(h.window.ticks, 1, "not due yet: the window goes on");
        h.report_if_due(t + Duration::from_secs(10), &now());
        assert_eq!(h.window, Stats::default(), "reported: the next window starts empty");
        assert_eq!(h.total().ticks, 1);
        assert_eq!(h.total().span, Duration::from_secs(10));
    }

    #[test]
    fn time_without_a_client_is_not_counted() {
        let mut h = Health::new(10, 60.0);
        let t = Instant::now();
        h.resume(t);
        h.suspend(t + Duration::from_secs(3));
        h.report_if_due(t + Duration::from_secs(60), &now());
        h.resume(t + Duration::from_secs(60));
        h.summary(t + Duration::from_secs(62), &now());
        assert_eq!(h.total().span, Duration::from_secs(5));
    }

    #[test]
    fn an_interval_of_zero_writes_no_periodic_lines() {
        let mut h = Health::new(0, 60.0);
        let t = Instant::now();
        h.resume(t);
        h.ticked(Duration::ZERO, MS);
        h.report_if_due(t + Duration::from_secs(3600), &now());
        assert_eq!(h.window.ticks, 1, "never closed by a report");
    }
}
