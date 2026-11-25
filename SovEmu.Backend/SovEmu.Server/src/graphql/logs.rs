//! The server's recent log (`logs`): its own lines and its game sessions'.

use super::*;
use crate::logs::{Filter, Level};

/// How much a line says.
#[derive(Enum, Clone, Copy, PartialEq, Eq)]
pub(super) enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl From<LogLevel> for Level {
    fn from(l: LogLevel) -> Self {
        match l {
            LogLevel::Trace => Self::Trace,
            LogLevel::Debug => Self::Debug,
            LogLevel::Info => Self::Info,
            LogLevel::Warn => Self::Warn,
            LogLevel::Error => Self::Error,
        }
    }
}

impl From<Level> for LogLevel {
    fn from(l: Level) -> Self {
        match l {
            Level::Trace => Self::Trace,
            Level::Debug => Self::Debug,
            Level::Info => Self::Info,
            Level::Warn => Self::Warn,
            Level::Error => Self::Error,
        }
    }
}

/// One of a line's fields.
#[derive(SimpleObject)]
pub(super) struct LogField {
    pub(super) key: String,
    pub(super) value: String,
}

/// One line of the log.
#[derive(SimpleObject)]
pub(super) struct LogLineNode {
    /// Its place in the buffer.
    pub(super) id: f64,
    /// When (Unix seconds).
    pub(super) at: f64,
    pub(super) level: LogLevel,
    /// The module that logged it, or `worker` for a game session's worker.
    pub(super) target: String,
    pub(super) message: String,
    pub(super) fields: Vec<LogField>,
    /// The game session it belongs to, if any.
    pub(super) session: Option<String>,
}

/// Lines of the log, and where to read on from.
#[derive(SimpleObject)]
pub(super) struct LogPage {
    /// Whether the server keeps its log for this view (`SovEmu__Logging__BufferLines` above 0).
    pub(super) enabled: bool,
    /// Oldest first.
    pub(super) lines: Vec<LogLineNode>,
    /// Give as `after` to read only what comes next.
    pub(super) cursor: f64,
}

const MAX_LINES: i32 = 1000;

#[derive(Default)]
pub(super) struct LogsQuery;

#[Object]
impl LogsQuery {
    /// Admins: the server's recent log, its own lines and its game sessions': the newest
    /// `limit` (1000 at most) at `level` and above, of one `session`, holding `text`, after line
    /// `after`. Follow it by asking again with the last `cursor`.
    async fn logs(
        &self,
        ctx: &Context<'_>,
        after: Option<f64>,
        level: Option<LogLevel>,
        session: Option<String>,
        text: Option<String>,
        #[graphql(default = 300)] limit: i32,
    ) -> Result<LogPage> {
        admin(ctx).await?;
        let Some(buffer) = &state(ctx).logs else {
            return Ok(LogPage { enabled: false, lines: Vec::new(), cursor: 0.0 });
        };
        let filter = Filter {
            level: level.map(Into::into),
            session: session.filter(|s| !s.trim().is_empty()),
            text: text.filter(|t| !t.trim().is_empty()),
        };
        let after = after.unwrap_or(0.0).max(0.0) as u64;
        let (lines, cursor) = buffer.read(&filter, after, limit.clamp(1, MAX_LINES) as usize);
        Ok(LogPage {
            enabled: true,
            lines: lines
                .into_iter()
                .map(|l| LogLineNode {
                    id: l.id as f64,
                    at: l.at,
                    level: l.level.into(),
                    target: l.target,
                    message: l.message,
                    fields: l
                        .fields
                        .into_iter()
                        .map(|(key, value)| LogField { key, value })
                        .collect(),
                    session: l.session,
                })
                .collect(),
            cursor: cursor as f64,
        })
    }
}
