//! The sign-in provider: oauth-as's authorization server, and what SovEmu adds around it.
//!
//! oauth-as runs the OAuth 2.1 protocol: the authorization code flow (PKCE required, S256 only,
//! exact redirect URIs), the device authorization grant (RFC 8628) and its verification page,
//! refresh rotation with reuse detection, revocation, and the RFC 8414 metadata. This module adds:
//!
//! - **The front of the interactive endpoints.** oauth-as has no sessions by design: it asks the
//!   host who the user is, synchronously. A layer in front of `/oauth/authorize` and `/device`
//!   reads the browser's sign-in cookie, sends a signed-out browser to the sign-in page, and
//!   otherwise names the user to oauth-as in a header only this layer can set.
//! - **OpenID Connect over it**: `/.well-known/openid-configuration` (oauth-as's metadata plus the
//!   OIDC fields), an ES256 ID token added to the token endpoint's answer when `openid` was
//!   granted, `/oauth/jwks`, `/oauth/userinfo`, and `/oauth/logout` (end session).
//! - **The sign-in form's target**, `POST /oauth/signin`.
//! - **The password fallback** for the apps, `POST /oauth/password` (behind
//!   `SovEmu__Authentication__PasswordSignIn`): OAuth 2.1 has no password grant, so this checks the password,
//!   then runs an ordinary code exchange with oauth-as on the app's behalf, so its tokens are
//!   oauth-as's own and refresh and revoke exactly like the others.
//!
//! oauth-as fixes one issuer per server, so there is one server per address people reach SovEmu
//! at (`SovEmu__Http__PublicUrl`), all sharing one store; a request is answered by the server for its
//! `Host`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::extract::{Form, Request, State};
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, HOST, LOCATION, ORIGIN, PRAGMA, SET_COOKIE};
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use oauth_as::http::{ApprovalDecision, AuthorizationService, ServiceBuilder};
use oauth_as::rate_limit::{FixedWindowRateLimiter, RateLimitConfig};
use oauth_as::scope::ScopeSet;
use oauth_as::server::{
    AuthorizationServer, ServerConfig, SystemClock, TokenRequest, UserApproval,
};
use oauth_as::{AuthorizationRequest, Client, ClientAuth, ClientId, GrantType};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::accounts::Credentials;
use crate::store::SqliteStorage;
use crate::{ClientIp, Identity, sessions};

/// The web client's id on an install from before ids were per install: such an install keeps
/// it, so its browsers stay signed in.
pub const LEGACY_WEB_CLIENT: &str = "sovemu-web";
/// The apps' id from before, kept the same way as the device client's.
pub const LEGACY_DEVICE_CLIENT: &str = "sovemu-native";

/// The clients' registered names, by kind. A registration with one of these names (or one an
/// older server gave the same kind) that is not this server's id of that kind is a replaced one,
/// removed with its sign-ins on start.
const WEB_NAME: &str = "SovEmu (web)";
const DEVICE_NAME: &str = "SovEmu (devices)";
const MOBILE_NAME: &str = "SovEmu (phones and tablets)";
/// What the apps' client was called when one client served them all.
const LEGACY_DEVICE_NAME: &str = "SovEmu (Android, iOS)";
const OUR_NAMES: [&str; 4] = [WEB_NAME, DEVICE_NAME, MOBILE_NAME, LEGACY_DEVICE_NAME];

/// Where the mobile client's sign-in comes back to: the Android and iOS apps' private-use scheme
/// (RFC 8252 section 7.1), named for their application id and bundle id, `media.sovereign.sovemu`.
/// Any app can claim a scheme, so PKCE is what makes a code read off it useless to another.
pub(crate) const MOBILE_REDIRECT: &str = "media.sovereign.sovemu:/oauth2redirect";

/// Where the web client receives its code: a page on this server.
const CALLBACK_PATH: &str = "/auth/callback/";
/// The web client's sign-in page, which posts to `/oauth/signin`.
const LOGIN_PAGE: &str = "/login/";
/// The redirect URI the password fallback's internal code exchange uses, the device client's
/// only one. It is never followed (nothing redirects there): oauth-as needs a registered one for
/// the exchange to be exact.
const PASSWORD_REDIRECT: &str = "media.sovereign.sovemu:/oauth/password";

const SCOPES: [&str; 3] = ["openid", "profile", "offline_access"];

/// The header the front layer names the signed-in user in. Removed from every incoming request
/// first, so only this layer can set it.
const SUBJECT: HeaderName = HeaderName::from_static("x-sovemu-subject");

/// An authorization code's `nonce`, kept from the authorization request until the code is
/// exchanged, for the ID token (oauth-as is not OIDC and does not carry it).
const NONCE_SECONDS: u64 = 120;

type Server = AuthorizationServer<SqliteStorage>;

/// One issuer: oauth-as's server for it and the HTTP service over that server.
pub(crate) struct Issuer {
    /// `http://192.168.1.10:8080`, no trailing slash.
    pub(crate) url: String,
    /// What a request's `Host` must be for this issuer, lower-cased.
    authority: String,
    pub(crate) server: Arc<Server>,
    service: AuthorizationService<SqliteStorage, SystemClock>,
}

/// Session-bound CSRF tokens for the device verification form, by session hash.
type CsrfTokens = Arc<Mutex<HashMap<String, String>>>;

pub(crate) struct Provider {
    pub(crate) issuers: Vec<Issuer>,
    clients: crate::ClientIds,
    nonces: Mutex<HashMap<String, (String, Instant)>>,
}

impl std::fmt::Debug for Provider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let urls: Vec<&str> = self.issuers.iter().map(|i| i.url.as_str()).collect();
        f.debug_struct("Provider").field("issuers", &urls).finish_non_exhaustive()
    }
}

/// How long tokens last.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Lifetimes {
    pub(crate) access: Duration,
    /// A sign-in's whole life, from signing in (refreshing does not extend it). `None`: until it
    /// is revoked.
    pub(crate) sign_in: Option<Duration>,
    /// How long a just-used refresh token is honoured again.
    pub(crate) refresh_retry: Duration,
}

fn authority_of(url: &str) -> crate::Result<String> {
    let bad = |why: String| crate::Error::Config(format!("SovEmu__Http__PublicUrl {url}: {why}"));
    let parsed = url::Url::parse(url).map_err(|e| bad(e.to_string()))?;
    let host = parsed.host_str().ok_or_else(|| bad("no host".into()))?;
    Ok(match parsed.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_owned(),
    }
    .to_ascii_lowercase())
}

fn scopes() -> ScopeSet {
    ScopeSet::from_tokens(SCOPES).expect("valid scope tokens")
}

impl Provider {
    pub(crate) async fn new(
        urls: &[String],
        clients: &crate::ClientIds,
        store: SqliteStorage,
        lifetimes: Lifetimes,
    ) -> crate::Result<Self> {
        if urls.is_empty() {
            return Err(crate::Error::Config(
                "no address to sign in at (SovEmu__Http__PublicUrl)".into(),
            ));
        }
        let csrf: CsrfTokens = Arc::default();
        let mut issuers = Vec::new();
        for url in urls {
            let url = url.trim_end_matches('/').to_owned();
            let authority = authority_of(&url)?;
            let mut config = ServerConfig::new(url.clone(), format!("{url}/device"));
            config.authorization_endpoint = Some(format!("{url}/oauth/authorize"));
            config.token_endpoint = Some(format!("{url}/oauth/token"));
            config.revocation_endpoint = Some(format!("{url}/oauth/revoke"));
            config.introspection_endpoint = Some(format!("{url}/oauth/introspect"));
            config.device_authorization_endpoint =
                Some(format!("{url}/oauth/device_authorization"));
            config.scopes_supported = Some(SCOPES.map(str::to_owned).to_vec());
            config.access_token_ttl = lifetimes.access;
            config.refresh_token_ttl = lifetimes.sign_in;
            // Two tabs (or an app's two requests) refreshing the same token at once both get
            // the new one rather than tripping reuse detection.
            config.refresh_retry_window = lifetimes.refresh_retry;
            let server =
                Arc::new(AuthorizationServer::new(config, store.clone()).with_rate_limiter(
                    Box::new(FixedWindowRateLimiter::with_config(RateLimitConfig::default())),
                ));
            let (issue, consume) = (csrf.clone(), csrf.clone());
            let service = ServiceBuilder::new(server.clone())
                .with_subject_resolver(|headers| {
                    headers.get(SUBJECT).and_then(|v| v.to_str().ok()).map(str::to_owned)
                })
                // Both clients are SovEmu's own (dynamic registration is off), and a code only
                // ever goes to this server's own callback page or the apps' exchange, behind
                // PKCE: signing in to SovEmu is the consent.
                .with_approval_resolver(|_| ApprovalDecision::Approve)
                .with_csrf_tokens(
                    move |headers| {
                        let session = sessions::cookie_token(headers)?;
                        let token = sessions::random_token().ok()?;
                        let mut map = issue.lock().unwrap_or_else(|e| e.into_inner());
                        if map.len() > 10_000 {
                            map.clear();
                        }
                        map.insert(sessions::token_hash(&session), token.clone());
                        Some(token)
                    },
                    move |headers| {
                        let session = sessions::cookie_token(headers)?;
                        let mut map = consume.lock().unwrap_or_else(|e| e.into_inner());
                        map.remove(&sessions::token_hash(&session))
                    },
                )
                .build()
                .map_err(|e| {
                    crate::Error::Config(format!("the sign-in service for {url}: {e:?}"))
                })?;
            issuers.push(Issuer { url, authority, server, service });
        }
        let provider = Self { issuers, clients: clients.clone(), nonces: Mutex::default() };
        provider.register_clients().await?;
        provider.remove_replaced_clients(lifetimes).await?;
        Ok(provider)
    }

    /// The three clients, written on every start so a change reaches existing installs. All are
    /// public: a browser, a TV and a phone keep no secret.
    async fn register_clients(&self) -> crate::Result<()> {
        let server = &self.issuers[0].server;
        let ids = &self.clients;
        let callbacks = self.issuers.iter().map(|i| format!("{}{CALLBACK_PATH}", i.url)).collect();
        let client = |id: &str, grant_types, redirect_uris, name: &str| Client {
            client_id: ClientId::new(id),
            auth: ClientAuth::Public,
            grant_types,
            redirect_uris,
            allowed_scopes: scopes(),
            default_scopes: scopes(),
            name: Some(name.into()),
            registration: None,
        };
        for client in [
            // No password grant: the browser's password goes only to the sign-in form.
            client(
                &ids.web,
                vec![GrantType::AuthorizationCode, GrantType::RefreshToken],
                callbacks,
                WEB_NAME,
            ),
            // The device grant, and the code flow only for the password fallback's own
            // exchange (to a redirect URI nothing is ever sent to).
            client(
                &ids.device,
                vec![GrantType::DeviceCode, GrantType::RefreshToken, GrantType::AuthorizationCode],
                vec![PASSWORD_REDIRECT.into()],
                DEVICE_NAME,
            ),
            // No password and no device grant: a phone signing in through the browser hands
            // the server's own sign-in page the password, never the app.
            client(
                &ids.mobile,
                vec![GrantType::AuthorizationCode, GrantType::RefreshToken],
                vec![MOBILE_REDIRECT.into()],
                MOBILE_NAME,
            ),
        ] {
            server
                .register_client(client)
                .await
                .map_err(|e| crate::Error::internal(format!("registering a client: {e:?}")))?;
        }
        Ok(())
    }

    /// Removes every registration of SovEmu's own that is none of this server's three ids (an
    /// id replaced in `server.json`), with every sign-in under it: they would otherwise keep
    /// signing in as a client that no longer exists. oauth-as's own deletion, so its tokens go
    /// with it and a barrier refuses any still in flight.
    async fn remove_replaced_clients(&self, lifetimes: Lifetimes) -> crate::Result<()> {
        let store = self.issuers[0].server.store();
        let ids = &self.clients;
        let current = [ids.web.as_str(), ids.device.as_str(), ids.mobile.as_str()];
        let now = std::time::SystemTime::now();
        let longest = lifetimes
            .sign_in
            .unwrap_or(Duration::from_secs(100 * 365 * 86_400))
            .max(lifetimes.access);
        let window = oauth_as::store::RevocationWindow { recorded_at: now, until: now + longest };
        for client in store.clients().await.map_err(crate::Error::internal)? {
            let ours = client.name.as_deref().is_some_and(|n| OUR_NAMES.contains(&n));
            if !ours || current.contains(&client.client_id.as_str()) {
                continue;
            }
            oauth_as::store::Storage::delete_client(store, &client.client_id, window)
                .await
                .map_err(crate::Error::internal)?;
            tracing::warn!(
                client = %client.client_id.as_str(),
                name = client.name.as_deref().unwrap_or_default(),
                "removed a replaced client and its sign-ins"
            );
        }
        Ok(())
    }

    /// The issuer a request came to, by its `Host`.
    pub(crate) fn issuer(&self, headers: &HeaderMap) -> Option<&Issuer> {
        let host = headers.get(HOST)?.to_str().ok()?.to_ascii_lowercase();
        self.issuers.iter().find(|i| i.authority == host)
    }

    fn remember_nonce(&self, code: &str, nonce: String) {
        let now = Instant::now();
        let mut map = self.nonces.lock().unwrap_or_else(|e| e.into_inner());
        map.retain(|_, (_, at)| now.duration_since(*at).as_secs() < NONCE_SECONDS);
        map.insert(sessions::token_hash(code), (nonce, now));
    }

    fn take_nonce(&self, code: &str) -> Option<String> {
        let mut map = self.nonces.lock().unwrap_or_else(|e| e.into_inner());
        map.remove(&sessions::token_hash(code))
            .filter(|(_, at)| at.elapsed().as_secs() < NONCE_SECONDS)
            .map(|(nonce, _)| nonce)
    }
}

// --- Routes ------------------------------------------------------------------------------------

pub(crate) fn router(identity: Identity) -> Router {
    Router::new()
        .route("/.well-known/openid-configuration", get(openid_configuration))
        .route("/oauth/jwks", get(jwks))
        .route("/oauth/userinfo", get(userinfo).post(userinfo))
        .route("/oauth/logout", get(logout))
        .route("/oauth/signin", post(signin))
        .route("/oauth/password", post(password))
        .route("/oauth/token", post(token))
        // Everything else oauth-as serves: authorize, revoke, introspect, device authorization,
        // its metadata and the device verification page.
        .route("/oauth/{*rest}", get(forward).post(forward))
        .route("/device", get(forward).post(forward))
        .route("/.well-known/oauth-authorization-server", get(forward))
        .with_state(identity)
}

fn wrong_address() -> Response {
    (
        StatusCode::BAD_REQUEST,
        [(CACHE_CONTROL, "no-store")],
        "Sign-in is not available at this address: add it to SovEmu__Http__PublicUrl.",
    )
        .into_response()
}

fn redirect(to: &str) -> Response {
    match HeaderValue::from_str(to) {
        Ok(location) => (StatusCode::SEE_OTHER, [(LOCATION, location)]).into_response(),
        Err(_) => (StatusCode::BAD_REQUEST, "bad redirect").into_response(),
    }
}

/// The sign-in page, coming back to `next` afterwards.
fn to_login(next: &str, extra: &[(&str, &str)]) -> Response {
    let mut query = url::form_urlencoded::Serializer::new(String::new());
    query.append_pair("continue", next);
    query.extend_pairs(extra);
    redirect(&format!("{LOGIN_PAGE}?{}", query.finish()))
}

/// Where the sign-in form may continue to: the two interactive endpoints, on this server.
fn safe_continue(next: &str) -> &str {
    if next.starts_with("/oauth/authorize?") || next == "/device" || next.starts_with("/device?") {
        next
    } else {
        "/"
    }
}

/// oauth-as's answer as axum's.
fn from_oauth(response: oauth_as::http::Response) -> Response {
    let (parts, body) = response.into_parts();
    Response::from_parts(parts, Body::from(body.into_bytes()))
}

/// Hands a request to the issuer's oauth-as service. Interactive endpoints first need the
/// browser's sign-in: signed out, a page request goes to the sign-in page; signed in, the user is
/// named to oauth-as. For the authorization endpoint, a `nonce` is kept for the ID token.
async fn forward(State(identity): State<Identity>, request: Request) -> Response {
    let provider = &identity.inner.provider;
    let Some(issuer) = provider.issuer(request.headers()) else { return wrong_address() };
    let (mut parts, body) = request.into_parts();
    parts.headers.remove(SUBJECT);
    let path = parts.uri.path().to_owned();
    let interactive = path == "/oauth/authorize" || path == "/device";
    if interactive {
        let signed_in = match sessions::user(&identity.inner.db, &parts.headers).await {
            Ok(user) => user,
            Err(e) => return crate::web_error(&e),
        };
        let prompt_login = parts.uri.query().is_some_and(|q| {
            url::form_urlencoded::parse(q.as_bytes())
                .any(|(k, v)| k == "prompt" && v.split(' ').any(|p| p == "login"))
        });
        match signed_in.filter(|_| !prompt_login) {
            Some((user, _)) => {
                let id = HeaderValue::from_str(&user.id.to_string()).expect("digits");
                parts.headers.insert(SUBJECT, id);
            }
            None if parts.method == Method::GET => {
                // Back to the same request afterwards, without `prompt=login`, or it would ask
                // forever.
                let pairs = url::form_urlencoded::parse(parts.uri.query().unwrap_or("").as_bytes())
                    .filter(|(k, _)| k != "prompt")
                    .map(|(k, v)| (k.into_owned(), v.into_owned()))
                    .collect::<Vec<_>>();
                let query = url::form_urlencoded::Serializer::new(String::new())
                    .extend_pairs(pairs)
                    .finish();
                let next = if query.is_empty() { path } else { format!("{path}?{query}") };
                return to_login(&next, &[]);
            }
            None => return (StatusCode::FORBIDDEN, "sign in first").into_response(),
        }
    }
    let nonce = (path == "/oauth/authorize")
        .then(|| {
            url::form_urlencoded::parse(parts.uri.query().unwrap_or("").as_bytes())
                .find(|(k, _)| k == "nonce")
                .map(|(_, v)| v.into_owned())
        })
        .flatten();
    let response = issuer.service.handle(axum::http::Request::from_parts(parts, body)).await;
    if let Some(nonce) = nonce
        && let Some(code) = code_in(response.headers())
    {
        provider.remember_nonce(&code, nonce);
    }
    from_oauth(response)
}

/// The `code` of an authorization response's redirect.
fn code_in(headers: &HeaderMap) -> Option<String> {
    let location = headers.get(LOCATION)?.to_str().ok()?;
    let url = url::Url::parse(location).ok()?;
    url.query_pairs().find(|(k, _)| k == "code").map(|(_, v)| v.into_owned())
}

/// The token endpoint: oauth-as's, plus an ID token when `openid` was granted.
async fn token(State(identity): State<Identity>, request: Request) -> Response {
    let provider = &identity.inner.provider;
    let Some(issuer) = provider.issuer(request.headers()) else { return wrong_address() };
    let (mut parts, body) = request.into_parts();
    parts.headers.remove(SUBJECT);
    let Ok(bytes) = axum::body::to_bytes(body, 64 * 1024).await else {
        return (StatusCode::PAYLOAD_TOO_LARGE, "too large").into_response();
    };
    let code =
        url::form_urlencoded::parse(&bytes).find(|(k, _)| k == "code").map(|(_, v)| v.into_owned());
    let response =
        issuer.service.handle(axum::http::Request::from_parts(parts, Body::from(bytes))).await;
    if response.status() != StatusCode::OK {
        return from_oauth(response);
    }
    let (mut parts, body) = response.into_parts();
    let Ok(mut answer) = serde_json::from_slice::<Value>(&body.into_bytes()) else {
        return (StatusCode::INTERNAL_SERVER_ERROR, "unreadable token answer").into_response();
    };
    let nonce = code.as_deref().and_then(|c| provider.take_nonce(c));
    if let Err(e) = add_id_token(&identity, issuer, &mut answer, nonce.as_deref()).await {
        return crate::web_error(&e);
    }
    parts.headers.remove(axum::http::header::CONTENT_LENGTH);
    Response::from_parts(parts, Body::from(answer.to_string()))
}

/// Adds an ID token to a token answer whose scope includes `openid`.
async fn add_id_token(
    identity: &Identity,
    issuer: &Issuer,
    answer: &mut Value,
    nonce: Option<&str>,
) -> crate::Result<()> {
    let openid = answer["scope"].as_str().is_some_and(|s| s.split(' ').any(|t| t == "openid"));
    let Some(access) = answer["access_token"].as_str().filter(|_| openid) else { return Ok(()) };
    let Some(token) = issuer.server.introspect(access).await.map_err(crate::Error::internal)?
    else {
        return Ok(());
    };
    let Some(user) = identity.user_of(token.subject.as_deref()).await? else { return Ok(()) };
    let iat = now();
    let mut claims = json!({
        "iss": issuer.url,
        "sub": user.id.to_string(),
        "aud": token.client_id.as_str(),
        "iat": iat,
        "exp": iat + answer["expires_in"].as_i64().unwrap_or(3600),
        "name": user.username,
        "preferred_username": user.username,
        "role": user.role.as_str(),
    });
    if let Some(nonce) = nonce {
        claims["nonce"] = json!(nonce);
    }
    answer["id_token"] = json!(identity.inner.key.sign(&claims));
    Ok(())
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// OpenID Connect discovery: oauth-as's RFC 8414 metadata, plus what OIDC adds.
async fn openid_configuration(State(identity): State<Identity>, headers: HeaderMap) -> Response {
    let Some(issuer) = identity.inner.provider.issuer(&headers) else { return wrong_address() };
    let mut doc = serde_json::to_value(issuer.server.metadata()).unwrap_or_else(|_| json!({}));
    let url = &issuer.url;
    doc["userinfo_endpoint"] = json!(format!("{url}/oauth/userinfo"));
    doc["jwks_uri"] = json!(format!("{url}/oauth/jwks"));
    doc["end_session_endpoint"] = json!(format!("{url}/oauth/logout"));
    doc["subject_types_supported"] = json!(["public"]);
    doc["id_token_signing_alg_values_supported"] = json!(["ES256"]);
    doc["claims_supported"] =
        json!(["sub", "iss", "aud", "exp", "iat", "nonce", "name", "preferred_username", "role"]);
    Json(doc).into_response()
}

async fn jwks(State(identity): State<Identity>) -> Json<Value> {
    Json(identity.inner.key.jwks())
}

async fn userinfo(State(identity): State<Identity>, headers: HeaderMap) -> Response {
    match identity.user_for_request(&headers).await {
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
        Err(e) => crate::web_error(&e),
    }
}

/// RP-initiated logout: ends the browser's sign-in, then returns to `post_logout_redirect_uri` if
/// it is this server's home page (else to `/`). The app revokes its own tokens.
async fn logout(
    State(identity): State<Identity>,
    headers: HeaderMap,
    axum::extract::Query(params): axum::extract::Query<HashMap<String, String>>,
) -> Response {
    let cookie = match sessions::end(&identity.inner.db, &headers).await {
        Ok(cookie) => cookie,
        Err(e) => return crate::web_error(&e),
    };
    let home = identity.inner.provider.issuer(&headers).map(|i| format!("{}/", i.url));
    let to = match params.get("post_logout_redirect_uri") {
        Some(uri) if Some(uri) == home.as_ref() => match params.get("state") {
            Some(state) => {
                let mut url = url::Url::parse(uri).expect("an issuer URL parses");
                url.query_pairs_mut().append_pair("state", state);
                url.to_string()
            }
            None => uri.clone(),
        },
        _ => "/".into(),
    };
    let mut response = redirect(&to);
    response.headers_mut().insert(SET_COOKIE, cookie);
    response
}

#[derive(Debug, Deserialize)]
struct SigninForm {
    username: String,
    password: String,
    #[serde(rename = "continue", default)]
    next: String,
}

/// Whether a form post came from this server's own pages (login CSRF).
fn same_origin(headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(ORIGIN).and_then(|o| o.to_str().ok()) else { return true };
    let host = headers.get(HOST).and_then(|h| h.to_str().ok()).unwrap_or_default();
    authority_of(origin).is_ok_and(|o| o.eq_ignore_ascii_case(host))
}

/// The sign-in page posts here. On success the browser is signed in at the server and goes on to
/// `continue`; on failure it goes back to the page with the error.
async fn signin(
    State(identity): State<Identity>,
    headers: HeaderMap,
    client: Option<Extension<ClientIp>>,
    Form(form): Form<SigninForm>,
) -> Response {
    if !same_origin(&headers) {
        return (StatusCode::FORBIDDEN, "Sign-in refused: the form came from another site")
            .into_response();
    }
    let next = safe_continue(&form.next);
    let creds = Credentials { username: form.username.clone(), password: form.password };
    let address = client.and_then(|Extension(c)| c.0);
    let user = match identity.check_password(&creds, address).await {
        Ok(user) => user,
        Err(e @ crate::Error::Internal(_)) => return crate::web_error(&e),
        Err(e) => {
            return to_login(next, &[("username", &form.username), ("error", e.message())]);
        }
    };
    match identity.sign_in_browser(user.id).await {
        Ok(cookie) => {
            let mut response = redirect(next);
            response.headers_mut().insert(SET_COOKIE, cookie);
            response
        }
        Err(e) => crate::web_error(&e),
    }
}

/// An OAuth error answer (RFC 6749 §5.2).
fn oauth_error(status: StatusCode, error: &str, description: &str) -> Response {
    let body = Json(json!({"error": error, "error_description": description}));
    (status, [(CACHE_CONTROL, "no-store"), (PRAGMA, "no-cache")], body).into_response()
}

#[derive(Debug, Deserialize)]
struct PasswordForm {
    client_id: Option<String>,
    username: Option<String>,
    password: Option<String>,
    scope: Option<String>,
}

/// The apps' password fallback: a username and password in, the same token answer as
/// `/oauth/token` out. Off with `SovEmu__Authentication__PasswordSignIn=false`.
async fn password(
    State(identity): State<Identity>,
    headers: HeaderMap,
    client: Option<Extension<ClientIp>>,
    Form(form): Form<PasswordForm>,
) -> Response {
    let inner = &identity.inner;
    if !inner.config.password_sign_in {
        return oauth_error(
            StatusCode::BAD_REQUEST,
            "unsupported_grant_type",
            "signing in with a password is turned off here: sign in with a code instead",
        );
    }
    if form.client_id.as_deref() != Some(inner.config.clients.device.as_str()) {
        return oauth_error(
            StatusCode::UNAUTHORIZED,
            "invalid_client",
            "only the SovEmu apps sign in with a password, as the device client",
        );
    }
    let Some(issuer) = inner.provider.issuer(&headers) else { return wrong_address() };
    let (Some(username), Some(password)) = (form.username, form.password) else {
        return oauth_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "username and password are needed",
        );
    };
    let address = client.and_then(|Extension(c)| c.0);
    let user = match identity.check_password(&Credentials { username, password }, address).await {
        Ok(user) => user,
        Err(crate::Error::TooManyRequests(m)) => {
            return oauth_error(StatusCode::TOO_MANY_REQUESTS, "too_many_requests", &m);
        }
        Err(crate::Error::Unauthorized(m)) => {
            return oauth_error(StatusCode::BAD_REQUEST, "invalid_grant", &m);
        }
        Err(e) => return crate::web_error(&e),
    };
    let scope = form.scope.unwrap_or_else(|| SCOPES.join(" "));
    match exchange_for(&identity, issuer, user.id, &scope).await {
        Ok(answer) => (
            StatusCode::OK,
            [(CACHE_CONTROL, "no-store"), (PRAGMA, "no-cache")],
            [(CONTENT_TYPE, "application/json")],
            answer.to_string(),
        )
            .into_response(),
        Err(response) => *response,
    }
}

/// Mints the app's tokens through oauth-as: an authorization code for the signed-in user, then
/// the ordinary exchange, with a verifier made here.
async fn exchange_for(
    identity: &Identity,
    issuer: &Issuer,
    user_id: i64,
    scope: &str,
) -> Result<Value, Box<Response>> {
    let server = &issuer.server;
    let failed = |what: &str, e: &dyn std::fmt::Debug| {
        tracing::error!(error = ?e, step = what, "the password sign-in's code exchange failed");
        Box::new(oauth_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "server_error",
            "signing in failed",
        ))
    };
    let verifier = sessions::random_token().map_err(|e| failed("verifier", &e))? + "-sovemu";
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let mut request = AuthorizationRequest::default();
    request.response_type = Some("code".into());
    let device = identity.inner.config.clients.device.clone();
    request.client_id = Some(device.clone().into());
    request.redirect_uri = Some(PASSWORD_REDIRECT.into());
    request.scope = Some(scope.to_owned().into());
    request.code_challenge = Some(challenge.into());
    request.code_challenge_method = Some("S256".into());
    let validated = server.validate_authorization_request(&request).await.map_err(|e| {
        let status = e.http_status();
        tracing::warn!(error = ?e, status, "the apps' password sign-in was refused");
        Box::new(oauth_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "the sign-in request was refused",
        ))
    })?;
    let code = server
        .issue_authorization_code(UserApproval::granted(&validated, user_id.to_string()))
        .await
        .map_err(|e| failed("code", &e))?
        .code;
    let tokens = server
        .token(TokenRequest::AuthorizationCode {
            client_id: ClientId::new(device),
            client_secret: None,
            code,
            redirect_uri: Some(PASSWORD_REDIRECT.into()),
            code_verifier: Some(verifier),
        })
        .await
        .map_err(|e| failed("exchange", &e))?;
    let mut answer = serde_json::to_value(&tokens).map_err(|e| failed("answer", &e))?;
    add_id_token(identity, issuer, &mut answer, None)
        .await
        .map_err(|e| Box::new(crate::web_error(&e)))?;
    Ok(answer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sign_in_form_continues_only_to_this_servers_endpoints() {
        assert_eq!(safe_continue("/oauth/authorize?client_id=x"), "/oauth/authorize?client_id=x");
        assert_eq!(safe_continue("/device"), "/device");
        assert_eq!(safe_continue("/device?user_code=ABCD"), "/device?user_code=ABCD");
        assert_eq!(safe_continue("https://evil.test/"), "/");
        assert_eq!(safe_continue("//evil.test/oauth/authorize?"), "/");
        assert_eq!(safe_continue("/devicex"), "/");
    }

    #[test]
    fn issuers_are_matched_by_host() {
        assert_eq!(authority_of("http://192.168.1.10:8080").unwrap(), "192.168.1.10:8080");
        assert_eq!(authority_of("https://Games.Example.com").unwrap(), "games.example.com");
        assert!(authority_of("not a url").is_err());
    }

    #[test]
    fn form_posts_must_come_from_this_site() {
        let mut headers = HeaderMap::new();
        headers.insert(HOST, HeaderValue::from_static("sovemu.test"));
        assert!(same_origin(&headers), "no Origin: a plain form post from an old browser");
        headers.insert(ORIGIN, HeaderValue::from_static("http://sovemu.test"));
        assert!(same_origin(&headers));
        headers.insert(ORIGIN, HeaderValue::from_static("http://evil.test"));
        assert!(!same_origin(&headers));
    }
}
