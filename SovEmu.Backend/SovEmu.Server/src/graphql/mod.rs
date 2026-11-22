//! The API: GraphQL at `POST /graphql` (GraphiQL at `GET /graphql`), the only API the clients
//! use. Binary files (artwork, videos, save screenshots) are plain HTTP under `/media/`, whose
//! URLs the API hands out; `/health` stays plain HTTP for probes.
//!
//! Resolvers call the same functions the rest of the server does (`crate::api`, `crate::auth`,
//! …). Requests are signed in with `Authorization: Bearer <access token>`, from the server's
//! OpenID Connect provider (`crate::oidc`); cookies sign nothing in here. Errors carry a `code` in their extensions (`UNAUTHENTICATED`, `FORBIDDEN`,
//! `NOT_FOUND`, `BAD_REQUEST`, `CONFLICT`, `GONE`, `TOO_MANY_REQUESTS`, `UNAVAILABLE`, …).
//!
//! Numbers: GraphQL's `Int` is 32-bit, so byte counts and Unix times are `Float` (exact to
//! 2^53); ids and counts are `Int`. The schema is checked in as `schema.graphql` (see
//! [`tests`]), for the web, Android and iOS clients.

use std::collections::BTreeMap;

use async_graphql::extensions::{Extension, ExtensionContext, ExtensionFactory, NextParseQuery};
use async_graphql::http::GraphiQLSource;
use async_graphql::parser::types::ExecutableDocument;
use async_graphql::{
    Context, EmptySubscription, Enum, ErrorExtensions, ID, InputObject, Json, Object, Schema,
    ServerResult, SimpleObject, Variables,
};
use async_graphql_axum::{GraphQLRequest, GraphQLResponse};
use axum::extract::State;
use axum::http::header::SET_COOKIE;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse};
use tokio::sync::OnceCell;

use crate::AppState;
use crate::auth::{Credentials, NewUser, User};
use crate::error::ApiError;

mod accounts;
mod activity;
mod appearance;
mod attention;
mod client_events;
mod controllers;
mod jobs;
mod library;
mod library_admin;
mod logs;
mod resources;
mod server;
mod sessions;
mod settings;
mod statistics;
mod streaming;

#[allow(unused_imports)]
use accounts::*;
#[allow(unused_imports)]
use appearance::*;
#[allow(unused_imports)]
use controllers::*;
#[allow(unused_imports)]
use jobs::*;
#[allow(unused_imports)]
use library::*;
#[allow(unused_imports)]
use library_admin::*;
#[allow(unused_imports)]
use resources::*;
#[allow(unused_imports)]
use server::*;
#[allow(unused_imports)]
use sessions::*;
#[allow(unused_imports)]
use settings::*;

pub(crate) type ApiSchema = Schema<Query, Mutation, EmptySubscription>;

/// The schema, without per-request data. Queries deeper than 12 levels or costlier than 2000
/// fields are refused.
pub(crate) fn schema() -> ApiSchema {
    Schema::build(Query::default(), Mutation::default(), EmptySubscription)
        .extension(OperationType)
        .limit_depth(12)
        .limit_complexity(2000)
        .finish()
}

/// Per request: its headers (for the bearer access token), and the user they name, looked up
/// once.
struct Auth {
    headers: HeaderMap,
    user: OnceCell<Option<User>>,
}

pub(crate) async fn handler(
    State(state): State<AppState>,
    client: Option<axum::Extension<crate::client_ip::ClientIp>>,
    headers: HeaderMap,
    request: GraphQLRequest,
) -> GraphQLResponse {
    let auth = Auth { headers, user: OnceCell::new() };
    // The client's address as `client_ip::remember` resolved it (the admin Network page).
    let client = client.map(|c| c.0).unwrap_or_default();
    let request = request.into_inner();
    // The request span (telemetry::trace_request) names the operation, for logs and traces.
    if let Some(name) = &request.operation_name {
        tracing::Span::current().record("graphql.operation.name", name.as_str());
    }
    let request = request.data(state.clone()).data(auth).data(client);
    let response = state.schema.execute(request).await;
    log_errors(&response.errors);
    response.into()
}

/// Names the operation's type (`query`, `mutation`) on the request span, once the query is
/// parsed: the span's other GraphQL attribute, after OpenTelemetry's `graphql.operation.type`.
struct OperationType;

impl ExtensionFactory for OperationType {
    fn create(&self) -> std::sync::Arc<dyn Extension> {
        std::sync::Arc::new(OperationType)
    }
}

#[async_graphql::async_trait::async_trait]
impl Extension for OperationType {
    async fn parse_query(
        &self,
        ctx: &ExtensionContext<'_>,
        query: &str,
        variables: &Variables,
        next: NextParseQuery<'_>,
    ) -> ServerResult<ExecutableDocument> {
        let document = next.run(ctx, query, variables).await?;
        let mut types = document.operations.iter().map(|(_, op)| op.node.ty);
        if let Some(first) = types.next()
            && types.all(|ty| ty == first)
        {
            tracing::Span::current().record("graphql.operation.type", first.to_string());
        }
        Ok(document)
    }
}

/// How a GraphQL error is logged: a 200 response can still carry errors, so without this a
/// failed menu action leaves nothing in the log.
#[derive(Debug, PartialEq, Eq)]
enum ErrorKind {
    /// The query did not parse or validate (no `code`, no `path`): usually a client out of step
    /// with the schema.
    InvalidQuery,
    /// The caller's own doing, or routine (an expired token, an id that is gone): debug.
    Routine(&'static str),
    /// Worth seeing at the default level, but the server is fine (a refused action): info.
    Refused(&'static str),
    /// The server's fault, or a service it relies on: warn, and the request span is an error.
    Failed(&'static str),
}

fn error_kind(code: Option<&str>, has_path: bool) -> ErrorKind {
    match code {
        None if !has_path => ErrorKind::InvalidQuery,
        Some("UNAUTHENTICATED") => ErrorKind::Routine("UNAUTHENTICATED"),
        Some("FORBIDDEN") => ErrorKind::Routine("FORBIDDEN"),
        Some("NOT_FOUND") => ErrorKind::Routine("NOT_FOUND"),
        Some("BAD_REQUEST") => ErrorKind::Refused("BAD_REQUEST"),
        Some("CONFLICT") => ErrorKind::Refused("CONFLICT"),
        Some("GONE") => ErrorKind::Refused("GONE"),
        Some("TOO_MANY_REQUESTS") => ErrorKind::Refused("TOO_MANY_REQUESTS"),
        Some("BAD_GATEWAY") => ErrorKind::Failed("BAD_GATEWAY"),
        Some("UNAVAILABLE") => ErrorKind::Failed("UNAVAILABLE"),
        // INTERNAL, or an error that never went through `gql` (a panic, a stray `?`).
        _ => ErrorKind::Failed("INTERNAL"),
    }
}

/// A result path as text: `game.saves.0.screenshot`.
fn error_path(path: &[async_graphql::PathSegment]) -> String {
    let segments = path.iter().map(|segment| match segment {
        async_graphql::PathSegment::Field(name) => name.clone(),
        async_graphql::PathSegment::Index(i) => i.to_string(),
    });
    segments.collect::<Vec<_>>().join(".")
}

/// Logs each error in a response inside the request span (which names the operation and the
/// user), with its code as `error.type` and the result path that failed.
fn log_errors(errors: &[async_graphql::ServerError]) {
    let span = tracing::Span::current();
    for error in errors {
        let code = error.extensions.as_ref().and_then(|ext| match ext.get("code") {
            Some(async_graphql::Value::String(code)) => Some(code.as_str()),
            _ => None,
        });
        let path = error_path(&error.path);
        let message = error.message.as_str();
        match error_kind(code, !error.path.is_empty()) {
            ErrorKind::InvalidQuery => {
                span.record("error.type", "GRAPHQL_VALIDATION_FAILED");
                tracing::warn!(error = message, "graphql: invalid query");
            }
            ErrorKind::Routine(code) => {
                tracing::debug!(error.type = code, graphql.error.path = path, error = message, "graphql error");
            }
            ErrorKind::Refused(code) => {
                span.record("error.type", code);
                tracing::info!(error.type = code, graphql.error.path = path, error = message, "graphql error");
            }
            ErrorKind::Failed(code) => {
                span.record("error.type", code);
                span.record("otel.status_code", "ERROR");
                tracing::warn!(error.type = code, graphql.error.path = path, error = message, "graphql error");
            }
        }
    }
}

/// GraphiQL, for trying queries in a browser. Signed-in queries need an access token in the
/// headers pane: `{"Authorization": "Bearer …"}`.
pub(crate) async fn graphiql() -> impl IntoResponse {
    Html(GraphiQLSource::build().endpoint("/graphql").finish())
}

type Result<T> = async_graphql::Result<T>;

/// An [`ApiError`] as a GraphQL error, with its kind as `extensions.code`.
fn gql(e: ApiError) -> async_graphql::Error {
    let code = match e.status {
        StatusCode::BAD_REQUEST => "BAD_REQUEST",
        StatusCode::UNAUTHORIZED => "UNAUTHENTICATED",
        StatusCode::FORBIDDEN => "FORBIDDEN",
        StatusCode::NOT_FOUND => "NOT_FOUND",
        StatusCode::CONFLICT => "CONFLICT",
        StatusCode::GONE => "GONE",
        StatusCode::TOO_MANY_REQUESTS => "TOO_MANY_REQUESTS",
        StatusCode::BAD_GATEWAY => "BAD_GATEWAY",
        StatusCode::SERVICE_UNAVAILABLE => "UNAVAILABLE",
        _ => "INTERNAL",
    };
    async_graphql::Error::new(e.message).extend_with(|_, ext| ext.set("code", code))
}

trait Api<T> {
    fn api(self) -> Result<T>;
}

impl<T> Api<T> for crate::error::ApiResult<T> {
    fn api(self) -> Result<T> {
        self.map_err(gql)
    }
}

fn state<'a>(ctx: &Context<'a>) -> &'a AppState {
    ctx.data_unchecked::<AppState>()
}

async fn viewer(ctx: &Context<'_>) -> Result<Option<User>> {
    let auth = ctx.data_unchecked::<Auth>();
    let user = auth
        .user
        .get_or_try_init(|| async {
            let user = state(ctx).identity.user_for_request(&auth.headers).await?;
            // Every line logged for the rest of the request then names whose it was.
            if let Some(user) = &user {
                tracing::Span::current().record("user.id", user.id);
            }
            Ok::<_, sovemu_identity::Error>(user)
        })
        .await
        .map_err(|e| gql(e.into()))?;
    Ok(user.clone())
}

/// The signed-in user, or `UNAUTHENTICATED`.
async fn user(ctx: &Context<'_>) -> Result<User> {
    viewer(ctx).await?.ok_or_else(|| gql(ApiError::unauthorized()))
}

/// Records what `user` just did in the activity log, with this request's address.
fn record(ctx: &Context<'_>, user: &User, entry: crate::activity::Entry) {
    let address = ctx.data_opt::<crate::client_ip::ClientIp>().and_then(|c| c.0);
    state(ctx).activity.record(entry.by(user).from(address));
}

/// The signed-in user, who must be an admin, or `FORBIDDEN`.
async fn admin(ctx: &Context<'_>) -> Result<User> {
    let user = user(ctx).await?;
    if user.is_admin() { Ok(user) } else { Err(gql(ApiError::forbidden())) }
}

fn set_cookie(ctx: &Context<'_>, cookie: HeaderValue) {
    ctx.insert_http_header(SET_COOKIE, cookie);
}

fn unix(t: std::time::SystemTime) -> f64 {
    t.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or_default()
}

// The query root: each area's queries, in this order (the schema file's). Plain comments: a doc
// comment would become the type's description in the schema.
#[derive(async_graphql::MergedObject, Default)]
pub(crate) struct Query(
    accounts::AccountsQuery,
    library::LibraryQuery,
    sessions::SessionsQuery,
    library_admin::LibraryAdminQuery,
    server::ServerQuery,
    jobs::JobsQuery,
    resources::ResourcesQuery,
    settings::SettingsQuery,
    controllers::ControllersQuery,
    appearance::AppearanceQuery,
    streaming::StreamingQuery,
    client_events::ClientEventsQuery,
    activity::ActivityQuery,
    statistics::StatisticsQuery,
    attention::AttentionQuery,
    logs::LogsQuery,
);

// The mutation root: each area's mutations, in this order.
#[derive(async_graphql::MergedObject, Default)]
pub(crate) struct Mutation(
    accounts::AccountsMutation,
    library::LibraryMutation,
    sessions::SessionsMutation,
    library_admin::LibraryAdminMutation,
    jobs::JobsMutation,
    settings::SettingsMutation,
    controllers::ControllersMutation,
    appearance::AppearanceMutation,
    streaming::StreamingMutation,
    client_events::ClientEventsMutation,
    attention::AttentionMutation,
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_are_logged_by_how_much_they_matter() {
        assert_eq!(error_kind(None, false), ErrorKind::InvalidQuery);
        assert_eq!(
            error_kind(Some("UNAUTHENTICATED"), true),
            ErrorKind::Routine("UNAUTHENTICATED")
        );
        assert_eq!(error_kind(Some("CONFLICT"), true), ErrorKind::Refused("CONFLICT"));
        assert_eq!(error_kind(Some("UNAVAILABLE"), true), ErrorKind::Failed("UNAVAILABLE"));
        assert_eq!(error_kind(Some("INTERNAL"), true), ErrorKind::Failed("INTERNAL"));
        assert_eq!(
            error_kind(None, true),
            ErrorKind::Failed("INTERNAL"),
            "no code: never went through gql"
        );
    }

    #[test]
    fn an_error_path_reads_as_dotted_text() {
        use async_graphql::PathSegment::{Field, Index};
        let path = [Field("game".into()), Field("saves".into()), Index(0), Field("url".into())];
        assert_eq!(error_path(&path), "game.saves.0.url");
    }

    /// A query that fails to validate comes back as an error with no path or code: the case
    /// logged as an invalid query, so the classification above sees real responses' shape.
    #[tokio::test]
    async fn an_invalid_query_has_neither_path_nor_code() {
        let response = schema().execute("{ noSuchField }").await;
        let error = &response.errors[0];
        assert!(error.path.is_empty());
        assert!(error.extensions.as_ref().and_then(|e| e.get("code")).is_none());
    }

    /// The checked-in schema, which the clients build against, matches the server's. Regenerate
    /// it with `UPDATE_SCHEMA=1 cargo test -p sovemu-server schema_file`.
    #[test]
    fn schema_file_is_current() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("schema.graphql");
        let sdl = schema().sdl();
        if std::env::var_os("UPDATE_SCHEMA").is_some() {
            std::fs::write(&path, &sdl).unwrap();
        }
        let current = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(
            current == sdl,
            "schema.graphql is out of date: run UPDATE_SCHEMA=1 cargo test -p sovemu-server schema_file"
        );
    }
}
