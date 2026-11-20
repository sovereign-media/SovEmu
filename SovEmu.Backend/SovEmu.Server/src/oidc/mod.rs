//! The server is its own OpenID Connect provider. Every app signs in here and calls the API with
//! the bearer access token it is given.
//!
//! Two clients are built in, both public (an app has no secret to keep):
//! - `sovemu-web`, the web client: the authorization code flow with PKCE (S256, required) and
//!   refresh tokens. No password grant: the browser's password goes only to this server's own
//!   sign-in form (`POST /oauth/signin`), never to the token endpoint.
//! - `sovemu-native`, the Android and iOS apps: the password grant and refresh tokens. A
//!   television has no browser to sign in with, so the apps keep their own sign-in screen.
//!
//! The routes are plain HTTP because OAuth defines them so (the API itself stays GraphQL):
//! discovery (`/.well-known/openid-configuration`), `/oauth/jwks`, `/oauth/authorize`,
//! `/oauth/signin` (the sign-in form posts here), `/oauth/token`, `/oauth/revoke`,
//! `/oauth/userinfo` and `/oauth/logout` (end session).
//!
//! The issuer is `SOVEMU_PUBLIC_URL` when set, otherwise the address the request came to (its
//! `Host`, with `X-Forwarded-Proto` or `SOVEMU_SECURE_COOKIES` choosing https), so the same
//! server works at its LAN address and behind a proxy without configuration.

pub(crate) mod keys;
pub(crate) mod tokens;

use std::collections::BTreeMap;

use axum::extract::{Form, OriginalUri, Query, State};
use axum::http::header::{
    AUTHORIZATION, CACHE_CONTROL, HOST, LOCATION, ORIGIN, PRAGMA, SET_COOKIE,
};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::AppState;
use crate::auth::{self, Credentials, User};
use crate::client_ip::ClientIp;
use crate::error::ApiError;
use tokens::{Code, Grant, Issued, Lifetimes, RefreshRefused};

/// An app that may sign in here.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Client {
    pub(crate) id: &'static str,
    /// The authorization code flow (with PKCE), returning to `CALLBACK_PATH` on this server.
    code: bool,
    /// The resource owner password grant.
    password: bool,
}

pub(crate) const WEB: Client = Client { id: "sovemu-web", code: true, password: false };
pub(crate) const NATIVE: Client = Client { id: "sovemu-native", code: false, password: true };
// Checked when the server is compiled: the web client never takes a password.
const _: () = assert!(WEB.code && !WEB.password);

fn client(id: Option<&str>) -> Option<&'static Client> {
    [&WEB, &NATIVE].into_iter().find(|c| Some(c.id) == id)
}

/// Where the web client receives its code: a page on this server.
const CALLBACK_PATH: &str = "/auth/callback/";
/// The web client's sign-in page, which posts to `/oauth/signin`.
const LOGIN_PAGE: &str = "/login/";

const SCOPES: [&str; 3] = ["openid", "profile", "offline_access"];

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/.well-known/openid-configuration", get(discovery))
        .route("/oauth/jwks", get(jwks))
        .route("/oauth/authorize", get(authorize))
        .route("/oauth/signin", post(signin))
        .route("/oauth/token", post(token))
        .route("/oauth/revoke", post(revoke))
        .route("/oauth/userinfo", get(userinfo).post(userinfo))
        .route("/oauth/logout", get(logout))
}

/// The bearer token in an `Authorization` header.
pub(crate) fn bearer(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    let token = token.trim();
    (scheme.eq_ignore_ascii_case("bearer") && !token.is_empty()).then(|| token.to_owned())
}

// --- Issuer ------------------------------------------------------------------------------------

/// The `Host` a request came to.
fn host(headers: &HeaderMap) -> Option<&str> {
    headers.get(HOST)?.to_str().ok().filter(|h| !h.is_empty())
}

/// This provider's issuer for a request (no trailing slash).
fn issuer(state: &AppState, headers: &HeaderMap) -> String {
    if let Some(url) = &state.config.public_url {
        return url.trim_end_matches('/').to_owned();
    }
    let forwarded = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(str::trim)
        .filter(|p| *p == "https" || *p == "http");
    let scheme = forwarded.unwrap_or(if state.config.secure_cookies { "https" } else { "http" });
    format!("{scheme}://{}", host(headers).unwrap_or("localhost"))
}

/// Whether `uri` is `path` on this server: its host and port are the ones the request came to
/// (or the public URL's).
fn on_this_server(state: &AppState, headers: &HeaderMap, uri: &str, path: &str) -> bool {
    let Ok(url) = url::Url::parse(uri) else { return false };
    let authority = |u: &url::Url| {
        u.host_str().map(|h| match u.port() {
            Some(p) => format!("{h}:{p}"),
            None => h.to_owned(),
        })
    };
    let ours = match &state.config.public_url {
        Some(public) => url::Url::parse(public).ok().as_ref().and_then(authority),
        None => host(headers).map(str::to_owned),
    };
    matches!(url.scheme(), "http" | "https")
        && url.path() == path
        && url.query().is_none()
        && url.fragment().is_none()
        && ours.is_some_and(|o| authority(&url).is_some_and(|a| a.eq_ignore_ascii_case(&o)))
}

// --- Discovery ---------------------------------------------------------------------------------

async fn discovery(State(state): State<AppState>, headers: HeaderMap) -> Json<Value> {
    let issuer = issuer(&state, &headers);
    Json(json!({
        "issuer": issuer,
        "authorization_endpoint": format!("{issuer}/oauth/authorize"),
        "token_endpoint": format!("{issuer}/oauth/token"),
        "userinfo_endpoint": format!("{issuer}/oauth/userinfo"),
        "jwks_uri": format!("{issuer}/oauth/jwks"),
        "revocation_endpoint": format!("{issuer}/oauth/revoke"),
        "end_session_endpoint": format!("{issuer}/oauth/logout"),
        "response_types_supported": ["code"],
        "response_modes_supported": ["query"],
        "grant_types_supported": ["authorization_code", "refresh_token", "password"],
        "subject_types_supported": ["public"],
        "id_token_signing_alg_values_supported": ["ES256"],
        "scopes_supported": SCOPES,
        "token_endpoint_auth_methods_supported": ["none"],
        "revocation_endpoint_auth_methods_supported": ["none"],
        "code_challenge_methods_supported": ["S256"],
        "claims_supported": [
            "sub", "iss", "aud", "exp", "iat", "auth_time", "nonce", "name",
            "preferred_username", "role",
        ],
        "authorization_response_iss_parameter_supported": true,
    }))
}

async fn jwks(State(state): State<AppState>) -> Json<Value> {
    Json(state.token_keys.jwks())
}

// --- Authorization endpoint --------------------------------------------------------------------

#[derive(Debug, Default, Deserialize)]
struct AuthorizeParams {
    response_type: Option<String>,
    client_id: Option<String>,
    redirect_uri: Option<String>,
    scope: Option<String>,
    state: Option<String>,
    nonce: Option<String>,
    code_challenge: Option<String>,
    code_challenge_method: Option<String>,
    prompt: Option<String>,
}

/// A page-level refusal: the request cannot be trusted to say where to send the browser.
fn refuse(message: &str) -> Response {
    (StatusCode::BAD_REQUEST, [(CACHE_CONTROL, "no-store")], format!("Sign-in refused: {message}"))
        .into_response()
}

fn redirect(to: &str) -> Response {
    match HeaderValue::from_str(to) {
        Ok(location) => (StatusCode::SEE_OTHER, [(LOCATION, location)]).into_response(),
        Err(_) => refuse("bad redirect"),
    }
}

/// Back to the client with `params` added to its redirect URI.
fn back_to(redirect_uri: &str, params: &[(&str, &str)]) -> Response {
    let mut url = match url::Url::parse(redirect_uri) {
        Ok(url) => url,
        Err(_) => return refuse("bad redirect_uri"),
    };
    url.query_pairs_mut().extend_pairs(params);
    redirect(url.as_str())
}

/// The scopes asked for that this provider knows, in a fixed order.
fn granted_scope(asked: Option<&str>) -> String {
    let asked: Vec<&str> = asked.unwrap_or_default().split_whitespace().collect();
    SCOPES.into_iter().filter(|s| asked.contains(s)).collect::<Vec<_>>().join(" ")
}

async fn authorize(
    State(state): State<AppState>,
    headers: HeaderMap,
    OriginalUri(uri): OriginalUri,
    Query(params): Query<AuthorizeParams>,
) -> Response {
    // Until the client and its redirect URI check out, errors are shown here, never redirected.
    let Some(client) = client(params.client_id.as_deref()).filter(|c| c.code) else {
        return refuse("unknown client, or one that does not use this flow");
    };
    let Some(redirect_uri) = params
        .redirect_uri
        .as_deref()
        .filter(|r| on_this_server(&state, &headers, r, CALLBACK_PATH))
    else {
        return refuse("redirect_uri is not this server's sign-in callback");
    };
    let issuer = issuer(&state, &headers);
    let st = params.state.as_deref().unwrap_or_default();
    let fail = |error: &str, description: &str| {
        let mut out = vec![("error", error), ("error_description", description)];
        if !st.is_empty() {
            out.push(("state", st));
        }
        out.push(("iss", issuer.as_str()));
        back_to(redirect_uri, &out)
    };
    if params.response_type.as_deref() != Some("code") {
        return fail("unsupported_response_type", "only response_type=code is supported");
    }
    let challenge = params.code_challenge.as_deref().unwrap_or_default();
    if params.code_challenge_method.as_deref() != Some("S256") || !valid_pkce_value(challenge) {
        return fail("invalid_request", "PKCE is required: code_challenge with method S256");
    }

    let prompt: Vec<&str> = params.prompt.as_deref().unwrap_or_default().split(' ').collect();
    let signed_in = match auth::browser_user(&state.db, &headers).await {
        Ok(found) => found.filter(|_| !prompt.contains(&"login")),
        Err(e) => return e.into_response(),
    };
    let Some((user, auth_time)) = signed_in else {
        if prompt.contains(&"none") {
            return fail("login_required", "not signed in");
        }
        // To the sign-in page, which comes back here (without `prompt=login`, or it would ask
        // forever) once the browser is signed in.
        let pairs = url::form_urlencoded::parse(uri.query().unwrap_or_default().as_bytes())
            .filter(|(k, _)| k != "prompt");
        let back =
            url::form_urlencoded::Serializer::new(String::new()).extend_pairs(pairs).finish();
        let next = format!("/oauth/authorize?{back}");
        let page = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("continue", &next)
            .finish();
        return redirect(&format!("{LOGIN_PAGE}?{page}"));
    };

    let code = Code {
        client_id: client.id.to_owned(),
        user_id: user.id,
        redirect_uri: redirect_uri.to_owned(),
        code_challenge: challenge.to_owned(),
        scope: granted_scope(params.scope.as_deref()),
        nonce: params.nonce.clone(),
        auth_time,
    };
    match tokens::new_code(&state.db, &code).await {
        Ok(value) => {
            let mut out = vec![("code", value.as_str())];
            if !st.is_empty() {
                out.push(("state", st));
            }
            out.push(("iss", issuer.as_str()));
            back_to(redirect_uri, &out)
        }
        Err(e) => e.into_response(),
    }
}

/// A PKCE verifier or S256 challenge: 43–128 unreserved characters (RFC 7636).
fn valid_pkce_value(value: &str) -> bool {
    (43..=128).contains(&value.len())
        && value.bytes().all(|b| b.is_ascii_alphanumeric() || b"-._~".contains(&b))
}

fn pkce_matches(verifier: &str, challenge: &str) -> bool {
    valid_pkce_value(verifier)
        && URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes())) == challenge
}

// --- The sign-in form --------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct SigninForm {
    username: String,
    password: String,
    #[serde(rename = "continue", default)]
    next: String,
}

/// The web client's sign-in page posts here. On success the browser is signed in at the
/// provider and goes on to `continue` (an `/oauth/authorize` URL); on failure it goes back to
/// the page with the error.
async fn signin(
    State(state): State<AppState>,
    headers: HeaderMap,
    client_ip: Option<Extension<ClientIp>>,
    Form(form): Form<SigninForm>,
) -> Response {
    // A form on another site must not sign a browser in here (login CSRF).
    if let Some(origin) = headers.get(ORIGIN).and_then(|o| o.to_str().ok())
        && origin != "null"
        && !host(&headers).is_some_and(|h| {
            url::Url::parse(origin)
                .ok()
                .and_then(|u| u.host_str().map(str::to_owned))
                .is_some_and(|o| h.split(':').next().is_some_and(|hh| hh.eq_ignore_ascii_case(&o)))
        })
    {
        return (StatusCode::FORBIDDEN, "Sign-in refused: the form came from another site")
            .into_response();
    }
    let next = if form.next.starts_with("/oauth/authorize?") { form.next.as_str() } else { "/" };
    let client = client_ip.and_then(|Extension(c)| c.0);
    let creds = Credentials { username: form.username.clone(), password: form.password };
    let user = match auth::check_password(&state, &creds, client).await {
        Ok(user) => user,
        Err(e) if e.status == StatusCode::INTERNAL_SERVER_ERROR => return e.into_response(),
        Err(e) => {
            let page = url::form_urlencoded::Serializer::new(String::new())
                .append_pair("continue", next)
                .append_pair("username", &form.username)
                .append_pair("error", &e.message)
                .finish();
            return redirect(&format!("{LOGIN_PAGE}?{page}"));
        }
    };
    match auth::start_session(&state, user.id).await {
        Ok(cookie) => {
            let mut response = redirect(next);
            response.headers_mut().insert(SET_COOKIE, cookie);
            response
        }
        Err(e) => e.into_response(),
    }
}

// --- Token endpoint ----------------------------------------------------------------------------

/// An OAuth error answer (RFC 6749 §5.2).
#[derive(Debug)]
struct OAuthError {
    status: StatusCode,
    error: &'static str,
    description: String,
}

impl OAuthError {
    fn new(error: &'static str, description: impl Into<String>) -> Self {
        Self { status: StatusCode::BAD_REQUEST, error, description: description.into() }
    }
}

impl From<ApiError> for OAuthError {
    fn from(e: ApiError) -> Self {
        let (status, error) = match e.status {
            StatusCode::UNAUTHORIZED => (StatusCode::BAD_REQUEST, "invalid_grant"),
            StatusCode::TOO_MANY_REQUESTS => (StatusCode::TOO_MANY_REQUESTS, "too_many_requests"),
            StatusCode::BAD_REQUEST => (StatusCode::BAD_REQUEST, "invalid_request"),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, "server_error"),
        };
        Self { status, error, description: e.message }
    }
}

impl IntoResponse for OAuthError {
    fn into_response(self) -> Response {
        let body = Json(json!({"error": self.error, "error_description": self.description}));
        (self.status, [(CACHE_CONTROL, "no-store"), (PRAGMA, "no-cache")], body).into_response()
    }
}

#[derive(Debug, Deserialize)]
struct TokenForm {
    grant_type: Option<String>,
    client_id: Option<String>,
    code: Option<String>,
    redirect_uri: Option<String>,
    code_verifier: Option<String>,
    refresh_token: Option<String>,
    username: Option<String>,
    password: Option<String>,
    scope: Option<String>,
}

fn lifetimes(state: &AppState) -> Lifetimes {
    Lifetimes {
        access_minutes: state.config.access_token_minutes,
        refresh_days: state.config.session_days,
    }
}

async fn token(
    State(state): State<AppState>,
    headers: HeaderMap,
    client_ip: Option<Extension<ClientIp>>,
    Form(form): Form<TokenForm>,
) -> Response {
    match exchange(&state, &headers, client_ip.and_then(|Extension(c)| c.0), form).await {
        Ok(body) => {
            (StatusCode::OK, [(CACHE_CONTROL, "no-store"), (PRAGMA, "no-cache")], Json(body))
                .into_response()
        }
        Err(e) => e.into_response(),
    }
}

async fn exchange(
    state: &AppState,
    headers: &HeaderMap,
    address: Option<std::net::IpAddr>,
    form: TokenForm,
) -> Result<Value, OAuthError> {
    let Some(client) = client(form.client_id.as_deref()) else {
        return Err(OAuthError {
            status: StatusCode::UNAUTHORIZED,
            error: "invalid_client",
            description: "unknown client_id".into(),
        });
    };
    let lifetimes = lifetimes(state);
    let (grant, issued, nonce) = match form.grant_type.as_deref() {
        Some("authorization_code") if client.code => {
            let missing =
                || OAuthError::new("invalid_request", "code and code_verifier are needed");
            let code = form.code.as_deref().ok_or_else(missing)?;
            let verifier = form.code_verifier.as_deref().ok_or_else(missing)?;
            let invalid = || OAuthError::new("invalid_grant", "the code is invalid or expired");
            let found = tokens::take_code(&state.db, code).await?.ok_or_else(invalid)?;
            if found.client_id != client.id
                || form.redirect_uri.as_deref() != Some(found.redirect_uri.as_str())
                || !pkce_matches(verifier, &found.code_challenge)
            {
                return Err(invalid());
            }
            let user: User = sqlx::query_as("SELECT id, username, role FROM users WHERE id = ?")
                .bind(found.user_id)
                .fetch_optional(&state.db)
                .await
                .map_err(ApiError::from)?
                .ok_or_else(invalid)?;
            let (grant, issued) = tokens::new_grant(
                &state.db,
                client.id,
                &user,
                &found.scope,
                found.auth_time,
                lifetimes,
            )
            .await?;
            (grant, issued, found.nonce)
        }
        Some("password") if client.password => {
            let (Some(username), Some(password)) = (form.username, form.password) else {
                return Err(OAuthError::new("invalid_request", "username and password are needed"));
            };
            let user =
                auth::check_password(state, &Credentials { username, password }, address).await?;
            let (grant, issued) = tokens::new_grant(
                &state.db,
                client.id,
                &user,
                &granted_scope(form.scope.as_deref()),
                now(),
                lifetimes,
            )
            .await?;
            (grant, issued, None)
        }
        Some("refresh_token") => {
            let token = form
                .refresh_token
                .as_deref()
                .ok_or_else(|| OAuthError::new("invalid_request", "refresh_token is needed"))?;
            match tokens::refresh(&state.db, client.id, token, lifetimes).await? {
                Ok((grant, issued)) => (grant, issued, None),
                Err(RefreshRefused::Invalid) => {
                    return Err(OAuthError::new(
                        "invalid_grant",
                        "the sign-in has expired or was ended: sign in again",
                    ));
                }
                Err(RefreshRefused::Replayed) => {
                    return Err(OAuthError::new(
                        "invalid_grant",
                        "this refresh token was already used, so the sign-in was ended: sign \
                         in again",
                    ));
                }
            }
        }
        Some("authorization_code" | "password") => {
            return Err(OAuthError::new(
                "unauthorized_client",
                format!("{} may not use this grant", client.id),
            ));
        }
        _ => return Err(OAuthError::new("unsupported_grant_type", "unsupported grant_type")),
    };
    Ok(token_response(state, headers, &grant, &issued, nonce.as_deref()))
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

fn token_response(
    state: &AppState,
    headers: &HeaderMap,
    grant: &Grant,
    issued: &Issued,
    nonce: Option<&str>,
) -> Value {
    let mut body = json!({
        "access_token": issued.access,
        "token_type": "Bearer",
        "expires_in": issued.expires_in,
        "refresh_token": issued.refresh,
        "scope": grant.scope,
    });
    if grant.scope.split(' ').any(|s| s == "openid") {
        let iat = now();
        let mut claims = json!({
            "iss": issuer(state, headers),
            "sub": grant.user_id.to_string(),
            "aud": grant.client_id,
            "iat": iat,
            "exp": iat + i64::try_from(issued.expires_in).unwrap_or(3600),
            "auth_time": grant.auth_time,
            "name": grant.username,
            "preferred_username": grant.username,
            "role": grant.role.as_str(),
        });
        if let Some(nonce) = nonce {
            claims["nonce"] = json!(nonce);
        }
        body["id_token"] = json!(state.token_keys.sign(&claims));
    }
    body
}

// --- Revocation, userinfo, end session ---------------------------------------------------------

#[derive(Debug, Deserialize)]
struct RevokeForm {
    token: Option<String>,
    client_id: Option<String>,
}

/// RFC 7009: revoking either of a sign-in's tokens ends that sign-in. The answer is 200 whether
/// or not the token was known.
async fn revoke(State(state): State<AppState>, Form(form): Form<RevokeForm>) -> Response {
    let Some(client) = client(form.client_id.as_deref()) else {
        return OAuthError {
            status: StatusCode::UNAUTHORIZED,
            error: "invalid_client",
            description: "unknown client_id".into(),
        }
        .into_response();
    };
    if let Some(token) = form.token.as_deref().filter(|t| !t.is_empty()) {
        match tokens::revoke(&state.db, client.id, token).await {
            Ok(true) => tracing::info!(client = client.id, "signed out"),
            Ok(false) => {}
            Err(e) => return e.into_response(),
        }
    }
    (StatusCode::OK, [(CACHE_CONTROL, "no-store")]).into_response()
}

async fn userinfo(State(state): State<AppState>, headers: HeaderMap) -> Response {
    match tokens::user_for_access_token(&state.db, bearer(&headers).as_deref()).await {
        Ok(Some(user)) => Json(json!({
            "sub": user.id.to_string(),
            "name": user.username,
            "preferred_username": user.username,
            "role": user.role.as_str(),
        }))
        .into_response(),
        Ok(None) => {
            (StatusCode::UNAUTHORIZED, [("www-authenticate", "Bearer error=\"invalid_token\"")])
                .into_response()
        }
        Err(e) => e.into_response(),
    }
}

/// RP-initiated logout: ends the browser's sign-in at the provider, then returns to
/// `post_logout_redirect_uri` if it is this server's home page (else to `/`). The app revokes
/// its own tokens.
async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<BTreeMap<String, String>>,
) -> Response {
    let cookie = match auth::end_session(&state, &headers).await {
        Ok(cookie) => cookie,
        Err(e) => return e.into_response(),
    };
    let mut response = match params.get("post_logout_redirect_uri") {
        Some(uri) if on_this_server(&state, &headers, uri, "/") => match params.get("state") {
            Some(st) => back_to(uri, &[("state", st)]),
            None => redirect(uri),
        },
        _ => redirect("/"),
    };
    response.headers_mut().insert(SET_COOKIE, cookie);
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_pkce() {
        // RFC 7636, appendix B.
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let challenge = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
        assert!(pkce_matches(verifier, challenge));
        assert!(!pkce_matches("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXX", challenge));
        assert!(!pkce_matches("short", challenge));
        assert!(!valid_pkce_value(&"a".repeat(129)));
        assert!(!valid_pkce_value(&format!("{}+", "a".repeat(43))));
    }

    #[test]
    fn grants_only_known_scopes() {
        assert_eq!(granted_scope(Some("offline_access email openid")), "openid offline_access");
        assert_eq!(granted_scope(None), "");
    }

    #[test]
    fn reads_bearer_tokens() {
        let mut headers = HeaderMap::new();
        assert_eq!(bearer(&headers), None);
        headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer abc"));
        assert_eq!(bearer(&headers).as_deref(), Some("abc"));
        headers.insert(AUTHORIZATION, HeaderValue::from_static("bearer  xyz "));
        assert_eq!(bearer(&headers).as_deref(), Some("xyz"));
        headers.insert(AUTHORIZATION, HeaderValue::from_static("Basic abc"));
        assert_eq!(bearer(&headers), None);
    }

    #[test]
    fn knows_its_clients() {
        assert_eq!(client(Some("sovemu-web")), Some(&WEB));
        assert_eq!(client(Some("sovemu-native")), Some(&NATIVE));
        assert_eq!(client(Some("other")), None);
    }
}
