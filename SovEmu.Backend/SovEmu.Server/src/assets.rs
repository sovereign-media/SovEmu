//! The `/media/…` routes' guard: a request gets through only with a URL signed for a user who
//! still exists (`sovemu_assets` signs and checks), and is served as that user.

use axum::extract::{FromRequestParts, Request, State};
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use sovemu_assets::Refused;

use crate::AppState;
use crate::error::ApiError;

/// The user a `/media/…` request was signed for (set by [`require_signature`]).
#[derive(Debug, Clone, Copy)]
pub struct SignedUser(pub i64);

impl<S: Send + Sync> FromRequestParts<S> for SignedUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts.extensions.get::<SignedUser>().copied().ok_or_else(ApiError::unauthorized)
    }
}

/// Lets a `/media/…` request through only with a valid signature, for a user who still exists.
pub async fn require_signature(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    let uri = request.uri();
    let user = match state.assets.check(uri.path(), uri.query()) {
        Ok(user) => user,
        Err(Refused::Unsigned) => {
            return refuse(StatusCode::UNAUTHORIZED, "a signed URL is required");
        }
        Err(Refused::Forged) => {
            return refuse(StatusCode::FORBIDDEN, "the URL's signature is not valid");
        }
        Err(Refused::Expired) => {
            return refuse(StatusCode::FORBIDDEN, "the URL has expired; ask for it again");
        }
    };
    let exists: Option<i64> = sqlx::query_scalar("SELECT id FROM users WHERE id = ?")
        .bind(user)
        .fetch_optional(&state.db)
        .await
        .unwrap_or(None);
    if exists.is_none() {
        return refuse(StatusCode::FORBIDDEN, "the URL's user no longer exists");
    }
    request.extensions_mut().insert(SignedUser(user));
    next.run(request).await
}

fn refuse(status: StatusCode, message: &'static str) -> Response {
    (status, message).into_response()
}
