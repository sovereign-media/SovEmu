//! What goes wrong on the clients: every signed-in client reports it (`reportClientEvents`),
//! and admins read it back grouped by kind and client (the Client errors page).

use super::*;
use crate::client_events::Kind;

/// What went wrong on a client.
#[derive(Enum, Clone, Copy, PartialEq, Eq)]
pub(super) enum ClientEventKind {
    /// WebRTC could not connect.
    ConnectionFailed,
    /// A connected stream dropped.
    ConnectionLost,
    /// No new picture for a while during play.
    StreamStalled,
    /// A controller the client has no layout for.
    ControllerUnknown,
    /// A screen or the app failed.
    ScreenError,
    Other,
}

impl From<ClientEventKind> for Kind {
    fn from(k: ClientEventKind) -> Self {
        match k {
            ClientEventKind::ConnectionFailed => Self::ConnectionFailed,
            ClientEventKind::ConnectionLost => Self::ConnectionLost,
            ClientEventKind::StreamStalled => Self::StreamStalled,
            ClientEventKind::ControllerUnknown => Self::ControllerUnknown,
            ClientEventKind::ScreenError => Self::ScreenError,
            ClientEventKind::Other => Self::Other,
        }
    }
}

impl From<Kind> for ClientEventKind {
    fn from(k: Kind) -> Self {
        match k {
            Kind::ConnectionFailed => Self::ConnectionFailed,
            Kind::ConnectionLost => Self::ConnectionLost,
            Kind::StreamStalled => Self::StreamStalled,
            Kind::ControllerUnknown => Self::ControllerUnknown,
            Kind::ScreenError => Self::ScreenError,
            Kind::Other => Self::Other,
        }
    }
}

/// One thing that went wrong on a client.
#[derive(InputObject)]
pub(super) struct ClientEventInput {
    pub(super) kind: ClientEventKind,
    /// What happened, in a line (kept to 500 characters).
    pub(super) message: String,
    /// The game session it happened in, if any.
    pub(super) session_id: Option<ID>,
    /// When, in seconds since the Unix epoch on the client's clock.
    pub(super) at: Option<f64>,
}

impl From<ClientEventInput> for crate::client_events::Event {
    fn from(e: ClientEventInput) -> Self {
        Self {
            kind: e.kind.into(),
            message: e.message,
            session_id: e.session_id.map(|id| id.0),
            at: e.at,
        }
    }
}

/// Client errors of one kind from one client (app and version) in a window: how many, from how
/// many players, and the latest.
#[derive(SimpleObject)]
pub(super) struct ClientEventGroup {
    pub(super) kind: ClientEventKind,
    /// The app and its version, as it named itself (`android 1.4.0 (Mi TV, Android 11)`).
    pub(super) client: String,
    pub(super) count: i32,
    /// How many accounts reported it.
    pub(super) users: i32,
    /// The latest one's time (Unix seconds) and message.
    pub(super) last_at: f64,
    pub(super) last_message: String,
}

/// One client error.
#[derive(SimpleObject)]
pub(super) struct ClientEvent {
    /// When the server received it (Unix seconds).
    pub(super) at: f64,
    /// When it happened, on the client's clock (Unix seconds), if it said.
    pub(super) client_at: Option<f64>,
    /// The player; null for an account since deleted.
    pub(super) username: Option<String>,
    pub(super) client: String,
    pub(super) kind: ClientEventKind,
    pub(super) message: String,
    /// The game session it happened in, if any.
    pub(super) session_id: Option<String>,
    /// That session's game, when the server could tell (the session was still running).
    pub(super) game_id: Option<i64>,
    pub(super) title: Option<String>,
    pub(super) system: Option<String>,
}

/// Errors older than this many days are not asked for: the window is a day, a week or a month.
const MAX_DAYS: i32 = 90;
/// A group's events, at most.
const MAX_EVENTS: i32 = 200;

/// The start of the last `days` days, as Unix seconds.
fn since(days: i32) -> Result<i64> {
    if !(1..=MAX_DAYS).contains(&days) {
        return Err(gql(ApiError::bad_request(format!("days: 1 to {MAX_DAYS}"))));
    }
    Ok(crate::client_events::now_unix() - i64::from(days) * 86_400)
}

#[derive(Default)]
pub(super) struct ClientEventsQuery;

#[Object]
impl ClientEventsQuery {
    /// Admins: the client errors of the last `days` days, by kind and client, the most frequent
    /// first.
    async fn client_event_groups(
        &self,
        ctx: &Context<'_>,
        #[graphql(default = 7)] days: i32,
    ) -> Result<Vec<ClientEventGroup>> {
        admin(ctx).await?;
        let groups = crate::client_events::groups(&state(ctx).db, since(days)?)
            .await
            .map_err(ApiError::from)
            .api()?;
        Ok(groups
            .into_iter()
            .map(|g| ClientEventGroup {
                kind: Kind::parse(&g.kind).into(),
                client: g.client,
                count: g.count as i32,
                users: g.users as i32,
                last_at: g.last_at as f64,
                last_message: g.last_message,
            })
            .collect())
    }

    /// Admins: one group's client errors in the last `days` days, newest first (at most
    /// `limit`, up to 200).
    async fn client_events(
        &self,
        ctx: &Context<'_>,
        kind: ClientEventKind,
        client: String,
        #[graphql(default = 7)] days: i32,
        #[graphql(default = 50)] limit: i32,
    ) -> Result<Vec<ClientEvent>> {
        admin(ctx).await?;
        let state = state(ctx);
        let limit = limit.clamp(1, MAX_EVENTS) as u32;
        let rows =
            crate::client_events::events(&state.db, kind.into(), &client, since(days)?, limit)
                .await
                .map_err(ApiError::from)
                .api()?;
        let names = crate::api::usernames(state).await.api()?;
        Ok(rows
            .into_iter()
            .map(|r| ClientEvent {
                at: r.at as f64,
                client_at: r.client_at,
                username: names.get(&r.user_id).cloned(),
                client: r.client,
                kind: Kind::parse(&r.kind).into(),
                message: r.message,
                session_id: r.session_id,
                game_id: r.game_id,
                title: r.title,
                system: r.system,
            })
            .collect())
    }
}

#[derive(Default)]
pub(super) struct ClientEventsMutation;

#[Object]
impl ClientEventsMutation {
    /// Signed-in users: what went wrong on this client (a connection that failed, a stalled
    /// picture, a screen that crashed), for the server's log beside the session's own lines and
    /// the admin's Client errors page. `client` names it and its version (`android 0.1.0
    /// (MiTV-AFMU1, Android 11)`). At most 20 events a call and 60 a minute per user; the rest
    /// are dropped. Returns how many were taken (0 when `SovEmu__ClientErrors__Enabled` is off).
    async fn report_client_events(
        &self,
        ctx: &Context<'_>,
        client: String,
        events: Vec<ClientEventInput>,
    ) -> Result<i32> {
        let user = user(ctx).await?;
        let events: Vec<crate::client_events::Event> = events.into_iter().map(Into::into).collect();
        let taken = crate::client_events::receive(state(ctx), user.id, &client, &events)
            .await
            .map_err(ApiError::from)
            .api()?;
        Ok(taken as i32)
    }
}
