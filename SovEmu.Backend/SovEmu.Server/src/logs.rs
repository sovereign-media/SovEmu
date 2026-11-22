//! The server's recent log, for the admin Logs page: the last `SOVEMU_LOG_BUFFER_LINES` lines
//! kept in memory (0 keeps none, and the page says the view is off). Lines still go to stderr as
//! ever; this is only a view of them.
//!
//! Two sources fill it: the server's own events, through [`LogLayer`] (a `tracing` layer behind
//! the same filter as stderr), each with its fields and the `session.id` of the span it ran in;
//! and each game's worker's lines, through `sovemu_sessions`' observer ([`LogBuffer`] is one),
//! tagged with their session. A field that holds an address (`…url`, `…endpoint`) goes through
//! `redact::url`, since the view is served over HTTP.

use std::collections::VecDeque;
use std::fmt::Debug;
use std::sync::Mutex;

use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber, span};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;

/// The buffer installed at startup ([`install`]), if `SOVEMU_LOG_BUFFER_LINES` keeps any.
static INSTALLED: std::sync::OnceLock<std::sync::Arc<LogBuffer>> = std::sync::OnceLock::new();

/// Makes the process's buffer, before logging starts: none when `kept` is 0.
pub(crate) fn install(kept: usize) -> Option<std::sync::Arc<LogBuffer>> {
    (kept > 0).then(|| INSTALLED.get_or_init(|| std::sync::Arc::new(LogBuffer::new(kept))).clone())
}

/// The buffer [`install`] made, if any.
pub(crate) fn installed() -> Option<std::sync::Arc<LogBuffer>> {
    INSTALLED.get().cloned()
}

/// How much a line says, least first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Level {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl Level {
    fn of(level: &tracing::Level) -> Self {
        match *level {
            tracing::Level::ERROR => Self::Error,
            tracing::Level::WARN => Self::Warn,
            tracing::Level::INFO => Self::Info,
            tracing::Level::DEBUG => Self::Debug,
            tracing::Level::TRACE => Self::Trace,
        }
    }

    fn parse(word: &str) -> Option<Self> {
        Some(match word.to_ascii_uppercase().as_str() {
            "ERROR" => Self::Error,
            "WARN" => Self::Warn,
            "INFO" => Self::Info,
            "DEBUG" => Self::Debug,
            "TRACE" => Self::Trace,
            _ => return None,
        })
    }
}

/// One line of the log.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LogLine {
    /// Its place in the buffer: a page's cursor.
    pub(crate) id: u64,
    /// When (Unix seconds).
    pub(crate) at: f64,
    pub(crate) level: Level,
    /// The module that logged it (`sovemu_server::jobs`), or `worker` for a worker's line.
    pub(crate) target: String,
    pub(crate) message: String,
    /// Its other fields, in order, addresses redacted.
    pub(crate) fields: Vec<(String, String)>,
    /// The game session it belongs to, if any.
    pub(crate) session: Option<String>,
}

/// Which lines a view asks for.
#[derive(Debug, Clone, Default)]
pub(crate) struct Filter {
    /// This level and above.
    pub(crate) level: Option<Level>,
    pub(crate) session: Option<String>,
    /// Somewhere in the message, the target or a field (ignoring case).
    pub(crate) text: Option<String>,
}

impl Filter {
    fn keeps(&self, line: &LogLine) -> bool {
        self.level.is_none_or(|l| line.level >= l)
            && self.session.as_ref().is_none_or(|s| line.session.as_ref() == Some(s))
            && self.text.as_ref().is_none_or(|t| {
                let t = t.to_lowercase();
                line.message.to_lowercase().contains(&t)
                    || line.target.to_lowercase().contains(&t)
                    || line.fields.iter().any(|(_, v)| v.to_lowercase().contains(&t))
            })
    }
}

/// The last lines, oldest first.
#[derive(Debug)]
pub(crate) struct LogBuffer {
    kept: usize,
    inner: Mutex<(VecDeque<LogLine>, u64)>,
}

impl LogBuffer {
    /// Keeps the last `kept` lines.
    pub(crate) fn new(kept: usize) -> Self {
        Self { kept, inner: Mutex::new((VecDeque::with_capacity(kept.min(4096)), 0)) }
    }

    fn push(&self, mut line: LogLine) {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.1 += 1;
        line.id = inner.1;
        if inner.0.len() == self.kept {
            inner.0.pop_front();
        }
        inner.0.push_back(line);
    }

    /// The newest `limit` lines `filter` keeps after line `after`, oldest first, and the cursor
    /// to read on from (the newest line there is, kept or not).
    pub(crate) fn read(&self, filter: &Filter, after: u64, limit: usize) -> (Vec<LogLine>, u64) {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let mut lines: Vec<LogLine> = inner
            .0
            .iter()
            .rev()
            .take_while(|l| l.id > after)
            .filter(|l| filter.keeps(l))
            .take(limit)
            .cloned()
            .collect();
        lines.reverse();
        (lines, inner.1)
    }

    /// A worker's line, as it wrote it: in the text format (`… INFO target: message`) or JSON.
    fn worker(&self, session_id: &str, raw: &str) {
        let (level, message) = match serde_json::from_str::<serde_json::Value>(raw) {
            Ok(json) if json.is_object() => (
                json["level"].as_str().and_then(Level::parse),
                json["message"]
                    .as_str()
                    .or_else(|| json["fields"]["message"].as_str())
                    .unwrap_or(raw)
                    .to_owned(),
            ),
            _ => (raw.split_whitespace().take(3).find_map(Level::parse), raw.to_owned()),
        };
        self.push(LogLine {
            id: 0,
            at: now(),
            level: level.unwrap_or(Level::Info),
            target: "worker".into(),
            message,
            fields: Vec::new(),
            session: Some(session_id.to_owned()),
        });
    }
}

impl sovemu_sessions::Observer for LogBuffer {
    fn worker_line(&self, session_id: &str, line: &str) {
        self.worker(session_id, line);
    }
}

fn now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}

/// An event's or a span's fields, as text.
#[derive(Default)]
struct Fields(Vec<(String, String)>);

impl Fields {
    fn add(&mut self, field: &Field, value: String) {
        let name = field.name();
        let value = if name.ends_with("url") || name.ends_with("endpoint") {
            crate::redact::url(&value)
        } else {
            value
        };
        self.0.push((name.to_owned(), value));
    }

    fn take(&mut self, name: &str) -> Option<String> {
        let at = self.0.iter().position(|(k, _)| k == name)?;
        Some(self.0.remove(at).1)
    }
}

impl Visit for Fields {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.add(field, value.to_owned());
    }

    fn record_debug(&mut self, field: &Field, value: &dyn Debug) {
        self.add(field, format!("{value:?}"));
    }
}

/// A span's game session, kept on the span for its events.
struct SessionOf(String);

/// Copies each event the filter lets through into the buffer.
pub(crate) struct LogLayer(pub(crate) std::sync::Arc<LogBuffer>);

impl<S> Layer<S> for LogLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &span::Attributes<'_>, id: &span::Id, ctx: Context<'_, S>) {
        let mut fields = Fields::default();
        attrs.record(&mut fields);
        if let (Some(session), Some(span)) = (fields.take("session.id"), ctx.span(id)) {
            span.extensions_mut().insert(SessionOf(session));
        }
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        let message = fields.take("message").unwrap_or_default();
        let session = fields.take("session.id").or_else(|| {
            ctx.event_scope(event)?
                .find_map(|span| span.extensions().get::<SessionOf>().map(|s| s.0.clone()))
        });
        let meta = event.metadata();
        self.0.push(LogLine {
            id: 0,
            at: now(),
            level: Level::of(meta.level()),
            target: meta.target().to_owned(),
            message,
            fields: fields.0,
            session,
        });
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tracing_subscriber::layer::SubscriberExt;

    use super::*;

    #[test]
    fn keeps_events_with_their_fields_and_session_and_the_last_n() {
        let buffer = Arc::new(LogBuffer::new(3));
        let subscriber = tracing_subscriber::registry().with(LogLayer(buffer.clone()));
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(target: "sovemu_server::jobs", job = "scan", "job finished");
            let span = tracing::info_span!("session", session.id = "abc123", system = "nes");
            span.in_scope(|| tracing::warn!(endpoint = "http://user:pw@otel:4318", "slow"));
            tracing::debug!("one");
            tracing::error!("two");
        });
        let (all, cursor) = buffer.read(&Filter::default(), 0, 10);
        assert_eq!(cursor, 4);
        assert_eq!(all.len(), 3, "the last three of four");
        let slow = &all[0];
        assert_eq!((slow.level, slow.message.as_str()), (Level::Warn, "slow"));
        assert_eq!(slow.session.as_deref(), Some("abc123"), "from the span it ran in");
        assert!(!slow.fields[0].1.contains("pw"), "an address is redacted: {:?}", slow.fields);

        let warn_up = Filter { level: Some(Level::Warn), ..Filter::default() };
        assert_eq!(buffer.read(&warn_up, 0, 10).0.len(), 2);
        let session = Filter { session: Some("abc123".into()), ..Filter::default() };
        assert_eq!(buffer.read(&session, 0, 10).0.len(), 1);
        let text = Filter { text: Some("TWO".into()), ..Filter::default() };
        assert_eq!(buffer.read(&text, 0, 10).0[0].message, "two");
        assert!(buffer.read(&Filter::default(), cursor, 10).0.is_empty(), "nothing new");
        assert_eq!(buffer.read(&Filter::default(), 0, 1).0[0].message, "two", "the newest");
    }

    #[test]
    fn a_workers_lines_keep_their_level_in_either_format() {
        let buffer = LogBuffer::new(10);
        buffer
            .worker("s1", "2026-11-22T12:00:00+00:00  WARN sovemu_worker::stream: falling behind");
        buffer.worker("s1", r#"{"level":"ERROR","fields":{"message":"core crashed"}}"#);
        buffer.worker("s2", "a line with no level");
        let (lines, _) = buffer.read(&Filter::default(), 0, 10);
        let got: Vec<(Level, &str, Option<&str>)> =
            lines.iter().map(|l| (l.level, l.message.as_str(), l.session.as_deref())).collect();
        assert_eq!(got[0].0, Level::Warn);
        assert_eq!((got[1].0, got[1].1), (Level::Error, "core crashed"));
        assert_eq!((got[2].0, got[2].2), (Level::Info, Some("s2")));
    }
}
