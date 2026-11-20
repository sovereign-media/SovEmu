//! Operational routes and safeguards: health, security headers and response compression
//! (sign-in throttling is `sovemu_identity`'s). Metrics are OpenTelemetry's (`telemetry.rs`).

use axum::extract::{Request, State};
use axum::http::{HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use tower_http::compression::Predicate;
use tower_http::compression::predicate::{DefaultPredicate, NotForContentType};

use crate::AppState;

pub(crate) async fn health(State(state): State<AppState>) -> Response {
    match sqlx::query_scalar::<_, i64>("SELECT 1").fetch_one(&state.db).await {
        Ok(_) => "ok".into_response(),
        Err(e) => {
            tracing::error!("health check: {e}");
            (StatusCode::SERVICE_UNAVAILABLE, "database unavailable").into_response()
        }
    }
}

/// Compresses responses (gzip or Brotli, as the client accepts): the GraphQL API's JSON and the
/// web client's scripts and styles. Not images, videos or anything tiny, which compress poorly or
/// not at all.
pub(crate) fn compression() -> tower_http::compression::CompressionLayer<impl Predicate> {
    let predicate = DefaultPredicate::new().and(NotForContentType::const_new("video/"));
    tower_http::compression::CompressionLayer::new().compress_when(predicate)
}

/// Headers every response carries.
pub(crate) async fn security_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    for (name, value) in [
        ("x-content-type-options", "nosniff"),
        ("x-frame-options", "DENY"),
        ("referrer-policy", "same-origin"),
        (
            "permissions-policy",
            "gamepad=(self), fullscreen=(self), camera=(), microphone=(), geolocation=()",
        ),
    ] {
        headers.entry(name).or_insert(HeaderValue::from_static(value));
    }
    response
}
