//! Logs, traces and metrics.
//!
//! Logs go to stderr through `tracing`, as text for a person or as JSON lines for a log
//! collector (`SOVEMU_LOG_FORMAT`). Every request runs in a `request` span and every game session
//! in a `session` span, so their fields (route, status, session id, game) come with each line
//! logged inside them.
//!
//! With `SOVEMU_OTEL_ENDPOINT` set, the same spans are exported as OpenTelemetry traces and the
//! server's metrics as OpenTelemetry metrics, over OTLP/HTTP to that collector (`/v1/traces`,
//! `/v1/metrics`). The standard `OTEL_*` variables apply on top (`OTEL_SERVICE_NAME`,
//! `OTEL_RESOURCE_ATTRIBUTES`, `OTEL_EXPORTER_OTLP_HEADERS`, `OTEL_METRIC_EXPORT_INTERVAL`, …).
//! Unset, nothing is exported and the instruments are no-ops.

use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use axum::extract::{MatchedPath, Request};
use axum::middleware::Next;
use axum::response::Response;
use clap::ValueEnum;
use opentelemetry::metrics::{Counter, Histogram, ObservableGauge};
use opentelemetry::trace::TracerProvider as _;
use opentelemetry::{KeyValue, global};
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::metrics::{PeriodicReader, SdkMeterProvider};
use opentelemetry_sdk::trace::SdkTracerProvider;
use tracing::Instrument;
use tracing::field::Empty;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};

use crate::AppState;

const SERVICE: &str = "sovemu-server";

/// How often the library's numbers (accounts, games per system) are counted for the gauges.
const LIBRARY_REFRESH: Duration = Duration::from_secs(60);

/// How log lines are written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum LogFormat {
    /// For a person reading a terminal.
    Text,
    /// One JSON object per line, with the span's fields: for a log collector.
    Json,
}

impl LogFormat {
    /// The value of `--log-format` for a worker.
    pub fn as_arg(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Json => "json",
        }
    }
}

/// The exporters, kept to flush and stop them at exit.
#[derive(Default)]
pub(crate) struct Telemetry {
    tracer: Option<SdkTracerProvider>,
    meter: Option<SdkMeterProvider>,
}

impl Telemetry {
    /// Sends what is still buffered. Call outside the async runtime: the exporter's HTTP
    /// client blocks.
    pub(crate) fn shutdown(self) {
        if let Some(tracer) = self.tracer
            && let Err(e) = tracer.shutdown()
        {
            eprintln!("telemetry: flushing traces: {e}");
        }
        if let Some(meter) = self.meter
            && let Err(e) = meter.shutdown()
        {
            eprintln!("telemetry: flushing metrics: {e}");
        }
    }
}

/// Installs the log subscriber, and the OpenTelemetry exporters when `otel_endpoint` is set.
pub(crate) fn init(format: LogFormat, otel_endpoint: Option<&str>) -> Result<Telemetry> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let logs = match format {
        LogFormat::Text => tracing_subscriber::fmt::layer().boxed(),
        LogFormat::Json => tracing_subscriber::fmt::layer()
            .json()
            .flatten_event(true)
            .with_current_span(true)
            .with_span_list(false)
            .boxed(),
    };
    let mut telemetry = Telemetry::default();
    let traces = match otel_endpoint {
        Some(base) => {
            let base = base.trim_end_matches('/');
            let resource = resource();
            let spans = opentelemetry_otlp::SpanExporter::builder()
                .with_http()
                .with_endpoint(format!("{base}/v1/traces"))
                .build()
                .context("the OTLP trace exporter")?;
            let tracer = SdkTracerProvider::builder()
                .with_batch_exporter(spans)
                .with_resource(resource.clone())
                .build();
            let metrics = opentelemetry_otlp::MetricExporter::builder()
                .with_http()
                .with_endpoint(format!("{base}/v1/metrics"))
                .build()
                .context("the OTLP metric exporter")?;
            let meter = SdkMeterProvider::builder()
                .with_reader(PeriodicReader::builder(metrics).build())
                .with_resource(resource)
                .build();
            global::set_meter_provider(meter.clone());
            let layer = tracing_opentelemetry::layer().with_tracer(tracer.tracer(SERVICE));
            telemetry = Telemetry { tracer: Some(tracer), meter: Some(meter) };
            Some(layer)
        }
        None => None,
    };
    tracing_subscriber::registry()
        .with(filter)
        .with(logs)
        .with(traces)
        .try_init()
        .context("installing the log subscriber")?;
    Ok(telemetry)
}

/// The service's identity: `OTEL_SERVICE_NAME` and `OTEL_RESOURCE_ATTRIBUTES` win.
fn resource() -> Resource {
    let builder = Resource::builder()
        .with_attribute(KeyValue::new("service.version", env!("CARGO_PKG_VERSION")));
    if std::env::var_os("OTEL_SERVICE_NAME").is_some() {
        builder.build()
    } else {
        builder.with_service_name(SERVICE).build()
    }
}

/// The instruments recorded as things happen (the gauges are read by [`observe`]).
pub(crate) struct Metrics {
    /// `http.server.request.duration`: seconds per API request, by method, route and status.
    pub requests: Histogram<f64>,
    /// `sovemu.sessions.started`: game sessions started, by system.
    pub sessions_started: Counter<u64>,
    /// `sovemu.session.duration`: seconds each game session ran, by system.
    pub session_duration: Histogram<f64>,
    /// `sovemu.sessions.ended`: game sessions ended, by system, core and reason (`crashed`
    /// counts a core that keeps crashing).
    pub sessions_ended: Counter<u64>,
}

/// The instruments, made on first use (after [`init`] has installed the exporter, if any).
/// Session metrics, as `sovemu_sessions` reports sessions starting and ending.
#[derive(Debug)]
pub(crate) struct SessionMetrics;

impl sovemu_sessions::Observer for SessionMetrics {
    fn session_started(&self, system: &str) {
        metrics().sessions_started.add(1, &[KeyValue::new("system", system.to_owned())]);
    }

    fn session_ended(&self, ended: &sovemu_sessions::Ended<'_>) {
        metrics()
            .session_duration
            .record(ended.ran.as_secs_f64(), &[KeyValue::new("system", ended.system.to_owned())]);
        metrics().sessions_ended.add(
            1,
            &[
                KeyValue::new("system", ended.system.to_owned()),
                KeyValue::new("core", ended.core.to_owned()),
                KeyValue::new("reason", ended.reason.as_str()),
            ],
        );
    }
}

pub(crate) fn metrics() -> &'static Metrics {
    static METRICS: OnceLock<Metrics> = OnceLock::new();
    METRICS.get_or_init(|| {
        let meter = global::meter(SERVICE);
        Metrics {
            requests: meter
                .f64_histogram("http.server.request.duration")
                .with_unit("s")
                .with_description("Duration of API requests.")
                .build(),
            sessions_started: meter
                .u64_counter("sovemu.sessions.started")
                .with_description("Game sessions started.")
                .build(),
            session_duration: meter
                .f64_histogram("sovemu.session.duration")
                .with_unit("s")
                .with_description("How long game sessions ran.")
                .with_boundaries(vec![60.0, 300.0, 900.0, 1800.0, 3600.0, 7200.0, 14400.0])
                .build(),
            sessions_ended: meter
                .u64_counter("sovemu.sessions.ended")
                .with_description("Game sessions ended, by why.")
                .build(),
        }
    })
}

/// The gauges, read at each export: sessions running, the sessions the UDP ports allow, accounts
/// and games per system (counted every minute, so an export never waits on the database). Keep
/// the returned value for as long as they should be reported.
pub(crate) struct Gauges {
    _gauges: Vec<ObservableGauge<u64>>,
}

pub(crate) fn observe(state: &AppState) -> Gauges {
    let meter = global::meter(SERVICE);
    let mut gauges = Vec::new();
    let workers = state.workers.clone();
    gauges.push(
        meter
            .u64_observable_gauge("sovemu.sessions")
            .with_description("Game sessions running.")
            .with_callback(move |o| o.observe(workers.count() as u64, &[]))
            .build(),
    );
    let (first, last) = state.config.udp_ports;
    let capacity = u64::from(last - first) + 1;
    gauges.push(
        meter
            .u64_observable_gauge("sovemu.sessions.capacity")
            .with_description("Concurrent game sessions the UDP port range allows.")
            .with_callback(move |o| o.observe(capacity, &[]))
            .build(),
    );
    if state.config.otel_endpoint.is_none() {
        // Nothing reads the gauges: no need to count the library.
        return Gauges { _gauges: gauges };
    }

    let library: Arc<Mutex<Library>> = Arc::default();
    let counted = library.clone();
    let db = state.db.clone();
    tokio::spawn(async move {
        loop {
            match Library::count(&db).await {
                Ok(now) => *counted.lock().unwrap_or_else(|e| e.into_inner()) = now,
                Err(e) => tracing::warn!(error = %e, "counting the library for metrics"),
            }
            tokio::time::sleep(LIBRARY_REFRESH).await;
        }
    });
    let users = library.clone();
    gauges.push(
        meter
            .u64_observable_gauge("sovemu.users")
            .with_description("Accounts.")
            .with_callback(move |o| {
                o.observe(users.lock().unwrap_or_else(|e| e.into_inner()).users, &[])
            })
            .build(),
    );
    gauges.push(
        meter
            .u64_observable_gauge("sovemu.games")
            .with_description("Games in the library, by system.")
            .with_callback(move |o| {
                for (system, n) in &library.lock().unwrap_or_else(|e| e.into_inner()).games {
                    o.observe(*n, &[KeyValue::new("system", system.clone())]);
                }
            })
            .build(),
    );
    Gauges { _gauges: gauges }
}

#[derive(Default)]
struct Library {
    users: u64,
    games: Vec<(String, u64)>,
}

impl Library {
    async fn count(db: &sqlx::SqlitePool) -> sqlx::Result<Self> {
        let users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users").fetch_one(db).await?;
        let games: Vec<(String, i64)> =
            sqlx::query_as("SELECT system, COUNT(*) FROM games GROUP BY system ORDER BY system")
                .fetch_all(db)
                .await?;
        Ok(Self {
            users: users.max(0) as u64,
            games: games.into_iter().map(|(s, n)| (s, n.max(0) as u64)).collect(),
        })
    }
}

/// Runs each API request in a `request` span (exported as the server span of its trace) and
/// records its duration. A route layer, so the route is the pattern (`/media/games/{id}/{kind}`),
/// never a path with ids in it.
pub(crate) async fn trace_request(request: Request, next: Next) -> Response {
    let method = request.method().clone();
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map_or_else(|| "unmatched".to_owned(), |p| p.as_str().to_owned());
    let span = tracing::info_span!(
        "request",
        otel.name = %format!("{method} {route}"),
        otel.kind = "server",
        http.request.method = %method,
        http.route = %route,
        http.response.status_code = Empty,
        graphql.operation.name = Empty,
        graphql.operation.type = Empty,
        user.id = Empty,
        error.type = Empty,
        otel.status_code = Empty,
        client.address = Empty,
    );
    if let Some(crate::client_ip::ClientIp(Some(ip))) = request.extensions().get() {
        span.record("client.address", tracing::field::display(ip));
    }
    let start = Instant::now();
    let response = next.run(request).instrument(span.clone()).await;
    let status = response.status().as_u16();
    let elapsed = start.elapsed();
    span.record("http.response.status_code", status);
    if response.status().is_server_error() {
        span.record("otel.status_code", "ERROR");
    }
    tracing::debug!(parent: &span, status, elapsed_ms = elapsed.as_millis() as u64, "request");
    metrics().requests.record(
        elapsed.as_secs_f64(),
        &[
            KeyValue::new("http.request.method", method.to_string()),
            KeyValue::new("http.route", route),
            KeyValue::new("http.response.status_code", i64::from(status)),
        ],
    );
    response
}
