//! The API (GraphQL) end to end through the router: accounts, the library and a real game
//! session, plus the plain-HTTP media and ops routes.
//!
//! The session test starts a real `sovemu-worker session`, with `SovEmu.TestCore` installed as
//! the "NES core" and an empty file as the ROM (the test core accepts any content).

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, OnceLock};

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use clap::Parser;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

use crate::{AppState, Config, build_state, router};

/// Builds the worker and the test core once; returns (worker binary, test core library).
fn binaries() -> &'static (PathBuf, PathBuf) {
    static BINS: OnceLock<(PathBuf, PathBuf)> = OnceLock::new();
    BINS.get_or_init(|| {
        let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
        let status = Command::new(cargo)
            .args(["build", "--quiet", "-p", "sovemu-worker", "-p", "sovemu-test-core"])
            .current_dir(&workspace)
            .status()
            .expect("run cargo");
        assert!(status.success(), "building the worker and test core failed");
        let target = std::env::var_os("CARGO_TARGET_DIR")
            .map_or_else(|| workspace.join("target"), PathBuf::from)
            .join("debug");
        (
            target.join(format!("sovemu-worker{}", std::env::consts::EXE_SUFFIX)),
            target.join(format!(
                "{}sovemu_test_core{}",
                std::env::consts::DLL_PREFIX,
                std::env::consts::DLL_SUFFIX
            )),
        )
    })
}

struct Harness {
    state: AppState,
    dir: PathBuf,
    /// The browser's sign-in at the provider (`/oauth/authorize` reads it; the API does not).
    cookie: Option<String>,
    /// The access token API calls carry, from `login`.
    token: Option<String>,
}

impl Harness {
    /// Two UDP ports of its own: tests run in parallel, and two harnesses' sessions must not
    /// bind the same ones.
    async fn new(name: &str, with_core: bool) -> Self {
        Self::new_with(name, with_core, Vec::new()).await
    }

    async fn new_with(name: &str, with_core: bool, extra: Vec<String>) -> Self {
        let dir = std::env::temp_dir().join(format!("sovemu-server-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for (path, bytes) in [
            ("roms/nes/Test Game (USA).nes", 64),
            // Artwork beside the ROMs, as Batocera keeps it: never read.
            ("roms/nes/images/Test Game (USA)-thumb.png", 2048),
            ("roms/gba/Other.gba", 32),
        ] {
            let p = dir.join(path);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, vec![0u8; bytes]).unwrap();
        }
        let cores = dir.join("cores");
        std::fs::create_dir_all(&cores).unwrap();
        let mut args: Vec<String> = vec![
            "sovemu-server".into(),
            "--data-dir".into(),
            dir.display().to_string(),
            "--cores-dir".into(),
            cores.display().to_string(),
            "--udp-bind".into(),
            "127.0.0.1".into(),
            "--udp-ports".into(),
            udp_ports(),
            // Sign-in answers at this address only; the OAuth helpers send it as `Host`.
            "--public-url".into(),
            "http://sovemu.test".into(),
            // Reuse detection at once, so a test can replay a refresh token.
            "--refresh-retry-seconds".into(),
            "0".into(),
        ];
        if with_core {
            let (worker, core) = binaries();
            let ext = core.extension().unwrap().to_string_lossy().into_owned();
            std::fs::copy(core, cores.join(format!("fceumm_libretro.{ext}"))).unwrap();
            args.extend(["--worker".into(), worker.display().to_string()]);
        }
        // No built-in decorations: drawing them takes seconds in a debug build. A test that
        // wants decorations gives its own set.
        args.extend(["--decorations-builtin".into(), dir.join("no-builtin").display().to_string()]);
        args.extend(extra);
        let state = build_state(Config::parse_from(args)).await.unwrap();
        crate::library::scan(&state.db, &state.config.roms_dir).await.unwrap();
        state.game_counts.refresh(&state.db).await.unwrap();
        Self { state, dir, cookie: None, token: None }
    }

    async fn call(&mut self, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
        let mut request = Request::builder().method(method).uri(uri);
        if let Some(cookie) = &self.cookie {
            request = request.header(header::COOKIE, cookie);
        }
        if let Some(token) = &self.token {
            request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        let request = match body {
            Some(b) => request
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(b.to_string())),
            None => request.body(Body::empty()),
        }
        .unwrap();
        let response = router(self.state.clone()).oneshot(request).await.unwrap();
        let status = response.status();
        if let Some(set) = response.headers().get(header::SET_COOKIE) {
            let pair = set.to_str().unwrap().split(';').next().unwrap().to_owned();
            self.cookie = Some(pair);
        }
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let value = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or(Value::Null)
        };
        (status, value)
    }

    /// GETs `uri` (a signed file URL); returns the status, the content type and the bytes.
    async fn fetch(&self, uri: &str) -> (StatusCode, String, Vec<u8>) {
        let request = Request::builder().uri(uri).body(Body::empty()).unwrap();
        let response = router(self.state.clone()).oneshot(request).await.unwrap();
        let status = response.status();
        let kind = response
            .headers()
            .get(header::CONTENT_TYPE)
            .map(|v| v.to_str().unwrap().to_owned())
            .unwrap_or_default();
        (status, kind, response.into_body().collect().await.unwrap().to_bytes().to_vec())
    }

    /// Runs a GraphQL operation; returns `data` and `errors` (null when there are none).
    async fn gql(&mut self, query: &str, variables: Value) -> (Value, Value) {
        let body = json!({"query": query, "variables": variables});
        let (status, response) = self.call("POST", "/graphql", Some(body)).await;
        assert_eq!(status, StatusCode::OK, "{response}");
        (response["data"].clone(), response["errors"].clone())
    }

    /// A GraphQL operation that must succeed: its `data`.
    async fn ok(&mut self, query: &str, variables: Value) -> Value {
        let (data, errors) = self.gql(query, variables).await;
        assert!(errors.is_null(), "{query}: {errors}");
        data
    }

    /// A GraphQL operation that must fail: its first error's `code` and message.
    async fn err(&mut self, query: &str, variables: Value) -> (String, String) {
        let (_, errors) = self.gql(query, variables).await;
        let first = &errors[0];
        assert!(!first.is_null(), "{query} succeeded");
        (
            first["extensions"]["code"].as_str().unwrap_or_default().to_owned(),
            first["message"].as_str().unwrap_or_default().to_owned(),
        )
    }

    /// First-run setup, then signs the admin in as an app would.
    async fn sign_up_admin(&mut self) {
        self.ok(SETUP, json!({"u": "admin", "p": "password123"})).await;
        self.login("admin", "password123").await.unwrap();
    }

    /// A GET of a file (a signed `/media/…` URL): status, the body's bytes, and headers.
    async fn get_raw(&self, uri: &str) -> (StatusCode, Vec<u8>, header::HeaderMap) {
        let request = Request::builder().method("GET").uri(uri).body(Body::empty()).unwrap();
        let response = router(self.state.clone()).oneshot(request).await.unwrap();
        let (status, headers) = (response.status(), response.headers().clone());
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (status, bytes.to_vec(), headers)
    }

    /// A form post (the OAuth endpoints): status, JSON body (null if none) and headers.
    async fn form(
        &self,
        uri: &str,
        pairs: &[(&str, &str)],
    ) -> (StatusCode, Value, header::HeaderMap) {
        let body =
            url::form_urlencoded::Serializer::new(String::new()).extend_pairs(pairs).finish();
        let mut request = Request::builder()
            .method("POST")
            .uri(uri)
            .header(header::HOST, "sovemu.test")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded");
        if let Some(cookie) = &self.cookie {
            request = request.header(header::COOKIE, cookie);
        }
        let response = router(self.state.clone())
            .oneshot(request.body(Body::from(body)).unwrap())
            .await
            .unwrap();
        let (status, headers) = (response.status(), response.headers().clone());
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null), headers)
    }

    /// Signs in as the apps do with a password and keeps the access token. The user (from
    /// `viewer`), or the OAuth error code (`invalid_grant`, `too_many_requests`).
    async fn login(&mut self, username: &str, password: &str) -> Result<Value, String> {
        let (status, body, _) = self
            .form(
                "/oauth/password",
                &[
                    ("client_id", "sovemu-native"),
                    ("username", username),
                    ("password", password),
                    ("scope", "openid profile offline_access"),
                ],
            )
            .await;
        if status != StatusCode::OK {
            return Err(body["error"].as_str().unwrap_or_default().to_owned());
        }
        self.token = Some(body["access_token"].as_str().unwrap().to_owned());
        Ok(self.ok(VIEWER, json!({})).await["viewer"]["user"].clone())
    }

    /// As the signed-in admin: adds a viewer, then signs in as them.
    async fn switch_to_new_viewer(&mut self, username: &str) {
        let add = "mutation($u: String!) { addUser(input: {username: $u, password: \"password123\"}) { id } }";
        self.ok(add, json!({"u": username})).await;
        self.sign_out();
        self.login(username, "password123").await.unwrap();
    }

    /// Forgets the access token and the browser sign-in.
    fn sign_out(&mut self) {
        self.cookie = None;
        self.token = None;
    }

    async fn systems(&mut self) -> Value {
        self.ok(SYSTEMS, json!({})).await["systems"].clone()
    }

    /// A file URL signed for `user_id`, as the API would hand it out.
    /// Gives the NES test game downloaded box art with these bytes, as a lookup and the
    /// download job would (artwork comes only from metadata services); returns its file.
    async fn give_box_art(&self, bytes: &[u8]) -> PathBuf {
        let id: i64 = sqlx::query_scalar("SELECT id FROM games WHERE system = 'nes'")
            .fetch_one(&self.state.db)
            .await
            .unwrap();
        let name = format!("{id}-box.webp");
        sqlx::query(
            "INSERT INTO game_metadata (game_id, status, box_art) VALUES (?, 'found', ?)
             ON CONFLICT(game_id) DO UPDATE SET box_art = excluded.box_art",
        )
        .bind(id)
        .bind(&name)
        .execute(&self.state.db)
        .await
        .unwrap();
        let file = self.state.metadata.media_dir.join(name);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, bytes).unwrap();
        file
    }

    fn signed(&self, user_id: i64, path: &str) -> String {
        self.state.assets.sign(user_id, path)
    }

    async fn games(&mut self, system: &str) -> Value {
        self.ok(GAMES, json!({"id": system})).await["system"]["games"].clone()
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// A pair of UDP ports no other harness in this run uses.
/// Two free UDP ports in a row, for this test's worker pool. Under nextest every test is its
/// own process, all at once, so a range worked out from the process id could be another test's
/// at the same moment (their workers then failed to bind, now and then); a pair is taken only
/// once both ports bind here.
fn udp_ports() -> String {
    loop {
        let first = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let base = first.local_addr().unwrap().port();
        if base < u16::MAX && std::net::UdpSocket::bind(("127.0.0.1", base + 1)).is_ok() {
            return format!("{base}-{}", base + 1);
        }
    }
}

const SETUP: &str = "mutation($u: String!, $p: String!) { setup(username: $u, password: $p) { id username isAdmin } }";
const VIEWER: &str = "{ viewer { setupRequired user { username isAdmin } } }";
const SYSTEMS: &str = "{ systems { id name gameCount playable rendering } }";
const GAMES: &str = "query($id: ID!) { system(id: $id) { games {
    id title fileTitle tags lastPlayedAt playCount media mediaUrl(kind: THUMB) description
    developer releaseDate rating players patch } } }";
const START: &str =
    "mutation($game: Int!, $start: String) { startSession(gameId: $game, startId: $start) {
    id title core width height } }";
const PROGRESS: &str =
    "query($id: String!) { startProgress(startId: $id) { phase elapsedMs error } }";
const METADATA: &str = "{ metadataStatus { running total done found failed sources notes \
                        artwork { running total done downloaded failed videos } \
                        working { title } finishedAt systems { system total done } \
                        providers { id asked found notFound errors quota { requestsToday } \
                        threads account { level maxThreads maxRequestsPerMinute } } \
                        recent { title outcome sources } } }";
const LIBRARY: &str = "{ libraryStatus { mode games cacheBytes mirror { running filesDone } } }";

fn system<'a>(systems: &'a Value, id: &str) -> &'a Value {
    systems.as_array().unwrap().iter().find(|s| s["id"] == id).unwrap()
}

#[tokio::test]
async fn first_run_setup_then_sign_in() {
    let mut h = Harness::new("auth", false).await;
    assert_eq!(
        h.ok(VIEWER, json!({})).await["viewer"],
        json!({"setupRequired": true, "user": null})
    );
    assert_eq!(h.err(SYSTEMS, json!({})).await.0, "UNAUTHENTICATED");

    h.sign_up_admin().await;
    let viewer = h.ok(VIEWER, json!({})).await["viewer"].clone();
    assert_eq!(
        viewer,
        json!({"setupRequired": false, "user": {"username": "admin", "isAdmin": true}})
    );

    // Setup only works once.
    assert_eq!(h.err(SETUP, json!({"u": "eve", "p": "password123"})).await.0, "CONFLICT");

    // The admin adds a player, who can sign in but not administer.
    let add = "mutation($u: String!) { addUser(input: {username: $u, password: \"password123\"}) { id } }";
    h.ok(add, json!({"u": "kid"})).await;
    assert_eq!(h.err(add, json!({"u": "kid"})).await.0, "CONFLICT");
    h.sign_out();
    assert_eq!(h.ok(VIEWER, json!({})).await["viewer"]["user"], Value::Null, "no token, no user");

    assert_eq!(h.login("kid", "nope-nope").await, Err("invalid_grant".into()));
    assert_eq!(h.login("ghost", "password123").await, Err("invalid_grant".into()));
    assert_eq!(h.login("KID", "password123").await.unwrap()["isAdmin"], false);
    assert_eq!(h.err("{ users { username } }", json!({})).await.0, "FORBIDDEN");
    assert_eq!(h.err("{ activeSessions { id } }", json!({})).await.0, "FORBIDDEN");
    assert_eq!(h.err("mutation { scanLibrary { found } }", json!({})).await.0, "FORBIDDEN");
    let ids: Vec<Value> =
        h.systems().await.as_array().unwrap().iter().map(|s| s["id"].clone()).collect();
    assert_eq!(ids, [json!("nes"), json!("gba")], "only the systems with games");
}

#[tokio::test]
async fn throttles_guessing_and_serves_ops_routes() {
    let mut h = Harness::new("ops", false).await;
    h.sign_up_admin().await;
    h.sign_out();
    for _ in 0..sovemu_identity::MAX_FAILURES {
        assert_eq!(h.login("admin", "wrong-password").await, Err("invalid_grant".into()));
    }
    let attempt =
        [("client_id", "sovemu-native"), ("username", "admin"), ("password", "wrong-password")];
    let (status, body, _) = h.form("/oauth/password", &attempt).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(body["error"], "too_many_requests");
    assert!(body["error_description"].as_str().unwrap().contains("try again"), "{body}");
    // Locked out even with the right password, until the window passes.
    assert_eq!(h.login("admin", "password123").await, Err("too_many_requests".into()));

    let health = Request::builder().uri("/health").body(Body::empty()).unwrap();
    let response = router(h.state.clone()).oneshot(health).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["x-content-type-options"], "nosniff");
    assert_eq!(response.headers()["x-frame-options"], "DENY");
    assert_eq!(
        h.call("GET", "/metrics", None).await.0,
        StatusCode::NOT_FOUND,
        "no Prometheus route: metrics go out over OpenTelemetry"
    );
    assert_eq!(h.err("{ clientConfig { iceServers } }", json!({})).await.0, "UNAUTHENTICATED");
    // The REST API is gone.
    assert_eq!(h.call("GET", "/api/systems", None).await.0, StatusCode::NOT_FOUND);
    // GraphiQL, for trying queries in a browser.
    assert_eq!(h.call("GET", "/graphql", None).await.0, StatusCode::OK);
}

/// A sign-in attempt from `peer`, with `forwarded` as its `X-Forwarded-For`: the OAuth error
/// code, or "" on success.
async fn login_from(h: &Harness, peer: &str, forwarded: Option<&str>, user: &str) -> String {
    let body = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs([
            ("client_id", "sovemu-native"),
            ("username", user),
            ("password", "wrong-password"),
        ])
        .finish();
    let mut request = Request::builder()
        .method("POST")
        .uri("/oauth/password")
        .header(header::HOST, "sovemu.test")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded");
    if let Some(f) = forwarded {
        request = request.header("x-forwarded-for", f);
    }
    let mut request = request.body(Body::from(body)).unwrap();
    let peer: std::net::SocketAddr = format!("{peer}:40000").parse().unwrap();
    request.extensions_mut().insert(axum::extract::ConnectInfo(peer));
    let response = router(h.state.clone()).oneshot(request).await.unwrap();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let value: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    value["error"].as_str().unwrap_or_default().to_owned()
}

#[tokio::test]
async fn throttles_each_address_across_usernames_behind_a_trusted_proxy() {
    let extra = ["--login-max-failures-per-ip", "3", "--trusted-proxies", "10.0.0.0/8"];
    let h = Harness::new_with("per-ip", false, extra.map(String::from).to_vec()).await;
    // A different name each time, so the per-username limit never trips.
    for name in ["a", "b", "c"] {
        assert_eq!(login_from(&h, "10.0.0.2", Some("198.51.100.7"), name).await, "invalid_grant");
    }
    assert_eq!(login_from(&h, "10.0.0.2", Some("198.51.100.7"), "d").await, "too_many_requests");
    // Another client behind the same proxy is not locked out...
    assert_eq!(login_from(&h, "10.0.0.2", Some("198.51.100.8"), "d").await, "invalid_grant");
    // ...nor can the locked-out one get past by claiming another address: its own claim comes
    // first, and the proxy appends the real one.
    let spoofed = Some("203.0.113.1, 198.51.100.7");
    assert_eq!(login_from(&h, "10.0.0.2", spoofed, "e").await, "too_many_requests");
    // An untrusted peer's header is ignored: it counts as itself.
    for name in ["f", "g", "h"] {
        assert_eq!(
            login_from(&h, "192.0.2.5", Some(&format!("10.1.1.{}", name.len())), name).await,
            "invalid_grant"
        );
    }
    assert_eq!(login_from(&h, "192.0.2.5", Some("10.9.9.9"), "i").await, "too_many_requests");
}

#[tokio::test]
async fn compresses_api_responses() {
    let mut h = Harness::new("compression", false).await;
    h.sign_up_admin().await;
    h.give_box_art(&[0u8; 2048]).await;
    let body = json!({"query": "{ __schema { types { name } } }"});
    let request = Request::builder()
        .method("POST")
        .uri("/graphql")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCEPT_ENCODING, "br, gzip")
        .body(Body::from(body.to_string()))
        .unwrap();
    let response = router(h.state.clone()).oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_ENCODING], "br");
    // Without Accept-Encoding, plain.
    let request = Request::builder()
        .method("POST")
        .uri("/graphql")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let response = router(h.state.clone()).oneshot(request).await.unwrap();
    assert!(response.headers().get(header::CONTENT_ENCODING).is_none());
    // Images are not recompressed.
    let png = h.games("nes").await[0]["mediaUrl"].as_str().unwrap().to_owned();
    let request = Request::builder()
        .uri(png)
        .header(header::ACCEPT_ENCODING, "gzip")
        .body(Body::empty())
        .unwrap();
    let response = router(h.state.clone()).oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.headers().get(header::CONTENT_ENCODING).is_none(), "{:?}", response.headers());
}

#[tokio::test]
async fn browses_the_library() {
    let mut h = Harness::new("library", false).await;
    h.sign_up_admin().await;
    assert_eq!(
        h.ok("{ clientConfig { iceServers } }", json!({})).await["clientConfig"]["iceServers"],
        json!([])
    );
    let systems = h.systems().await;
    assert_eq!(system(&systems, "nes")["gameCount"], 1);
    assert_eq!(system(&systems, "nes")["name"], "Nintendo Entertainment System");
    assert_eq!(system(&systems, "nes")["playable"], false, "no cores installed");
    assert!(
        systems.as_array().unwrap().iter().all(|s| s["id"] != "snes"),
        "a system with no games is not listed"
    );

    let games = h.games("nes").await;
    assert_eq!(games[0]["media"], json!([]), "the artwork beside the ROM is not read");
    assert_eq!(games[0]["mediaUrl"], Value::Null);
    h.give_box_art(&[0u8; 2048]).await;
    let games = h.games("nes").await;
    assert_eq!(games[0]["title"], "Test Game");
    assert_eq!(games[0]["tags"], json!(["USA"]));
    assert_eq!(games[0]["lastPlayedAt"], Value::Null);
    assert_eq!(games[0]["playCount"], 0);
    assert_eq!(games[0]["media"], json!(["THUMB"]));
    let id = games[0]["id"].as_i64().unwrap();
    // Files are at signed URLs (assets.rs): for this user, for a while, no cookie needed.
    let url = games[0]["mediaUrl"].as_str().unwrap().to_owned();
    assert!(url.starts_with(&format!("/media/games/{id}/thumb?Sov-UserId=1&Sov-Expiry=")), "{url}");
    let token = h.token.take();
    assert_eq!(h.call("GET", &url, None).await.0, StatusCode::OK, "no cookie needed");
    assert_eq!(
        h.call("GET", &format!("/media/games/{id}/thumb"), None).await.0,
        StatusCode::UNAUTHORIZED,
        "unsigned"
    );
    assert_eq!(
        h.call("GET", &url.replace("/thumb?", "/image?"), None).await.0,
        StatusCode::FORBIDDEN,
        "signed for another file"
    );
    let forged = sovemu_assets::AssetSigner::new(b"other key".to_vec(), 24)
        .sign(1, &format!("/media/games/{id}/thumb"));
    assert_eq!(h.call("GET", &forged, None).await.0, StatusCode::FORBIDDEN, "another key");
    h.token = token;
    let video = h.signed(1, &format!("/media/games/{id}/video"));
    assert_eq!(h.call("GET", &video, None).await.0, StatusCode::NOT_FOUND);
    let escape = h.signed(1, &format!("/media/games/{id}/..%2F..%2Fetc"));
    assert_eq!(h.call("GET", &escape, None).await.0, StatusCode::NOT_FOUND);
    // A user who no longer exists: their URLs stop working.
    let ghost = h.signed(999, &format!("/media/games/{id}/thumb"));
    assert_eq!(h.call("GET", &ghost, None).await.0, StatusCode::FORBIDDEN);
    let game = "query($id: Int!) { game(id: $id) { system } }";
    assert_eq!(h.ok(game, json!({"id": id})).await["game"]["system"], "nes");
    assert_eq!(h.err(game, json!({"id": 9999})).await.0, "NOT_FOUND");
    assert_eq!(h.ok(GAMES, json!({"id": "saturn"})).await["system"], Value::Null);

    // The home feed: games with artwork are featured, every game is new, none played yet.
    let home = h.ok("{ home { featured { title } recent { id } added { id } } }", json!({})).await;
    assert_eq!(home["home"]["featured"], json!([{"title": "Test Game"}]));
    assert_eq!(home["home"]["added"].as_array().unwrap().len(), 2);
    assert_eq!(home["home"]["recent"], json!([]));

    // A new file appears after a rescan; a deleted one goes.
    std::fs::write(h.dir.join("roms/nes/Second (Japan).nes"), [0u8; 8]).unwrap();
    std::fs::remove_file(h.dir.join("roms/gba/Other.gba")).unwrap();
    let report = h.ok("mutation { scanLibrary { found added removed } }", json!({})).await;
    assert_eq!(report["scanLibrary"], json!({"found": 2, "added": 1, "removed": 1}));

    // No core: starting a game says so, and so does its progress.
    let (code, message) = h.err(START, json!({"game": id, "start": "no-core-start"})).await;
    assert_eq!(code, "UNAVAILABLE");
    assert!(message.contains("no core"), "{message}");
    let progress = h.ok(PROGRESS, json!({"id": "no-core-start"})).await["startProgress"].clone();
    assert_eq!(progress["phase"], "FAILED");
    assert!(progress["error"].as_str().unwrap().contains("no core"));
    assert_eq!(h.err(START, json!({"game": id, "start": "../../x"})).await.0, "BAD_REQUEST");
    assert_eq!(h.err(PROGRESS, json!({"id": "never-started"})).await.0, "NOT_FOUND");
}

#[tokio::test]
async fn runs_a_game_session_in_a_worker() {
    let mut h = Harness::new("session", true).await;
    h.sign_up_admin().await;
    assert_eq!(system(&h.systems().await, "nes")["playable"], true);
    let id = h.games("nes").await[0]["id"].as_i64().unwrap();

    let session =
        h.ok(START, json!({"game": id, "start": "first-start-1"})).await["startSession"].clone();
    let progress = h.ok(PROGRESS, json!({"id": "first-start-1"})).await["startProgress"].clone();
    assert_eq!(progress["phase"], "READY", "{progress}");
    assert!(progress["elapsedMs"].as_f64().is_some());
    assert_eq!(session["core"], "SovEmu Test Core");
    assert_eq!((session["width"].clone(), session["height"].clone()), (json!(256), json!(224)));
    assert_eq!(session["title"], "Test Game");
    let sid = session["id"].as_str().unwrap().to_owned();

    // The options the core declared with its game loaded are kept for the Settings page.
    let (core, options): (String, String) =
        sqlx::query_as("SELECT core, options FROM core_options")
            .fetch_one(&h.state.db)
            .await
            .unwrap();
    assert!(core.starts_with("fceumm_libretro."), "{core}");
    assert!(options.contains("sovemu_test_pattern"), "{options}");

    let current = h.ok("{ currentSession { id connected } }", json!({})).await;
    assert_eq!(current["currentSession"]["id"], sid.as_str());

    // Admins see it on the sessions page; nothing is streamed yet, so no pipeline or health.
    let active = "{ activeSessions { id username title system core connected startedAt client pipeline { videoCodec } health { fps } clientStats { fps } } }";
    let listed = h.ok(active, json!({})).await["activeSessions"].clone();
    assert_eq!(listed.as_array().unwrap().len(), 1, "{listed}");
    let row = &listed[0];
    assert_eq!((row["id"].as_str(), row["username"].as_str()), (Some(sid.as_str()), Some("admin")));
    assert_eq!((row["title"].clone(), row["system"].clone()), (json!("Test Game"), json!("nes")));
    assert_eq!(row["connected"], false);
    assert!(row["startedAt"].as_f64().unwrap() > 1.7e9);
    for empty in ["client", "pipeline", "health", "clientStats"] {
        assert_eq!(row[empty], Value::Null, "{empty}");
    }
    let reset = "mutation($id: ID!) { resetSession(id: $id) }";
    assert_eq!(h.ok(reset, json!({"id": sid})).await["resetSession"], true);
    let offer = "mutation($id: ID!) { sessionOffer(id: $id, sdp: \"not sdp\") }";
    assert_eq!(h.err(offer, json!({"id": sid})).await.0, "BAD_REQUEST");

    // An offer carries what the client can decode: the whole document, checked before the SDP
    // and kept against the sign-in. One of another version asks the player to update.
    let offer = "mutation($id: ID!, $c: ClientCapabilitiesInput) { sessionOffer(id: $id, sdp: \"not sdp\", client: \"android 1.4.0 (Mi TV)\", capabilities: $c) }";
    let document = json!({
        "version": 1,
        "videoDecoders": [
            {"codec": "VP8", "hardware": false, "lowLatency": false, "name": "c2.amlogic.vp8.decoder.sw"},
            {"codec": "H264", "hardware": true, "lowLatency": true, "name": " c2.amlogic.avc.decoder\n"},
            {"codec": "AV1", "hardware": true, "lowLatency": true},
        ],
        "display": {"width": 3840, "height": 2160, "refreshHz": 59.94},
    });
    let kept = "{ signIns { capabilities { version videoDecoders { codec hardware lowLatency name } display { width height refreshHz } } } }";
    let stored = |v: Value| v["signIns"][0]["capabilities"].clone();
    assert_eq!(stored(h.ok(kept, json!({})).await), Value::Null, "nothing reported yet");
    let mut newer = document.clone();
    newer["version"] = json!(2);
    let (code, message) = h.err(offer, json!({"id": sid, "c": newer})).await;
    assert_eq!(code, "BAD_REQUEST");
    assert!(message.contains("version 2") && message.ends_with("Update the app."), "{message}");
    assert_eq!(stored(h.ok(kept, json!({})).await), Value::Null, "a refused document is not kept");
    let (code, message) = h.err(offer, json!({"id": sid, "c": document})).await;
    assert_eq!(code, "BAD_REQUEST");
    assert!(!message.contains("capabilities"), "the SDP is what is wrong now: {message}");
    let mut expected = document.clone();
    expected["videoDecoders"][1]["name"] = json!("c2.amlogic.avc.decoder");
    expected["videoDecoders"][2]["name"] = Value::Null;
    assert_eq!(stored(h.ok(kept, json!({})).await), expected);

    // What a device's decoder measures is kept by sign-in and codec, the latest replacing the
    // last, and read back for the next offer's choice of codec.
    let family: String = sqlx::query_scalar("SELECT family_id FROM sign_in_capabilities")
        .fetch_one(&h.state.db)
        .await
        .unwrap();
    crate::decode_times::keep(&h.state.db, &family, "h264", 23.0, 20).await.unwrap();
    crate::decode_times::keep(&h.state.db, &family, "h264", 21.5, 40).await.unwrap();
    crate::decode_times::keep(&h.state.db, &family, "vp8", 7.0, 20).await.unwrap();
    crate::decode_times::keep(&h.state.db, "a sign-in that has ended", "vp8", 9.0, 20)
        .await
        .unwrap();
    let mut measured = crate::decode_times::of(&h.state.db, &family).await.unwrap();
    measured.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(measured, [("h264".to_owned(), 21.5), ("vp8".to_owned(), 7.0)]);
    let gone = crate::decode_times::of(&h.state.db, "a sign-in that has ended").await.unwrap();
    assert!(gone.is_empty(), "an ended sign-in's times are forgotten: {gone:?}");

    // Save states: save slot 2, see it listed, load it; an empty slot cannot be loaded.
    let saves = "query($id: Int!) { game(id: $id) { saves { slot savedAt screenshotUrl } autoSave { slot } } }";
    let save = "mutation($s: ID!, $slot: Int!) { saveState(sessionId: $s, slot: $slot) }";
    let load = "mutation($s: ID!, $slot: Int!) { loadState(sessionId: $s, slot: $slot) }";
    let delete = "mutation($g: Int!, $slot: Int!) { deleteSave(gameId: $g, slot: $slot) }";
    assert_eq!(h.ok(saves, json!({"id": id})).await["game"]["saves"], json!([]));
    h.ok(save, json!({"s": sid, "slot": 2})).await;
    let game = h.ok(saves, json!({"id": id})).await["game"].clone();
    assert_eq!(game["saves"][0]["slot"], 2);
    assert_eq!(game["saves"][0]["screenshotUrl"], Value::Null, "no frame streamed yet");
    assert_eq!(game["autoSave"], Value::Null);
    h.ok(load, json!({"s": sid, "slot": 2})).await;
    let (code, message) = h.err(load, json!({"s": sid, "slot": 5})).await;
    assert_eq!(code, "BAD_REQUEST");
    assert!(message.contains("empty"), "{message}");
    let screenshot = h.signed(1, &format!("/media/games/{id}/saves/2"));
    assert_eq!(h.call("GET", &screenshot, None).await.0, StatusCode::NOT_FOUND);
    assert!(h.dir.join("saves/1/nes/states/Test Game (USA)/slot2.state").is_file());
    h.ok(delete, json!({"g": id, "slot": 2})).await;
    assert_eq!(h.ok(saves, json!({"id": id})).await["game"]["saves"], json!([]));
    assert_eq!(h.err(delete, json!({"g": id, "slot": 2})).await.0, "NOT_FOUND");

    // Cheats: added, toggled and removed while the game runs (each change reaches the worker).
    let add = "mutation($g: Int!, $c: String!, $d: String!) { addCheat(gameId: $g, code: $c, description: $d) { id code enabled } }";
    let set = "mutation($g: Int!, $c: Int!) { setCheatEnabled(gameId: $g, cheatId: $c, enabled: false) { enabled } }";
    let remove = "mutation($g: Int!, $c: Int!) { deleteCheat(gameId: $g, cheatId: $c) }";
    let list =
        h.ok(add, json!({"g": id, "c": " SXIOPO ", "d": "Infinite lives"})).await["addCheat"]
            .clone();
    assert_eq!(
        (list[0]["code"].clone(), list[0]["enabled"].clone()),
        (json!("SXIOPO"), json!(true))
    );
    let cheat = list[0]["id"].as_i64().unwrap();
    assert_eq!(
        h.ok(set, json!({"g": id, "c": cheat})).await["setCheatEnabled"][0]["enabled"],
        false
    );
    assert_eq!(h.err(add, json!({"g": id, "c": "  ", "d": ""})).await.0, "BAD_REQUEST");
    h.ok(remove, json!({"g": id, "c": cheat})).await;
    assert_eq!(h.err(remove, json!({"g": id, "c": cheat})).await.0, "NOT_FOUND");
    assert_eq!(h.err(add, json!({"g": 9999, "c": "X", "d": ""})).await.0, "NOT_FOUND");

    // Play history is recorded.
    assert_eq!(h.games("nes").await[0]["playCount"], 1);
    let home = h.ok("{ home { recent { id } } }", json!({})).await;
    assert_eq!(home["home"]["recent"][0]["id"], id, "the game just played comes first");

    // Starting another game replaces the session; stopping ends it.
    let second = h.ok(START, json!({"game": id})).await["startSession"]["id"].clone();
    assert_ne!(second, sid.as_str());
    assert_eq!(h.state.workers.count(), 1);
    let stop = "mutation($id: ID!) { stopSession(id: $id) }";
    h.ok(stop, json!({"id": second})).await;
    assert_eq!(h.state.workers.count(), 0);
    assert_eq!(h.ok("{ currentSession { id } }", json!({})).await["currentSession"], Value::Null);
    assert_eq!(h.err(stop, json!({"id": sid})).await.0, "NOT_FOUND");
}

/// A stand-in for ScreenScraper on a local port. It answers for the NES game (checking the
/// exact query a real client must send: by file and MD5 the first time, by game id once known)
/// and for the GBA one, whose box art it serves only with credentials. `quota` makes it refuse.
/// Returns its address and every query it was sent.
async fn mock_services(
    quota: bool,
) -> (String, Arc<std::sync::Mutex<Vec<HashMap<String, String>>>>) {
    use axum::extract::Query;
    use axum::routing::get;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let calls = Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen = calls.clone();
    let image = format!("{base}/api2/mediaJeu.php?devid=dev&amp;devpassword=pw&amp;media=");
    let box_url = format!("{image}box-2D(wor)");
    let video_url = format!(
        "{base}/api2/mediaVideoJeu.php?devid=dev&amp;devpassword=pw&amp;media=video-normalized"
    );
    let gba = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Data>
  <ssuser><id>someone</id><requeststoday>3</requeststoday><maxrequestsperday>20000</maxrequestsperday></ssuser>
  <jeu id="77">
    <noms><nom region="wor">Other Game</nom></noms>
    <developpeur>Dev Co</developpeur>
    <joueurs>2</joueurs>
    <synopsis><synopsis langue="en">From ScreenScraper</synopsis></synopsis>
    <medias><media type="box-2D" region="wor" format="png">{box_url}</media>
      <media type="ss" region="wor" format="png">{image}ss(wor)</media>
      <media type="wheel" region="wor" format="png">{image}wheel(wor)</media>
      <media type="video-normalized" format="mp4">{video_url}</media></medias>
  </jeu>
</Data>"#
    );
    // The library's sample game, its pictures served by this ScreenScraper.
    let nes = include_str!("../../SovEmu.Library/tests/fixtures/screenscraper.xml")
        .replace("https://example.invalid/", &image);
    let app = axum::Router::new()
        .route(
            "/api2/ssuserInfos.php",
            get(|Query(q): Query<HashMap<String, String>>| async move {
                assert_eq!(q["devid"], "dev");
                // As the real one: no user account, or a wrong one, is refused here.
                if q.get("ssid").is_none_or(|u| u == "refused") {
                    return (
                        StatusCode::FORBIDDEN,
                        "Erreur de login : Vérifier les identifiants utilisateurs !".to_owned(),
                    );
                }
                // An account that may have two requests open, 600 a minute.
                (
                    StatusCode::OK,
                    "<Data><ssuser><id>someone</id><niveau>1</niveau><maxthreads>2</maxthreads>\
                     <maxrequestspermin>600</maxrequestspermin><requeststoday>1</requeststoday>\
                     </ssuser></Data>"
                        .to_owned(),
                )
            }),
        )
        .route(
            "/api2/jeuInfos.php",
            get(move |Query(q): Query<HashMap<String, String>>| {
                let (gba, nes) = (gba.clone(), nes.clone());
                let seen = seen.clone();
                async move {
                    seen.lock().unwrap().push(q.clone());
                    if quota {
                        return (StatusCode::from_u16(430).unwrap(), "Quota exceeded".to_owned());
                    }
                    assert_eq!(q["devid"], "dev");
                    assert_eq!(q["devpassword"], "pw");
                    assert_eq!(q["output"], "xml");
                    if q.get("ssid").is_some_and(|u| u == "refused") {
                        return (
                            StatusCode::FORBIDDEN,
                            "Erreur de login : Vérifier les identifiants utilisateurs !".to_owned(),
                        );
                    }
                    match q["systemeid"].as_str() {
                        "3" => {
                            if let Some(id) = q.get("gameid") {
                                assert_eq!(id, "1234", "the id the first lookup found");
                                assert!(!q.contains_key("md5"), "no hashing once the id is known");
                            } else {
                                assert_eq!(q["romnom"], "Test Game (USA).nes");
                                assert_eq!(q["romtaille"], "64");
                                // MD5 of the fixture's 64 zero bytes.
                                assert_eq!(q["md5"], "3b5d3c7d207e37dceeedd301e35e2e58");
                            }
                            (StatusCode::OK, nes)
                        }
                        "12" => (StatusCode::OK, gba),
                        _ => (
                            StatusCode::NOT_FOUND,
                            "Erreur : Rom/Iso/Dossier non trouvée !".to_owned(),
                        ),
                    }
                }
            }),
        )
        .route(
            "/api2/mediaJeu.php",
            get(|Query(q): Query<HashMap<String, String>>| async move {
                if q.get("devid").map(String::as_str) != Some("dev") {
                    return (StatusCode::FORBIDDEN, [(header::CONTENT_TYPE, "text/plain")], vec![]);
                }
                // Asked for at the size it is shown, not as a full scan.
                assert_eq!((q["maxwidth"].as_str(), q["maxheight"].as_str()), ("1000", "1000"));
                (
                    StatusCode::OK,
                    [(header::CONTENT_TYPE, "image/png")],
                    include_bytes!("../../SovEmu.Library/tests/fixtures/box.png").to_vec(),
                )
            }),
        )
        .route(
            "/api2/mediaVideoJeu.php",
            get(|Query(q): Query<HashMap<String, String>>| async move {
                if q.get("devid").map(String::as_str) != Some("dev") {
                    return (StatusCode::FORBIDDEN, [(header::CONTENT_TYPE, "text/plain")], vec![]);
                }
                assert!(!q.contains_key("maxwidth"), "videos are not scaled");
                let mp4 = [&[0u8, 0, 0, 24][..], b"ftypisom", &[0u8; 16]].concat();
                (StatusCode::OK, [(header::CONTENT_TYPE, "video/mp4")], mp4)
            }),
        );
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (base, calls)
}

/// ScreenScraper with developer credentials and a user account (`user`: none for a guest).
fn metadata_args_as(base: &str, user: Option<&str>) -> Vec<String> {
    let mut args: Vec<String> = [
        "--screenscraper-devid",
        "dev",
        "--screenscraper-devpassword",
        "pw",
        "--screenscraper-url",
        &format!("{base}/api2"),
        "--screenscraper-delay-ms",
        "0",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    if let Some(user) = user {
        args.extend(
            ["--screenscraper-user", user, "--screenscraper-password", "pw"].map(String::from),
        );
    }
    args
}

fn metadata_args(base: &str) -> Vec<String> {
    metadata_args_as(base, Some("someone"))
}

async fn wait_for_job(h: &mut Harness) -> Value {
    for _ in 0..200 {
        let status = h.ok(METADATA, json!({})).await["metadataStatus"].clone();
        // The lookup, then the box art downloads that follow it.
        if status["running"] == false
            && status["done"].as_u64() > Some(0)
            && status["artwork"]["running"] == false
        {
            return status;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("the metadata job did not finish");
}

const FETCH: &str = "mutation($s: ID) { fetchMetadata(system: $s) { running } }";

/// Sends `mutation` and waits for the job it starts to finish (not for one that finished
/// before it).
async fn fetch_and_wait(h: &mut Harness, mutation: &str) -> Value {
    let before = h.ok(METADATA, json!({})).await["metadataStatus"]["finishedAt"].clone();
    h.ok(mutation, json!({})).await;
    for _ in 0..200 {
        let status = h.ok(METADATA, json!({})).await["metadataStatus"].clone();
        // A job that has just started has no finish time yet: wait for a new one.
        if status["running"] == false
            && status["finishedAt"].is_f64()
            && status["finishedAt"] != before
            && status["artwork"]["running"] == false
        {
            return status;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("the metadata job did not finish");
}

#[tokio::test]
async fn asks_again_about_games_not_found_once_a_service_is_added() {
    // Both games were looked up before ScreenScraper was configured: FBNeo alone, nothing found.
    let (base, _) = mock_services(false).await;
    let mut h = Harness::new_with("metadata-retry", false, metadata_args(&base)).await;
    h.sign_up_admin().await;
    sqlx::query(
        "INSERT INTO game_metadata (game_id, status, sources, tried) SELECT id, 'not_found', '', 'fbneo' FROM games",
    )
    .execute(&h.state.db)
    .await
    .unwrap();

    // ScreenScraper is configured now, so an ordinary (not forced) job asks it.
    h.ok(FETCH, json!({})).await;
    let status = wait_for_job(&mut h).await;
    assert_eq!(
        (status["total"].clone(), status["found"].clone()),
        (json!(2), json!(2)),
        "{status}"
    );
    assert_eq!(h.games("nes").await[0]["title"], "Castlevania");

    // Once every configured service has been asked, a game is not asked again.
    sqlx::query("UPDATE game_metadata SET status = 'not_found', tried = 'fbneo,screenscraper'")
        .execute(&h.state.db)
        .await
        .unwrap();
    h.ok(FETCH, json!({})).await;
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert_eq!(h.ok(METADATA, json!({})).await["metadataStatus"]["total"], 0);
}

#[tokio::test]
async fn fetches_metadata_from_screenscraper_with_detailed_progress() {
    let (base, _) = mock_services(false).await;
    let mut h = Harness::new_with("metadata", false, metadata_args(&base)).await;
    h.sign_up_admin().await;
    let status = h.ok(METADATA, json!({})).await["metadataStatus"].clone();
    assert_eq!(status["sources"], json!(["fbneo", "screenscraper"]));
    assert_eq!(status["notes"], json!([]));

    h.ok(FETCH, json!({})).await;
    let status = wait_for_job(&mut h).await;
    assert_eq!(
        (status["found"].clone(), status["failed"].clone()),
        (json!(2), json!(0)),
        "{status}"
    );
    // Box art, a screenshot and a logo for each game, and the GBA game's video, in the
    // background, downloaded with the credentials the stored URLs were stripped of. The NES
    // game's own artwork beside the ROM is not read.
    assert_eq!(
        status["artwork"],
        json!({"running": false, "total": 7, "done": 7, "downloaded": 7, "failed": 0, "videos": 1})
    );
    let stored: String =
        sqlx::query_scalar("SELECT box_art_url FROM game_metadata WHERE box_art_url IS NOT NULL")
            .fetch_one(&h.state.db)
            .await
            .unwrap();
    assert!(!stored.contains("devpassword"), "{stored}");

    // The progress: per system, per provider (with the quota ScreenScraper last reported), and
    // the games just looked up, most recent first.
    assert_eq!(status["working"], json!([]));
    assert!(status["finishedAt"].as_f64().is_some());
    assert_eq!(
        status["systems"],
        json!([{"system": "gba", "total": 1, "done": 1}, {"system": "nes", "total": 1, "done": 1}])
    );
    assert_eq!(
        status["providers"],
        json!([
            {"id": "fbneo", "asked": 2, "found": 0, "notFound": 2, "errors": 0, "quota": null,
             "threads": 1, "account": null},
            {"id": "screenscraper", "asked": 2, "found": 2, "notFound": 0, "errors": 0,
             "quota": {"requestsToday": 3.0}, "threads": 2,
             "account": {"level": 1, "maxThreads": 2, "maxRequestsPerMinute": 600}},
        ])
    );
    // Looked up two at a time (the account allows two), so in either order.
    let mut recent = status["recent"].as_array().unwrap().clone();
    recent.sort_by_key(|r| r["title"].as_str().unwrap().to_owned());
    assert_eq!(
        recent,
        [
            json!({"title": "Castlevania", "outcome": "FOUND", "sources": ["screenscraper"]}),
            json!({"title": "Other Game", "outcome": "FOUND", "sources": ["screenscraper"]}),
        ]
    );

    // ScreenScraper named the NES game and described it; the file's own title is kept too.
    let nes = h.games("nes").await;
    assert_eq!(nes[0]["title"], "Castlevania");
    assert_eq!(nes[0]["fileTitle"], "Test Game");
    assert_eq!(nes[0]["developer"], "Konami");
    assert_eq!(nes[0]["releaseDate"], "1987-05-01");
    assert_eq!(nes[0]["rating"], 0.8);

    let gba = h.games("gba").await;
    assert_eq!(gba[0]["description"], "From ScreenScraper");
    assert_eq!(gba[0]["developer"], "Dev Co");
    assert_eq!(gba[0]["players"], "2");
    assert_eq!(gba[0]["media"], json!(["IMAGE", "THUMB", "MARQUEE", "VIDEO"]));
    let gba_id = gba[0]["id"].as_i64().unwrap();
    for picture in ["box", "shot", "marquee"] {
        let kept = std::fs::read(h.dir.join(format!("metadata/media/{gba_id}-{picture}.webp")));
        assert_eq!(&kept.unwrap()[8..12], b"WEBP", "{picture} is kept as WebP");
    }
    let url = gba[0]["mediaUrl"].as_str().unwrap().to_owned();
    assert_eq!(h.call("GET", &url, None).await.0, StatusCode::OK);

    // Looked-up games are not asked for again.
    h.ok(FETCH, json!({})).await;
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert_eq!(h.ok(METADATA, json!({})).await["metadataStatus"]["total"], 0);
}

#[tokio::test]
async fn a_guest_looks_games_up_without_a_user_account() {
    // Only developer credentials, as a fresh install has: ssuserInfos.php would refuse (it needs
    // a user account), so it is not asked, and the lookups run as a guest.
    let (base, calls) = mock_services(false).await;
    let mut h = Harness::new_with("metadata-guest", false, metadata_args_as(&base, None)).await;
    h.sign_up_admin().await;
    h.ok(FETCH, json!({})).await;
    let status = wait_for_job(&mut h).await;
    assert_eq!(
        (status["found"].clone(), status["notes"].clone()),
        (json!(2), json!([])),
        "{status}"
    );
    assert!(calls.lock().unwrap().iter().all(|q| !q.contains_key("ssid")), "as a guest");
    // The guest's limits come with the answers.
    assert_eq!(status["providers"][1]["account"]["level"], 1, "{status}");
}

#[tokio::test]
async fn a_refused_user_account_falls_back_to_a_guest() {
    let (base, calls) = mock_services(false).await;
    let mut h =
        Harness::new_with("metadata-refused", false, metadata_args_as(&base, Some("refused")))
            .await;
    h.sign_up_admin().await;
    h.ok(FETCH, json!({})).await;
    let status = wait_for_job(&mut h).await;
    assert_eq!(status["found"], 2, "still looked up, as a guest: {status}");
    assert!(
        status["notes"].as_array().unwrap().iter().any(|n| n.as_str().unwrap().contains("guest")),
        "{status}"
    );
    assert!(calls.lock().unwrap().iter().all(|q| !q.contains_key("ssid")), "the account dropped");
}

#[tokio::test]
async fn a_partial_refresh_asks_by_game_id_and_keeps_other_fields() {
    let (base, calls) = mock_services(false).await;
    let mut h = Harness::new_with("metadata-partial", false, metadata_args(&base)).await;
    h.sign_up_admin().await;
    h.ok(FETCH, json!({"s": "nes"})).await;
    wait_for_job(&mut h).await;
    let sources: String = sqlx::query_scalar("SELECT field_sources FROM game_metadata")
        .fetch_one(&h.state.db)
        .await
        .unwrap();
    assert!(sources.contains(r#""description":"screenscraper""#), "{sources}");

    sqlx::query("UPDATE game_metadata SET title = 'Kept', description = 'Edited'")
        .execute(&h.state.db)
        .await
        .unwrap();
    let refresh = "mutation { fetchMetadata(system: \"nes\", force: true, fields: [DESCRIPTION], \
                   sources: [\"screenscraper\"]) { running } }";
    fetch_and_wait(&mut h, refresh).await;
    let nes = h.games("nes").await;
    assert_eq!(nes[0]["title"], "Kept", "a field not asked for is kept");
    assert_eq!(nes[0]["description"], "Simon Belmont sets out to defeat Dracula.");
    let last = calls.lock().unwrap().last().cloned().unwrap();
    assert_eq!(last["gameid"], "1234", "asked by the id the first lookup found");

    let bad = "mutation { fetchMetadata(sources: [\"launchbox\"]) { running } }";
    assert_eq!(h.err(bad, json!({})).await.0, "BAD_REQUEST");
}

#[tokio::test]
async fn without_credentials_screenscraper_is_skipped_with_a_note() {
    let mut h = Harness::new("metadata-none", false).await;
    h.sign_up_admin().await;
    let status = h.ok(METADATA, json!({})).await["metadataStatus"].clone();
    assert_eq!(status["sources"], json!(["fbneo"]));
    assert!(status["notes"][0].as_str().unwrap().contains("no developer credentials"), "{status}");
}

/// The width of a lossy (VP8) WebP, from its frame header.
fn webp_width(bytes: &[u8]) -> u32 {
    assert_eq!((&bytes[..4], &bytes[8..12]), (&b"RIFF"[..], &b"WEBP"[..]), "a WebP");
    // The first chunk is VP8 (opaque) or VP8X (with alpha); a VP8 frame's width is at 26.
    assert_eq!(&bytes[12..16], b"VP8 ", "lossy");
    u32::from(u16::from_le_bytes([bytes[26], bytes[27]]) & 0x3fff)
}

#[tokio::test]
async fn artwork_at_a_width_is_a_kept_webp_copy() {
    let mut h = Harness::new("artwork-width", false).await;
    h.sign_up_admin().await;
    let thumb = h.give_box_art(include_bytes!("../../SovEmu.Library/tests/fixtures/box.png")).await;
    let q = "query { system(id: \"nes\") { games { id \
             small: mediaUrl(kind: THUMB, width: 150) wide: mediaUrl(kind: THUMB, width: 2000) \
             original: mediaUrl(kind: THUMB) } } }";
    let game = h.ok(q, json!({})).await["system"]["games"][0].clone();
    let id = game["id"].as_i64().unwrap();
    let small = game["small"].as_str().unwrap();
    assert!(small.starts_with(&format!("/media/games/{id}/thumb/w160?")), "on the grid: {small}");
    assert!(!game["original"].as_str().unwrap().contains("/w"), "no width: the original");

    // A lossy WebP copy at the width, kept and served again without being made again.
    let (status, kind, bytes) = h.fetch(small).await;
    assert_eq!((status, kind.as_str()), (StatusCode::OK, "image/webp"));
    assert_eq!(webp_width(&bytes), 160);
    let copy = h.state.config.paths().artwork_widths().join(format!("games/{id}/thumb/w160.webp"));
    let made = std::fs::metadata(&copy).unwrap().modified().unwrap();
    assert_eq!(h.fetch(small).await.2, bytes);
    assert_eq!(std::fs::metadata(&copy).unwrap().modified().unwrap(), made, "kept");

    // Never enlarged: wider than the 300 px original is the original's width.
    let (_, _, wide) = h.fetch(game["wide"].as_str().unwrap()).await;
    assert_eq!(webp_width(&wide), 300);

    // A width off the grid is refused, even signed; unsigned is refused as any file is.
    let off = h.signed(1, &format!("/media/games/{id}/thumb/w150"));
    assert_eq!(h.fetch(&off).await.0, StatusCode::NOT_FOUND);
    assert_ne!(h.fetch(&format!("/media/games/{id}/thumb/w160")).await.0, StatusCode::OK);
    let video = h.signed(1, &format!("/media/games/{id}/video/w160"));
    assert_eq!(h.fetch(&video).await.0, StatusCode::NOT_FOUND, "videos have no widths");

    // A picture that cannot be read is served as it is.
    std::fs::write(&thumb, b"not a picture").unwrap();
    let other = h.signed(1, &format!("/media/games/{id}/thumb/w320"));
    let (status, _, body) = h.fetch(&other).await;
    assert_eq!((status, body.as_slice()), (StatusCode::OK, &b"not a picture"[..]));

    // Clearing the cache clears the copies; they are made again when asked for.
    crate::storage::clear_cache(&h.state).await.unwrap();
    assert!(!copy.exists());
    std::fs::write(&thumb, include_bytes!("../../SovEmu.Library/tests/fixtures/box.png")).unwrap();
    assert_eq!(webp_width(&h.fetch(small).await.2), 160);
}

#[tokio::test]
async fn startup_removes_the_launchbox_dump_and_orphaned_box_art() {
    let mut h = Harness::new("metadata-tidy", false).await;
    h.sign_up_admin().await;
    let paths = h.state.config.paths();
    let media = paths.artwork();
    std::fs::create_dir_all(paths.removed_launchbox()).unwrap();
    std::fs::write(paths.removed_launchbox().join("Metadata.zip"), b"zip").unwrap();
    std::fs::create_dir_all(&media).unwrap();
    let id: i64 = sqlx::query_scalar("SELECT id FROM games ORDER BY id LIMIT 1")
        .fetch_one(&h.state.db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO game_metadata (game_id, status, box_art) VALUES (?, 'found', ?)")
        .bind(id)
        .bind(format!("{id}-box.png"))
        .execute(&h.state.db)
        .await
        .unwrap();
    for name in
        [format!("{id}-box.png"), "999-box.jpg".into(), "999-video.mp4".into(), "notes.txt".into()]
    {
        std::fs::write(media.join(name), b"x").unwrap();
    }
    crate::metadata::tidy(&paths, &h.state.db).await;
    assert!(!paths.removed_launchbox().exists());
    assert!(media.join(format!("{id}-box.png")).exists(), "box art a game refers to stays");
    assert!(!media.join("999-box.jpg").exists(), "orphaned box art goes");
    assert!(!media.join("999-video.mp4").exists(), "and orphaned videos");
    assert!(media.join("notes.txt").exists(), "nothing else is touched");
}

#[tokio::test]
async fn stops_a_service_that_is_out_of_quota() {
    let (base, _) = mock_services(true).await;
    let mut h = Harness::new_with("metadata-quota", false, metadata_args(&base)).await;
    h.sign_up_admin().await;
    h.ok(FETCH, json!({"s": "nes"})).await;
    let status = wait_for_job(&mut h).await;
    assert_eq!(status["total"], 1, "only the system asked for");
    assert!(status["notes"][0].as_str().unwrap().contains("quota"), "{status}");
    assert_eq!(status["recent"][0]["outcome"], "FAILED");
    // A failed lookup is not stored, so the next job retries it.
    assert_eq!(h.games("nes").await[0]["description"], Value::Null);
    h.ok(FETCH, json!({"s": "nes"})).await;
    assert_eq!(wait_for_job(&mut h).await["total"], 1);
}

#[tokio::test]
async fn mirror_mode_copies_the_library_and_plays_from_it() {
    let mut h =
        Harness::new_with("mirror", false, vec!["--library-mode".into(), "mirror".into()]).await;
    h.sign_up_admin().await;
    assert_eq!(h.state.config.roms_dir, h.dir.join("library"), "the server plays from the mirror");
    let status = h.ok(LIBRARY, json!({})).await["libraryStatus"].clone();
    assert_eq!((status["mode"].clone(), status["games"].clone()), (json!("MIRROR"), json!(0)));

    crate::admin::sync_then_scan(h.state.clone()).await.unwrap();
    assert!(!h.dir.join("library/nes/images").exists(), "artwork folders are not mirrored");
    let status = h.ok(LIBRARY, json!({})).await["libraryStatus"].clone();
    assert_eq!(status["games"], 2);
    assert_eq!(status["mirror"], json!({"running": false, "filesDone": 2}));
    assert_eq!(h.ok("mutation { syncLibrary }", json!({})).await["syncLibrary"], true);
}

#[tokio::test]
async fn library_status_reports_direct_mode() {
    let mut h = Harness::new("direct", false).await;
    h.sign_up_admin().await;
    assert_eq!(
        h.ok(LIBRARY, json!({})).await["libraryStatus"],
        json!({"mode": "DIRECT", "games": 2, "cacheBytes": null, "mirror": null})
    );
    assert_eq!(h.err("mutation { syncLibrary }", json!({})).await.0, "BAD_REQUEST");
}

const JOBS: &str = "{ jobs { name intervalMinutes running lastRun { trigger outcome message } } }";

#[tokio::test]
async fn admins_see_the_jobs_and_start_them() {
    let mut h = Harness::new("jobs", false).await;
    h.sign_up_admin().await;
    let jobs = h.ok(JOBS, json!({})).await["jobs"].clone();
    let names: Vec<&str> =
        jobs.as_array().unwrap().iter().map(|j| j["name"].as_str().unwrap()).collect();
    assert_eq!(
        names,
        ["SCAN", "METADATA", "MAINTENANCE", "BACKUP"],
        "no mirror sync outside mirror mode"
    );
    assert_eq!(jobs[0]["intervalMinutes"], 60.0);
    assert_eq!(jobs[1]["intervalMinutes"], Value::Null, "the lookup follows each scan");

    h.ok("mutation { runJob(job: MAINTENANCE) { name } }", json!({})).await;
    let mut last = Value::Null;
    for _ in 0..200 {
        last = h.ok(JOBS, json!({})).await["jobs"][2]["lastRun"].clone();
        if last["outcome"] != "RUNNING" && !last.is_null() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(
        (last["trigger"].clone(), last["outcome"].clone()),
        (json!("ADMIN"), json!("SUCCEEDED"))
    );
    assert!(last["message"].as_str().unwrap().contains("expired sign-ins deleted"));

    // A rescan waited for is recorded too.
    h.ok("mutation { scanLibrary { found } }", json!({})).await;
    let runs = h.ok("{ jobRuns(job: SCAN) { trigger outcome message } }", json!({})).await;
    assert_eq!(
        runs["jobRuns"][0],
        json!({"trigger": "ADMIN", "outcome": "SUCCEEDED", "message": "2 games: 0 added, 0 removed"})
    );
    assert_eq!(
        h.err("mutation { runJob(job: MIRROR_SYNC) { name } }", json!({})).await.0,
        "BAD_REQUEST"
    );
}

#[tokio::test]
async fn admins_see_the_servers_resources() {
    let mut h = Harness::new("resources", false).await;
    h.sign_up_admin().await;
    let r = h
        .ok(
            "{ serverResources { host { arch usableCpus } sampleSeconds usage { cpuPercent } usageNote startedAt } }",
            json!({}),
        )
        .await["serverResources"]
        .clone();
    assert_eq!(r["host"]["arch"], std::env::consts::ARCH);
    assert!(r["host"]["usableCpus"].as_i64() >= Some(1));
    assert_eq!(r["sampleSeconds"], 5);
    let started = r["startedAt"].as_f64().unwrap();
    let now = crate::client_events::now_unix() as f64;
    assert!(started <= now && now - started < 300.0, "started a moment ago: {started} at {now}");
    // The harness starts no sampler.
    assert_eq!((r["usage"].clone(), r["usageNote"].clone()), (Value::Null, json!("Not read yet.")));
    let history = h
        .ok("{ resourceHistory(range: DAY) { bucketSeconds series { resource } } }", json!({}))
        .await;
    assert_eq!(history["resourceHistory"], json!({"bucketSeconds": 432, "series": []}));
}

#[tokio::test]
async fn checks_bios_files_per_system() {
    let mut h = Harness::new("bios", false).await;
    h.sign_up_admin().await;
    let bios = h.dir.join("bios");
    assert!(bios.join("ps2").is_dir() && bios.join("snes").is_dir(), "the recommended layout");
    // A PCSX-ReARMed BIOS in the wrong case, and a Neo Geo set beside the games.
    std::fs::write(bios.join("psx/SCPH1001.BIN"), vec![0u8; 512 * 1024]).unwrap();
    std::fs::create_dir_all(h.dir.join("roms/neogeo")).unwrap();
    std::fs::write(h.dir.join("roms/neogeo/neogeo.zip"), b"zip").unwrap();
    // Only systems with games are listed: give each one checked here a game.
    for game in ["snes/A.sfc", "ps2/A.iso", "psx/A.chd", "neogeo/mslug.zip", "lynx/A.lnx"] {
        let path = h.dir.join("roms").join(game);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"game").unwrap();
    }
    h.ok("mutation { scanLibrary { found } }", json!({})).await;

    let query = "{ systems { id biosMissing bios { state folder groups { importance anyOne \
                 satisfied files { name state foundAt } } } } }";
    let systems = h.ok(query, json!({})).await["systems"].clone();
    let get =
        |id: &str| systems.as_array().unwrap().iter().find(|s| s["id"] == id).unwrap().clone();
    assert_eq!(get("snes")["bios"], Value::Null);
    assert_eq!(get("snes")["biosMissing"], false);
    assert_eq!(get("ps2")["biosMissing"], true);
    assert_eq!(get("ps2")["bios"]["state"], "MISSING");
    assert_eq!(get("neogeo")["biosMissing"], false, "found beside the games");
    assert_eq!(get("neogeo")["bios"]["state"], "WARNING", "not the known version");
    let psx = get("psx")["bios"].clone();
    assert_eq!(psx["state"], "WARNING", "used, though not a known dump");
    assert_eq!(psx["folder"], "psx");
    assert_eq!(psx["groups"][0]["importance"], "RECOMMENDED");
    assert_eq!(psx["groups"][0]["anyOne"], true);
    let files = psx["groups"][0]["files"].as_array().unwrap();
    let scph1001 = files.iter().find(|f| f["name"] == "scph1001.bin").unwrap();
    assert_eq!(scph1001["state"], "UNVERIFIED", "found by its name in any case");
    assert_eq!(scph1001["foundAt"], "psx/SCPH1001.BIN");
    let lynx = get("lynx");
    assert_eq!(lynx["biosMissing"], true, "Handy needs its boot ROM");
    assert_eq!(lynx["bios"]["groups"][0]["files"][0]["name"], "lynxboot.img");
    assert!(systems.as_array().unwrap().iter().all(|s| s["id"] != "dreamcast"), "no games");

    // Any layout: Batocera keeps it at the top of its BIOS folder.
    std::fs::write(bios.join("lynxboot.img"), vec![0u8; 512]).unwrap();
    let systems = h.ok(query, json!({})).await["systems"].clone();
    let lynx = systems.as_array().unwrap().iter().find(|s| s["id"] == "lynx").unwrap().clone();
    assert_eq!(lynx["biosMissing"], false);
    assert_eq!(lynx["bios"]["groups"][0]["files"][0]["foundAt"], "lynxboot.img");
}

#[tokio::test]
async fn the_admin_comes_from_the_environment() {
    let admin = |pw: &str| {
        vec![
            "--admin-username".to_owned(),
            "boss".to_owned(),
            "--admin-password".to_owned(),
            pw.to_owned(),
        ]
    };
    let mut h = Harness::new_with("env-admin", false, admin("first-password")).await;
    let viewer = h.ok(VIEWER, json!({})).await["viewer"].clone();
    assert_eq!(viewer["setupRequired"], false, "no first-run page when the admin is configured");
    assert_eq!(h.login("boss", "first-password").await.unwrap()["isAdmin"], true);

    // Restarting with a new password rotates it and signs existing browsers out.
    let no_builtin = h.dir.join("no-builtin").display().to_string();
    let config = Config::parse_from(
        [
            "sovemu-server",
            "--data-dir",
            &h.dir.display().to_string(),
            "--decorations-builtin",
            &no_builtin,
            "--public-url",
            "http://sovemu.test",
        ]
        .into_iter()
        .map(str::to_owned)
        .chain(admin("second-password")),
    );
    h.state = build_state(config).await.unwrap();
    assert_eq!(h.err(SYSTEMS, json!({})).await.0, "UNAUTHENTICATED", "old session gone");
    h.sign_out();
    assert_eq!(h.login("boss", "first-password").await, Err("invalid_grant".into()));
    assert!(h.login("boss", "second-password").await.is_ok());
}

#[tokio::test]
async fn admins_manage_accounts_but_keep_an_admin_and_leave_declared_ones_alone() {
    const USERS: &str = "{ users { id username role declared } }";
    const ROLE: &str =
        "mutation($u: Int!, $r: Role!) { setUserRole(userId: $u, role: $r) { username role } }";
    const PASSWORD: &str =
        "mutation($u: Int!, $p: String!) { setUserPassword(userId: $u, password: $p) }";
    const REMOVE: &str = "mutation($u: Int!) { removeUser(userId: $u) }";
    let mut h = Harness::new("manage-users", false).await;
    h.sign_up_admin().await;
    let me = h.ok("{ viewer { user { id } } }", json!({})).await["viewer"]["user"]["id"]
        .as_i64()
        .unwrap();
    let add = "mutation { addUser(input: {username: \"kid\", password: \"password123\"}) { id } }";
    let kid = h.ok(add, json!({})).await["addUser"]["id"].as_i64().unwrap();

    // The only admin can be neither demoted nor removed, and nobody removes themselves.
    let (code, message) = h.err(ROLE, json!({"u": me, "r": "VIEWER"})).await;
    assert_eq!(code, "CONFLICT");
    assert!(message.contains("only admin"), "{message}");
    assert_eq!(h.err(REMOVE, json!({"u": me})).await.0, "CONFLICT");
    assert_eq!(h.err(REMOVE, json!({"u": 9999})).await.0, "NOT_FOUND");

    // Roles change both ways; with two admins, one may become a viewer.
    let promoted = h.ok(ROLE, json!({"u": kid, "r": "ADMIN"})).await["setUserRole"].clone();
    assert_eq!(promoted, json!({"username": "kid", "role": "ADMIN"}));
    h.ok(ROLE, json!({"u": kid, "r": "VIEWER"})).await;

    // A new password signs the user out everywhere: their token stops working.
    h.sign_out();
    h.login("kid", "password123").await.unwrap();
    let kids_token = h.token.clone();
    h.sign_out();
    h.login("admin", "password123").await.unwrap();
    assert_eq!(h.err(PASSWORD, json!({"u": kid, "p": "short"})).await.0, "BAD_REQUEST");
    assert_eq!(
        h.ok(PASSWORD, json!({"u": kid, "p": "a-new-password"})).await["setUserPassword"],
        true
    );
    let admins_token = std::mem::replace(&mut h.token, kids_token);
    assert_eq!(h.err("{ collections { id } }", json!({})).await.0, "UNAUTHENTICATED");
    h.token = admins_token;
    h.sign_out();
    assert!(h.login("kid", "password123").await.is_err(), "the old password is gone");
    h.login("kid", "a-new-password").await.unwrap();
    h.sign_out();
    h.login("admin", "password123").await.unwrap();

    // An account the environment declares is locked here: it would be undone at the next start.
    let mut config = (*h.state.config).clone();
    config.users = vec![crate::structured::UserSpec {
        username: "Kid".into(),
        password: "a-new-password".into(),
        role: crate::auth::Role::Viewer,
    }];
    h.state.config = std::sync::Arc::new(config);
    let listed = h.ok(USERS, json!({})).await["users"].clone();
    assert!(
        listed
            .as_array()
            .unwrap()
            .contains(&json!({"id": kid, "username": "kid", "role": "VIEWER", "declared": true}))
    );
    for (doc, vars) in [
        (ROLE, json!({"u": kid, "r": "ADMIN"})),
        (PASSWORD, json!({"u": kid, "p": "another-password"})),
        (REMOVE, json!({"u": kid})),
    ] {
        let (code, message) = h.err(doc, vars).await;
        assert_eq!(code, "CONFLICT", "{doc}");
        assert!(message.contains("declared in the environment"), "{message}");
    }
    assert_eq!(
        h.ok("mutation($u: Int!) { signOutEverywhere(userId: $u) }", json!({"u": kid})).await["signOutEverywhere"],
        true,
        "changes nothing the environment sets"
    );

    // Not declared any more: removed, with everything of theirs.
    let mut config = (*h.state.config).clone();
    config.users.clear();
    h.state.config = std::sync::Arc::new(config);
    assert_eq!(h.ok(REMOVE, json!({"u": kid})).await["removeUser"], true);
    let names: Vec<String> = h.ok(USERS, json!({})).await["users"]
        .as_array()
        .unwrap()
        .iter()
        .map(|u| u["username"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(names, ["admin"]);
}

#[tokio::test]
async fn the_activity_log_keeps_sign_ins_and_admin_changes() {
    const PAGE: &str = "query($u: Int, $c: ActivityCategory, $a: String, $l: Int!) {
        activity(userId: $u, category: $c, after: $a, limit: $l) {
            entries { kind category username subject detail address } next } }";
    let mut h = Harness::new("activity", false).await;
    h.sign_up_admin().await;
    h.sign_out();
    assert!(h.login("admin", "not-the-password").await.is_err());
    h.login("admin", "password123").await.unwrap();
    let add = "mutation { addUser(input: {username: \"kid\", password: \"password123\"}) { id } }";
    h.ok(add, json!({})).await;

    // Recorded in the background: wait for the four entries, not for a time.
    let all = json!({"l": 50});
    let mut entries = Value::Null;
    for _ in 0..100 {
        entries = h.ok(PAGE, all.clone()).await["activity"]["entries"].clone();
        if entries.as_array().unwrap().len() >= 4 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let kinds: Vec<&str> =
        entries.as_array().unwrap().iter().map(|e| e["kind"].as_str().unwrap()).collect();
    // Newest first; the first sign-in is the harness's own, setting the admin up.
    assert_eq!(kinds, ["USER_ADDED", "SIGNED_IN", "SIGN_IN_FAILED", "SIGNED_IN"]);
    assert_eq!(
        entries[0],
        json!({"kind": "USER_ADDED", "category": "ADMIN", "username": "admin", "subject": "kid",
               "detail": "viewer", "address": null})
    );
    // (The harness's requests have no peer address; `activity`'s own tests keep one.)
    assert_eq!(entries[2]["username"], "admin", "a refused sign-in keeps the name tried");
    assert_eq!(entries[2]["category"], "SIGN_IN");

    // By category, and a page at a time.
    let sign_ins =
        h.ok(PAGE, json!({"c": "SIGN_IN", "l": 50})).await["activity"]["entries"].clone();
    assert_eq!(sign_ins.as_array().unwrap().len(), 3);
    let first = h.ok(PAGE, json!({"l": 1})).await["activity"].clone();
    assert_eq!(first["entries"][0]["kind"], "USER_ADDED");
    let next = first["next"].as_str().unwrap().to_owned();
    let second = h.ok(PAGE, json!({"a": next, "l": 1})).await["activity"].clone();
    assert_eq!(second["entries"][0]["kind"], "SIGNED_IN");
    assert_eq!(h.err(PAGE, json!({"a": "nonsense", "l": 1})).await.0, "BAD_REQUEST");
}

#[tokio::test]
async fn admins_see_each_sign_in_and_end_one() {
    let mut h = Harness::new("sign-ins", false).await;
    h.sign_up_admin().await;
    h.switch_to_new_viewer("kid").await;
    let kids_token = h.token.clone();
    let kid = h.ok("{ viewer { user { id } } }", json!({})).await["viewer"]["user"]["id"]
        .as_i64()
        .unwrap();
    // The kid's app names its device, as an app does once signed in.
    let name = "mutation($n: String!) { nameDevice(name: $n) }";
    h.ok(name, json!({"n": "  SovEmu for Android 1.4.0 ·\n Mi TV "})).await;
    assert_eq!(h.err(name, json!({"n": " "})).await.0, "BAD_REQUEST");
    h.sign_out();
    assert_eq!(h.err(name, json!({"n": "x"})).await.0, "UNAUTHENTICATED");
    h.login("admin", "password123").await.unwrap();

    // Every sign-in, the admin's own among them; the kid's is the app's (the password grant).
    let all = h
        .ok("{ signIns { id username client signedInAt lastUsedAt device } }", json!({}))
        .await["signIns"]
        .clone();
    let kids: Vec<&Value> =
        all.as_array().unwrap().iter().filter(|s| s["username"] == "kid").collect();
    assert_eq!(kids.len(), 1, "{all}");
    assert_eq!(kids[0]["client"], "APP");
    assert_eq!(kids[0]["device"], "SovEmu for Android 1.4.0 · Mi TV");
    assert!(kids[0]["lastUsedAt"].as_f64() >= kids[0]["signedInAt"].as_f64());
    let id = kids[0]["id"].as_str().unwrap().to_owned();

    // The kid's page: their sign-in, nothing played or saved yet.
    let page = h
        .ok(
            "query($u: Int!) { user(id: $u) { user { username } joinedAt signIns { id } gamesPlayed starts lastPlayedAt saveBytes saveFiles } }",
            json!({"u": kid}),
        )
        .await["user"]
        .clone();
    assert_eq!(page["user"]["username"], "kid");
    assert!(page["joinedAt"].as_str().is_some_and(|j| j.len() == 19), "{page}");
    assert_eq!(page["signIns"], json!([{"id": id}]));
    assert_eq!(
        (page["gamesPlayed"].clone(), page["starts"].clone(), page["lastPlayedAt"].clone()),
        (json!(0), json!(0), Value::Null)
    );
    assert_eq!((page["saveBytes"].clone(), page["saveFiles"].clone()), (json!(0.0), json!(0)));
    assert_eq!(h.err("{ user(id: 9999) { joinedAt } }", json!({})).await.0, "NOT_FOUND");

    // Ending it signs that app out; the admin's own sign-in stands.
    let end = "mutation($id: ID!) { endSignIn(id: $id) }";
    assert_eq!(h.ok(end, json!({"id": id})).await["endSignIn"], true);
    assert_eq!(h.err(end, json!({"id": id})).await.0, "NOT_FOUND", "already ended");
    let admins = std::mem::replace(&mut h.token, kids_token);
    assert_eq!(h.err("{ collections { id } }", json!({})).await.0, "UNAUTHENTICATED");
    h.token = admins;
    let left = h.ok("{ signIns { username } }", json!({})).await["signIns"].clone();
    assert!(left.as_array().unwrap().iter().all(|s| s["username"] != "kid"), "{left}");

    // The activity log names whose sign-in it was (recorded in the background: wait for it).
    let ended = "{ activity(category: ADMIN) { entries { kind subject detail } } }";
    let mut found = Value::Null;
    for _ in 0..100 {
        found = h.ok(ended, json!({})).await["activity"]["entries"][0].clone();
        if found["kind"] == "SIGN_IN_ENDED" {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(
        found,
        json!({"kind": "SIGN_IN_ENDED", "subject": "kid", "detail": "sovemu-native"})
    );
}

#[tokio::test]
async fn a_game_played_counts_in_the_play_statistics() {
    const STATS: &str = "{ playStatistics(range: WEEK) { totals { sessions players games }
        days { sessions } games { gameId title system sessions players }
        players { username sessions games } systems { system sessions } } }";
    let mut h = Harness::new("play-stats", true).await;
    h.sign_up_admin().await;
    let empty = h.ok(STATS, json!({})).await["playStatistics"].clone();
    assert_eq!(empty["totals"], json!({"sessions": 0, "players": 0, "games": 0}));
    let game = h.games("nes").await[0]["id"].as_i64().unwrap();
    let sid =
        h.ok(START, json!({"game": game})).await["startSession"]["id"].as_str().unwrap().to_owned();
    h.ok("mutation($s: ID!) { stopSession(id: $s) }", json!({"s": sid})).await;

    // Written as the worker is reaped, in the background: wait for it.
    let mut stats = Value::Null;
    for _ in 0..200 {
        stats = h.ok(STATS, json!({})).await["playStatistics"].clone();
        if stats["totals"]["sessions"] == 1 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(stats["totals"], json!({"sessions": 1, "players": 1, "games": 1}), "{stats}");
    assert_eq!(stats["days"], json!([{"sessions": 1}]));
    assert_eq!(
        stats["games"],
        json!([{"gameId": game, "title": "Test Game", "system": "nes", "sessions": 1, "players": 1}])
    );
    assert_eq!(stats["players"], json!([{"username": "admin", "sessions": 1, "games": 1}]));
    assert_eq!(stats["systems"], json!([{"system": "nes", "sessions": 1}]));
}

#[tokio::test]
async fn admins_read_and_follow_the_log() {
    use sovemu_sessions::Observer;
    const LOGS: &str = "query($a: Float, $l: LogLevel, $s: String, $t: String) {
        logs(after: $a, level: $l, session: $s, text: $t) {
            enabled lines { level target message session } cursor } }";
    let mut h = Harness::new("logs", false).await;
    h.sign_up_admin().await;
    // Off (`SOVEMU_LOG_BUFFER_LINES=0`, and in this harness): the page says so.
    let off = h.ok(LOGS, json!({})).await["logs"].clone();
    assert_eq!((off["enabled"].clone(), off["lines"].clone()), (json!(false), json!([])));

    let buffer = std::sync::Arc::new(crate::logs::LogBuffer::new(100));
    h.state.logs = Some(buffer.clone());
    buffer.worker_line("s1", "2026-11-22T12:00:00+00:00  INFO sovemu_worker: the game loaded");
    buffer.worker_line("s2", "2026-11-22T12:00:01+00:00 ERROR sovemu_worker: the core crashed");
    let page = h.ok(LOGS, json!({})).await["logs"].clone();
    assert_eq!(page["enabled"], true);
    assert_eq!(page["lines"].as_array().unwrap().len(), 2);
    assert_eq!(page["lines"][1]["level"], "ERROR");
    let errors = h.ok(LOGS, json!({"l": "WARN"})).await["logs"]["lines"].clone();
    assert_eq!(
        errors,
        json!([{"level": "ERROR", "target": "worker",
        "message": "2026-11-22T12:00:01+00:00 ERROR sovemu_worker: the core crashed", "session": "s2"}])
    );
    assert_eq!(h.ok(LOGS, json!({"s": "s1"})).await["logs"]["lines"].as_array().unwrap().len(), 1);
    assert_eq!(
        h.ok(LOGS, json!({"t": "CRASHED"})).await["logs"]["lines"].as_array().unwrap().len(),
        1
    );

    // Following: only what came after the cursor.
    let cursor = page["cursor"].as_f64().unwrap();
    buffer.worker_line("s1", "INFO a third line");
    let next = h.ok(LOGS, json!({"a": cursor})).await["logs"]["lines"].clone();
    assert_eq!(next.as_array().unwrap().len(), 1);
    assert_eq!(next[0]["message"], "INFO a third line");
}

#[tokio::test]
async fn declared_users_are_reconciled_and_pruned() {
    let mut h = Harness::new("env-users", false).await;
    h.sign_up_admin().await;
    let dir = h.dir.display().to_string();
    let no_builtin = h.dir.join("no-builtin").display().to_string();
    let restart = |users: Vec<crate::structured::UserSpec>, prune: bool| {
        let mut config = Config::parse_from([
            "sovemu-server",
            "--data-dir",
            &dir,
            "--decorations-builtin",
            &no_builtin,
            "--public-url",
            "http://sovemu.test",
        ]);
        config.users = users;
        config.users_prune = prune;
        config
    };
    let user = |name: &str, pw: &str, admin: bool| crate::structured::UserSpec {
        username: name.into(),
        password: pw.into(),
        role: if admin { crate::auth::Role::Admin } else { crate::auth::Role::Viewer },
    };
    // Declared users are created; the hand-made admin stays without pruning.
    let config =
        restart(vec![user("mum", "password-mum", true), user("kid", "password-kid", false)], false);
    h.state = build_state(config).await.unwrap();
    h.sign_out();
    assert_eq!(h.login("kid", "password-kid").await.unwrap()["isAdmin"], false);
    let names = |state: &AppState| {
        let db = state.db.clone();
        async move {
            sqlx::query_scalar::<_, String>("SELECT username FROM users ORDER BY username")
                .fetch_all(&db)
                .await
                .unwrap()
        }
    };
    assert_eq!(names(&h.state).await.len(), 3);

    // Pruning leaves exactly the declared users; kid is now an admin.
    let config =
        restart(vec![user("mum", "password-mum", true), user("kid", "password-kid", true)], true);
    h.state = build_state(config).await.unwrap();
    assert_eq!(names(&h.state).await, ["kid", "mum"]);
    let role: String = sqlx::query_scalar("SELECT role FROM users WHERE username = 'kid'")
        .fetch_one(&h.state.db)
        .await
        .unwrap();
    assert_eq!(role, "admin");

    // Pruning with nobody declared is refused rather than deleting everyone.
    assert!(build_state(restart(Vec::new(), true)).await.is_err());
}

#[tokio::test]
async fn system_overrides_hide_systems_and_choose_cores() {
    let mut h = Harness::new_with("env-systems", false, Vec::new()).await;
    h.sign_up_admin().await;
    let mut config = (*h.state.config).clone();
    config.systems = crate::structured::systems(&[
        ("SOVEMU_SYSTEMS__gba__ENABLED".to_owned(), "false".to_owned()),
        ("SOVEMU_SYSTEMS__nes__CORE".to_owned(), "nestopia".to_owned()),
    ])
    .unwrap();
    h.state.config = std::sync::Arc::new(config);
    let systems = h.systems().await;
    let ids: Vec<&str> =
        systems.as_array().unwrap().iter().map(|s| s["id"].as_str().unwrap()).collect();
    assert!(!ids.contains(&"gba"), "disabled systems are hidden");
    assert_eq!(h.ok(GAMES, json!({"id": "gba"})).await["system"], Value::Null);
    let home = h.ok("{ home { added { system } } }", json!({})).await;
    assert_eq!(
        home["home"]["added"],
        json!([{"system": "nes"}]),
        "hidden systems stay off the home screen"
    );

    // NES now needs nestopia: an fceumm core does not count.
    let cores = h.state.config.cores_dir.clone();
    std::fs::write(cores.join("fceumm_libretro.so"), b"").unwrap();
    assert_eq!(system(&h.systems().await, "nes")["playable"], false);
    std::fs::write(cores.join("nestopia_libretro.so"), b"").unwrap();
    assert_eq!(system(&h.systems().await, "nes")["playable"], true);
}

#[tokio::test]
async fn admins_change_emulator_settings() {
    let mut h = Harness::new("settings", true).await;
    h.sign_up_admin().await;
    let get = "query($s: ID!) { emulatorSettings(system: $s) {
        core environment settings { key choices labels value source info category } } }";
    let change = "mutation($s: ID!, $c: [SettingChange!]!) { changeEmulatorSettings(system: $s, changes: $c) {
        environment settings { key value source } } }";
    let setting = |body: &Value, key: &str| {
        body["settings"].as_array().unwrap().iter().find(|s| s["key"] == key).unwrap().clone()
    };

    // What the core declares, probed in a worker.
    let body = h.ok(get, json!({"s": "nes"})).await["emulatorSettings"].clone();
    assert_eq!(body["core"], "SovEmu Test Core 1.0.0");
    let pattern = setting(&body, "sovemu_test_pattern");
    assert_eq!(pattern["choices"], json!(["gradient", "solid", "noise"]));
    assert_eq!(pattern["labels"], json!(["Gradient", "Solid colour", "Noise"]));
    assert_eq!(pattern["category"], "video");
    assert_eq!(
        (pattern["value"].clone(), pattern["source"].clone()),
        (json!("gradient"), json!("CORE"))
    );

    // Changed, kept, offered as an environment line, and reset.
    let to = |v: Value| json!({"s": "nes", "c": [{"key": "sovemu_test_pattern", "value": v}]});
    let body = h.ok(change, to(json!("solid"))).await["changeEmulatorSettings"].clone();
    let pattern = setting(&body, "sovemu_test_pattern");
    assert_eq!(
        (pattern["value"].clone(), pattern["source"].clone()),
        (json!("solid"), json!("ADMIN"))
    );
    assert_eq!(
        body["environment"],
        json!(["SOVEMU_SYSTEMS__nes__OPTIONS__sovemu_test_pattern=solid"])
    );
    assert_eq!(h.err(change, to(json!("plaid"))).await.0, "BAD_REQUEST");
    let unknown = json!({"s": "nes", "c": [{"key": "no_such_option", "value": "x"}]});
    assert_eq!(h.err(change, unknown).await.0, "BAD_REQUEST");
    let body = h.ok(change, to(Value::Null)).await["changeEmulatorSettings"].clone();
    assert_eq!(setting(&body, "sovemu_test_pattern")["source"], "CORE");

    // The environment wins and is locked.
    let mut config = (*h.state.config).clone();
    config.systems = crate::structured::systems(&[(
        "SOVEMU_SYSTEMS__nes__OPTIONS__sovemu_test_pattern".to_owned(),
        "solid".to_owned(),
    )])
    .unwrap();
    h.state.config = std::sync::Arc::new(config);
    let body = h.ok(get, json!({"s": "nes"})).await["emulatorSettings"].clone();
    assert_eq!(setting(&body, "sovemu_test_pattern")["source"], "ENVIRONMENT");
    assert_eq!(h.err(change, to(json!("gradient"))).await.0, "CONFLICT");

    // Admins only.
    h.sign_out();
    assert_eq!(h.err(get, json!({"s": "nes"})).await.0, "UNAUTHENTICATED");
}

#[tokio::test]
async fn run_ahead_is_on_by_default_admins_change_it_and_the_environment_locks_it() {
    let mut h = Harness::new("runahead", true).await;
    h.sign_up_admin().await;
    let get = "query($s: ID!) { emulatorSettings(system: $s) {
        environment runAhead { frames max source supported } } }";
    let set = "mutation($s: ID!, $f: Int) { setRunAhead(system: $s, frames: $f) {
        environment runAhead { frames source } } }";

    // One frame until someone changes it.
    let body = h.ok(get, json!({"s": "nes"})).await["emulatorSettings"].clone();
    assert_eq!(
        body["runAhead"],
        json!({"frames": 1, "max": 4, "source": "SOVEMU", "supported": true})
    );

    // Changed, offered as an environment line, refused where it cannot go, turned off, and
    // back to the default.
    let body = h.ok(set, json!({"s": "nes", "f": 2})).await["setRunAhead"].clone();
    assert_eq!(body["runAhead"], json!({"frames": 2, "source": "ADMIN"}));
    assert_eq!(body["environment"], json!(["SOVEMU_SYSTEMS__nes__RUN_AHEAD=2"]));
    assert_eq!(h.err(set, json!({"s": "nes", "f": 5})).await.0, "BAD_REQUEST");
    assert_eq!(h.err(set, json!({"s": "nes", "f": -1})).await.0, "BAD_REQUEST");
    assert_eq!(h.err(set, json!({"s": "n64", "f": 1})).await.0, "BAD_REQUEST");
    let body = h.ok(set, json!({"s": "nes", "f": 0})).await["setRunAhead"].clone();
    assert_eq!(body["runAhead"], json!({"frames": 0, "source": "ADMIN"}));
    assert_eq!(body["environment"], json!(["SOVEMU_SYSTEMS__nes__RUN_AHEAD=0"]));
    let body = h.ok(set, json!({"s": "nes", "f": null})).await["setRunAhead"].clone();
    assert_eq!(body["runAhead"], json!({"frames": 1, "source": "SOVEMU"}));
    assert_eq!(body["environment"], json!([]));

    // The environment wins and is locked.
    let mut config = (*h.state.config).clone();
    config.systems = crate::structured::systems(&[(
        "SOVEMU_SYSTEMS__nes__RUN_AHEAD".to_owned(),
        "3".to_owned(),
    )])
    .unwrap();
    h.state.config = std::sync::Arc::new(config);
    let body = h.ok(get, json!({"s": "nes"})).await["emulatorSettings"].clone();
    assert_eq!(body["runAhead"]["frames"], 3);
    assert_eq!(body["runAhead"]["source"], "ENVIRONMENT");
    assert_eq!(h.err(set, json!({"s": "nes", "f": 2})).await.0, "CONFLICT");
}

#[tokio::test]
async fn client_events_are_kept_for_signed_in_users_within_their_allowance() {
    const REPORT: &str = "mutation($c: String!, $e: [ClientEventInput!]!) {
        reportClientEvents(client: $c, events: $e) }";
    let mut h = Harness::new("client-events", false).await;
    let one = json!([{"kind": "CONNECTION_FAILED", "message": "ICE failed", "sessionId": "abc", "at": 1.5e9}]);
    assert_eq!(h.err(REPORT, json!({"c": "web", "e": one})).await.0, "UNAUTHENTICATED");
    h.sign_up_admin().await;
    assert_eq!(h.ok(REPORT, json!({"c": "web", "e": one})).await["reportClientEvents"], 1);
    let many: Vec<Value> = (0..30).map(|_| json!({"kind": "OTHER", "message": "x"})).collect();
    assert_eq!(h.ok(REPORT, json!({"c": "web", "e": many})).await["reportClientEvents"], 20);

    // The admin reads them back, grouped, then one group's events.
    let groups = h
        .ok("{ clientEventGroups { kind client count users lastMessage } }", json!({}))
        .await["clientEventGroups"]
        .clone();
    assert_eq!(
        groups,
        json!([
            {"kind": "OTHER", "client": "web", "count": 20, "users": 1, "lastMessage": "x"},
            {"kind": "CONNECTION_FAILED", "client": "web", "count": 1, "users": 1, "lastMessage": "ICE failed"},
        ])
    );
    let failed = "{ clientEvents(kind: CONNECTION_FAILED, client: \"web\") { username message sessionId gameId clientAt } }";
    assert_eq!(
        h.ok(failed, json!({})).await["clientEvents"],
        json!([{"username": "admin", "message": "ICE failed", "sessionId": "abc", "gameId": null, "clientAt": 1.5e9}]),
        "no game: session abc is not running"
    );
    assert_eq!(h.err("{ clientEventGroups(days: 0) { count } }", json!({})).await.0, "BAD_REQUEST");

    // Off, they are accepted and dropped: clients need no special case.
    let mut config = (*h.state.config).clone();
    config.client_events = false;
    h.state.config = std::sync::Arc::new(config);
    assert_eq!(h.ok(REPORT, json!({"c": "web", "e": one})).await["reportClientEvents"], 0);
    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM client_events")
        .fetch_one(&h.state.db)
        .await
        .unwrap();
    assert_eq!(total, 21, "nothing kept while off");
}

#[tokio::test]
async fn controller_mappings_are_kept_per_user_and_model() {
    const LIST: &str = "{ controllerMappings { controller name bindings { input button } } }";
    const SAVE: &str = "mutation($c: String!, $n: String!, $b: [BindingInput!]!) {
        saveControllerMapping(controller: $c, name: $n, bindings: $b) { controller name } }";
    const RESET: &str = "mutation($c: String!) { resetControllerMapping(controller: $c) }";
    let mut h = Harness::new("controllers", false).await;
    assert_eq!(h.err(LIST, json!({})).await.0, "UNAUTHENTICATED");
    h.sign_up_admin().await;
    assert_eq!(h.ok(LIST, json!({})).await["controllerMappings"], json!([]));

    let swapped = json!([{"input": "south", "button": "A"}, {"input": "east", "button": "B"}]);
    let saved =
        h.ok(SAVE, json!({"c": "xbox", "n": " Xbox Wireless Controller ", "b": swapped})).await;
    assert_eq!(saved["saveControllerMapping"]["name"], "Xbox Wireless Controller");
    // Saving again replaces the mapping rather than adding to it.
    let keys = json!([{"input": "KeyJ", "button": "B"}]);
    h.ok(SAVE, json!({"c": "keyboard", "n": "", "b": keys})).await;
    h.ok(SAVE, json!({"c": "keyboard", "n": "", "b": [{"input": "KeyK", "button": "B"}]})).await;
    assert_eq!(
        h.ok(LIST, json!({})).await["controllerMappings"],
        json!([
            {"controller": "keyboard", "name": "", "bindings": [{"input": "KeyK", "button": "B"}]},
            {"controller": "xbox", "name": "Xbox Wireless Controller", "bindings": swapped},
        ])
    );

    let bad = json!({"c": "xbox", "n": "", "b": [{"input": "KeyZ", "button": "B"}]});
    let (code, message) = h.err(SAVE, bad).await;
    assert_eq!(code, "BAD_REQUEST", "{message}");
    let tab = json!({"c": "keyboard", "n": "", "b": [{"input": "Tab", "button": "B"}]});
    assert_eq!(h.err(SAVE, tab).await.0, "BAD_REQUEST", "the fast-forward key");

    assert_eq!(h.ok(RESET, json!({"c": "xbox"})).await["resetControllerMapping"], true);
    assert_eq!(h.ok(RESET, json!({"c": "xbox"})).await["resetControllerMapping"], false);
    assert_eq!(h.ok(LIST, json!({})).await["controllerMappings"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn controller_defaults_come_from_the_environment_then_the_admin() {
    const LIST: &str = "{ controllerMappings { controller source bindings { input button } } }";
    const DEFAULTS: &str = "{ controllerDefaults { mappings { controller source } environment } }";
    const SET_DEFAULT: &str = "mutation($c: String!, $n: String!, $b: [BindingInput!]!) {
        saveControllerDefault(controller: $c, name: $n, bindings: $b) { source } }";
    const RESET_DEFAULT: &str = "mutation($c: String!) { resetControllerDefault(controller: $c) }";
    const SAVE: &str = "mutation($c: String!, $b: [BindingInput!]!) {
        saveControllerMapping(controller: $c, bindings: $b) { source } }";
    const RESET: &str = "mutation($c: String!) { resetControllerMapping(controller: $c) }";
    let mut h = Harness::new_with("controller-defaults", false, Vec::new()).await;
    h.sign_up_admin().await;
    let mut config = (*h.state.config).clone();
    config.controllers = crate::structured::controllers(&[
        ("SOVEMU_CONTROLLERS__0__ID".to_owned(), "dualsense".to_owned()),
        ("SOVEMU_CONTROLLERS__0__BINDINGS".to_owned(), "south:A,east:B".to_owned()),
    ])
    .unwrap();
    h.state.config = std::sync::Arc::new(config);

    // The admin sets a default; the environment's cannot be changed.
    let swapped = json!([{"input": "south", "button": "A"}]);
    let set = h.ok(SET_DEFAULT, json!({"c": "xbox", "n": "Xbox", "b": swapped})).await;
    assert_eq!(set["saveControllerDefault"]["source"], "ADMIN");
    let locked = json!({"c": "dualsense", "n": "", "b": swapped});
    assert_eq!(h.err(SET_DEFAULT, locked).await.0, "CONFLICT");
    let defaults = h.ok(DEFAULTS, json!({})).await["controllerDefaults"].clone();
    assert_eq!(
        defaults["mappings"],
        json!([
            {"controller": "dualsense", "source": "ENVIRONMENT"},
            {"controller": "xbox", "source": "ADMIN"},
        ])
    );
    assert_eq!(
        defaults["environment"],
        json!([
            "SOVEMU_CONTROLLERS__1__ID=xbox",
            "SOVEMU_CONTROLLERS__1__NAME=Xbox",
            "SOVEMU_CONTROLLERS__1__BINDINGS=south:A",
        ]),
        "numbered after the environment's own"
    );

    // A player gets the defaults until they save their own, and gets them back on reset.
    let add = "mutation { addUser(input: {username: \"kid\", password: \"password123\"}) { id } }";
    h.ok(add, json!({})).await;
    h.sign_out();
    h.login("kid", "password123").await.unwrap();
    assert_eq!(h.err(DEFAULTS, json!({})).await.0, "FORBIDDEN");
    assert_eq!(h.err(SET_DEFAULT, json!({"c": "xbox", "n": "", "b": []})).await.0, "FORBIDDEN");
    let sources = |list: &Value| {
        list["controllerMappings"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| {
                format!("{}={}", m["controller"].as_str().unwrap(), m["source"].as_str().unwrap())
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(sources(&h.ok(LIST, json!({})).await), ["dualsense=ENVIRONMENT", "xbox=ADMIN"]);
    h.ok(SAVE, json!({"c": "xbox", "b": [{"input": "east", "button": "A"}]})).await;
    assert_eq!(sources(&h.ok(LIST, json!({})).await), ["dualsense=ENVIRONMENT", "xbox=USER"]);
    h.ok(RESET, json!({"c": "xbox"})).await;
    assert_eq!(sources(&h.ok(LIST, json!({})).await), ["dualsense=ENVIRONMENT", "xbox=ADMIN"]);

    // The admin forgets their default: the model is back on the built-in one (no mapping).
    h.sign_out();
    h.login("admin", "password123").await.unwrap();
    assert_eq!(h.ok(RESET_DEFAULT, json!({"c": "xbox"})).await["resetControllerDefault"], true);
    assert_eq!(sources(&h.ok(LIST, json!({})).await), ["dualsense=ENVIRONMENT"]);
}

#[tokio::test]
async fn controller_mappings_are_kept_per_system_too() {
    const LIST: &str = "query($s: ID) { controllerMappings(system: $s) {
        controller system source bindings { input button } } }";
    const SAVE: &str = "mutation($c: String!, $s: ID, $b: [BindingInput!]!) {
        saveControllerMapping(controller: $c, system: $s, bindings: $b) { system } }";
    const RESET: &str =
        "mutation($c: String!, $s: ID) { resetControllerMapping(controller: $c, system: $s) }";
    const SET_DEFAULT: &str = "mutation($c: String!, $s: ID, $b: [BindingInput!]!) {
        saveControllerDefault(controller: $c, system: $s, bindings: $b) { system } }";
    const DEFAULTS: &str = "{ controllerDefaults { environment } }";
    let mut h = Harness::new_with("controllers-per-system", false, Vec::new()).await;
    h.sign_up_admin().await;
    let one = |input: &str| json!([{"input": input, "button": "B"}]);
    // What each layer binds to B tells them apart.
    let b_of = |list: &Value| {
        let m = &list["controllerMappings"][0];
        format!(
            "{}/{}/{}",
            m["source"].as_str().unwrap_or("-"),
            m["system"].as_str().unwrap_or("all"),
            m["bindings"][0]["input"].as_str().unwrap_or("-")
        )
    };
    let md = json!({"s": "megadrive"});

    h.ok(SET_DEFAULT, json!({"c": "xbox", "b": one("north")})).await;
    assert_eq!(b_of(&h.ok(LIST, md.clone()).await), "ADMIN/all/north");
    h.ok(SET_DEFAULT, json!({"c": "xbox", "s": "megadrive", "b": one("west")})).await;
    assert_eq!(b_of(&h.ok(LIST, md.clone()).await), "ADMIN/megadrive/west");
    assert_eq!(b_of(&h.ok(LIST, json!({"s": "snes"})).await), "ADMIN/all/north", "other systems");
    // The user's own, even for every system, wins over any default.
    h.ok(SAVE, json!({"c": "xbox", "b": one("east")})).await;
    assert_eq!(b_of(&h.ok(LIST, md.clone()).await), "USER/all/east");
    h.ok(SAVE, json!({"c": "xbox", "s": "megadrive", "b": one("south")})).await;
    assert_eq!(b_of(&h.ok(LIST, md.clone()).await), "USER/megadrive/south");
    assert_eq!(b_of(&h.ok(LIST, json!({})).await), "USER/all/east", "no system: every system's");
    // Resetting the system's mapping falls back to the user's own for every system.
    h.ok(RESET, json!({"c": "xbox", "s": "megadrive"})).await;
    assert_eq!(b_of(&h.ok(LIST, md.clone()).await), "USER/all/east");

    assert_eq!(h.err(LIST, json!({"s": "atari"})).await.0, "BAD_REQUEST");
    let lines = h.ok(DEFAULTS, json!({})).await["controllerDefaults"]["environment"].clone();
    assert!(
        lines.as_array().unwrap().contains(&json!("SOVEMU_CONTROLLERS__1__SYSTEM=megadrive")),
        "{lines}"
    );
}

#[tokio::test]
async fn systems_describe_their_buttons_and_pads() {
    let mut h = Harness::new("system-controls", false).await;
    h.sign_up_admin().await;
    let data = h
        .ok(
            "{ system(id: \"nes\") { controls { buttons { button label } layout { id width keys { key label } } } }
               controllerLayouts { id keys { key label } } }",
            json!({}),
        )
        .await;
    let controls = &data["system"]["controls"];
    assert_eq!(controls["buttons"][4], json!({"button": "A", "label": "A"}));
    assert_eq!(controls["layout"]["id"], "nes");
    assert_eq!(controls["layout"]["keys"].as_array().unwrap().len(), 8);
    let families: Vec<&str> = data["controllerLayouts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["id"].as_str().unwrap())
        .collect();
    assert_eq!(families, ["xbox", "playstation", "nintendo", "generic"]);
}

#[tokio::test]
async fn favourites_and_collections() {
    let mut h = Harness::new("collections", false).await;
    h.sign_up_admin().await;
    let id = h.games("nes").await[0]["id"].as_i64().unwrap();

    // A favourite: on the game, and as Home's row.
    h.ok("mutation($id: Int!) { setFavourite(gameId: $id, favourite: true) }", json!({"id": id}))
        .await;
    let game = h.ok("query($id: Int!) { game(id: $id) { favourite } }", json!({"id": id})).await;
    assert_eq!(game["game"]["favourite"], true);
    let home = h.ok("{ home { favourites { id favourite } } }", json!({})).await;
    assert_eq!(home["home"]["favourites"][0]["id"], id);
    h.ok("mutation($id: Int!) { setFavourite(gameId: $id, favourite: false) }", json!({"id": id}))
        .await;
    let home = h.ok("{ home { favourites { id } } }", json!({})).await;
    assert_eq!(home["home"]["favourites"], json!([]));

    // A collection: created, named once, filled, renamed, emptied, deleted.
    let made = h
        .ok(
            r#"mutation { createCollection(name: "  Co-op   night ") { id name gameCount } }"#,
            json!({}),
        )
        .await;
    assert_eq!(made["createCollection"]["name"], "Co-op night", "spaces tidied");
    let cid = made["createCollection"]["id"].as_i64().unwrap();
    let (code, _) =
        h.err(r#"mutation { createCollection(name: "Co-op night") { id } }"#, json!({})).await;
    assert_eq!(code, "CONFLICT");
    let (code, _) = h.err(r#"mutation { createCollection(name: "   ") { id } }"#, json!({})).await;
    assert_eq!(code, "BAD_REQUEST");
    h.ok(
        "mutation($c: Int!, $g: Int!) { setInCollection(collectionId: $c, gameId: $g, member: true) }",
        json!({"c": cid, "g": id}),
    )
    .await;
    let list = h.ok("{ collections { id name gameCount games { id } } }", json!({})).await;
    assert_eq!(list["collections"][0]["gameCount"], 1);
    assert_eq!(list["collections"][0]["games"][0]["id"], id);
    let game =
        h.ok("query($id: Int!) { game(id: $id) { collectionIds } }", json!({"id": id})).await;
    assert_eq!(game["game"]["collectionIds"], json!([cid]));
    h.ok(r#"mutation($c: Int!) { renameCollection(id: $c, name: "Weekend") }"#, json!({"c": cid}))
        .await;
    h.ok(
        "mutation($c: Int!, $g: Int!) { setInCollection(collectionId: $c, gameId: $g, member: false) }",
        json!({"c": cid, "g": id}),
    )
    .await;
    let list = h.ok("{ collections { name gameCount } }", json!({})).await;
    assert_eq!(list["collections"], json!([{"name": "Weekend", "gameCount": 0}]));
    h.ok("mutation($c: Int!) { deleteCollection(id: $c) }", json!({"c": cid})).await;
    assert_eq!(h.ok("{ collections { id } }", json!({})).await["collections"], json!([]));
    let (code, _) =
        h.err("mutation($c: Int!) { deleteCollection(id: $c) }", json!({"c": cid})).await;
    assert_eq!(code, "NOT_FOUND");
}

#[tokio::test]
async fn search_spans_every_system() {
    let mut h = Harness::new("search", false).await;
    h.sign_up_admin().await;
    h.games("nes").await;

    let q = r#"query($t: String!) { search(text: $t) { title system favourite } }"#;
    let found = h.ok(q, json!({"t": "  TEST  game "})).await;
    assert_eq!(
        found["search"],
        json!([{"title": "Test Game", "system": "nes", "favourite": false}])
    );
    let found = h.ok(q, json!({"t": "other"})).await;
    assert_eq!(found["search"][0]["system"], "gba", "another system's games too");
    assert_eq!(h.ok(q, json!({"t": "test zelda"})).await["search"], json!([]), "every word");
    assert_eq!(h.ok(q, json!({"t": "   "})).await["search"], json!([]), "blank");
}

#[tokio::test]
async fn admins_see_how_the_server_is_reached() {
    let mut h = Harness::new_with(
        "network",
        false,
        vec![
            "--advertise-ip".into(),
            "192.0.2.10".into(),
            "--ice-servers".into(),
            r#"[{"urls":"stun:stun.example:3478"},{"urls":["turn:t.example"],"username":"u","credential":"pa55"}]"#
                .into(),
        ],
    )
    .await;
    h.sign_up_admin().await;
    let q = "{ network { udpFirst udpLast portCount portsInUse sessions advertisedIp \
             advertisedIpDetected publicUrls publicUrlsConfigured signInAvailableHere \
             iceServers { urls hasCredential } settings { name value } } }";
    let n = h.ok(q, json!({})).await["network"].clone();
    let (first, last) = (n["udpFirst"].as_i64().unwrap(), n["udpLast"].as_i64().unwrap());
    assert_eq!(n["portCount"].as_i64(), Some(last - first + 1));
    assert_eq!((n["portsInUse"].clone(), n["sessions"].clone()), (json!(0), json!(0)));
    assert_eq!(n["advertisedIp"], "192.0.2.10");
    assert_eq!(n["advertisedIpDetected"], false);
    assert_eq!(n["publicUrlsConfigured"], true, "the harness names its public URL");
    assert_eq!(n["signInAvailableHere"], false, "the harness's requests name no host");
    assert_eq!(
        n["iceServers"],
        json!([
            {"urls": ["stun:stun.example:3478"], "hasCredential": false},
            {"urls": ["turn:t.example"], "hasCredential": true},
        ])
    );
    let ice = n["settings"].as_array().unwrap().iter().find(|s| s["name"] == "SOVEMU_ICE_SERVERS");
    assert!(!ice.unwrap()["value"].as_str().unwrap().contains("pa55"), "credentials masked");
}

#[tokio::test]
async fn admins_see_where_the_space_goes() {
    let mut h = Harness::new("storage", true).await;
    h.sign_up_admin().await;
    let id = h.games("nes").await[0]["id"].as_i64().unwrap();
    let sid =
        h.ok(START, json!({"game": id})).await["startSession"]["id"].as_str().unwrap().to_owned();
    let save = "mutation($s: ID!, $slot: Int!) { saveState(sessionId: $s, slot: $slot) }";
    h.ok(save, json!({"s": sid, "slot": 1})).await;

    let q = "query($r: Boolean) { storage(refresh: $r) { measuredAt totalBytes freeBytes \
             areas { id bytes files } saves { userId username bytes files } } }";
    let r = h.ok(q, json!({"r": true})).await["storage"].clone();
    let area = |id: &str| r["areas"].as_array().unwrap().iter().find(|a| a["id"] == id).cloned();
    assert!(area("database").unwrap()["bytes"].as_f64().unwrap() > 0.0);
    assert!(area("saves").unwrap()["files"].as_f64().unwrap() >= 1.0);
    assert!(area("other").is_some());
    assert_eq!(r["saves"][0]["userId"], 1);
    assert!(r["saves"][0]["username"].is_string(), "named from the users table");
    assert!(r["totalBytes"].as_f64().unwrap() >= r["freeBytes"].as_f64().unwrap());
    // Kept a few minutes: the same measurement unless refreshed.
    let again = h.ok(q, json!({"r": false})).await["storage"]["measuredAt"].clone();
    assert_eq!(again, r["measuredAt"]);
    // Direct mode keeps no copies: nothing to clear.
    assert_eq!(h.ok("mutation { clearCache }", json!({})).await["clearCache"], json!(0.0));
}

#[tokio::test]
async fn admins_see_whether_telemetry_is_exported() {
    let mut h = Harness::new("telemetry", false).await;
    h.sign_up_admin().await;
    let q = "{ telemetry { enabled endpoint logFormat serviceName metricIntervalMs headerNames \
             traces { lastAt ok exports } settings { name } } }";
    let t = h.ok(q, json!({})).await["telemetry"].clone();
    assert_eq!((t["enabled"].clone(), t["endpoint"].clone()), (json!(false), Value::Null));
    assert_eq!(t["serviceName"], "sovemu-server");
    assert_eq!(t["metricIntervalMs"], json!(60_000.0));
    assert_eq!(t["traces"]["lastAt"], Value::Null, "nothing exported");
    let names: Vec<&str> =
        t["settings"].as_array().unwrap().iter().map(|s| s["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"SOVEMU_OTEL_ENDPOINT") && names.contains(&"SOVEMU_LOG_FORMAT"));
}

#[tokio::test]
async fn everyone_sees_the_build_and_admins_the_attributions() {
    let mut h = Harness::new("about", false).await;
    h.sign_up_admin().await;
    let q = "{ buildInfo { version libretroRelease licence source } }";
    let build = h.ok(q, json!({})).await["buildInfo"].clone();
    assert_eq!(build["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(build["libretroRelease"], sovemu_library::cores::LIBRETRO_RELEASE);
    assert_eq!(
        (build["licence"].as_str(), build["source"].as_str()),
        (Some("AGPL-3.0-or-later"), Some("https://github.com/sovereign-media/SovEmu"))
    );
    let q = "{ attributions { fbneoCommit services { name licence } components { name commit } } }";
    let a = h.ok(q, json!({})).await["attributions"].clone();
    assert_eq!(a["services"][0], json!({"name": "ScreenScraper", "licence": "CC BY-NC-SA 4.0"}));
    assert_eq!(a["fbneoCommit"].as_str().unwrap().len(), 40);
    assert!(a["components"].as_array().unwrap().iter().any(|c| c["commit"].is_string()));
}

#[tokio::test]
async fn admins_see_every_core_with_its_licence() {
    let mut h = Harness::new("cores", true).await;
    h.sign_up_admin().await;
    let q = "{ libretroRelease cores { name label loaded error file licence nonCommercial \
             sourceUrl systems { id chosen } } }";
    let data = h.ok(q, json!({})).await;
    assert_eq!(data["libretroRelease"], sovemu_library::cores::LIBRETRO_RELEASE);
    let cores = data["cores"].as_array().unwrap();
    assert_eq!(cores.len(), sovemu_library::cores::CORES.len(), "every shipped core, in order");
    // The test core, installed as fceumm: probed now, since there is no manifest.
    let fceumm = &cores[0];
    assert_eq!(fceumm["name"], "fceumm");
    assert_eq!((fceumm["loaded"].clone(), fceumm["error"].clone()), (json!(true), Value::Null));
    assert!(fceumm["file"].as_str().unwrap().contains("fceumm_libretro."));
    assert_eq!(fceumm["licence"], "GPLv2");
    assert_eq!(fceumm["systems"], json!([{"id": "nes", "chosen": true}]));
    // Shipped, but not in this cores folder.
    let snes9x = cores.iter().find(|c| c["name"] == "snes9x").unwrap();
    assert_eq!((snes9x["file"].clone(), snes9x["loaded"].clone()), (Value::Null, json!(false)));
    assert_eq!(snes9x["nonCommercial"], true);
    assert_eq!(snes9x["systems"][0], json!({"id": "snes", "chosen": false}));
}

#[tokio::test]
async fn a_library_that_went_away_keeps_its_games() {
    let mut h = Harness::new("library-gone", false).await;
    h.sign_up_admin().await;
    let scan = "mutation { scanLibrary { found removed } }";
    let known = h.ok(scan, json!({})).await["scanLibrary"]["found"].as_i64().unwrap();
    assert!(known > 0);
    let count = |state: &AppState| {
        let db = state.db.clone();
        async move {
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM games").fetch_one(&db).await.unwrap()
        }
    };

    // An unmounted share: its mount point is an empty folder.
    let roms = h.dir.join("roms");
    let away = h.dir.join("roms-away");
    std::fs::rename(&roms, &away).unwrap();
    std::fs::create_dir_all(&roms).unwrap();
    let (_, message) = h.err(scan, json!({})).await;
    assert!(message.contains("is the share mounted"), "{message}");
    assert_eq!(count(&h.state).await, known, "nothing removed");

    // No folder at all.
    std::fs::remove_dir_all(&roms).unwrap();
    let (_, message) = h.err(scan, json!({})).await;
    assert!(message.contains("reading the library"), "{message}");
    assert_eq!(count(&h.state).await, known, "nothing removed");

    // Back again: the same games, nothing added or removed.
    std::fs::rename(&away, &roms).unwrap();
    let back = h.ok(scan, json!({})).await["scanLibrary"].clone();
    assert_eq!((back["found"].as_i64(), back["removed"].as_i64()), (Some(known), Some(0)));
}

#[tokio::test]
async fn system_counts_follow_each_scan() {
    let mut h = Harness::new("counts", false).await;
    h.sign_up_admin().await;
    let nes = |systems: &Value| system(systems, "nes")["gameCount"].as_i64().unwrap();
    let before = nes(&h.systems().await);
    std::fs::write(h.dir.join("roms/nes/Second Game (USA).nes"), [0u8; 16]).unwrap();
    assert_eq!(
        nes(&h.systems().await),
        before,
        "counted when the library changes, not per request"
    );
    h.ok("mutation { scanLibrary { found } }", json!({})).await;
    assert_eq!(nes(&h.systems().await), before + 1);
    std::fs::remove_file(h.dir.join("roms/nes/Second Game (USA).nes")).unwrap();
    h.ok("mutation { scanLibrary { found } }", json!({})).await;
    assert_eq!(nes(&h.systems().await), before);
}

#[tokio::test]
async fn backgrounds_and_system_photos_are_served_and_the_admin_chooses() {
    let mut h = Harness::new("appearance", false).await;
    h.sign_up_admin().await;
    let bg = "{ background { id name url builtin } }";

    // Nothing chosen: one at random, each a built-in one.
    let shown = h.ok(bg, json!({})).await["background"].clone();
    assert!(["dither", "lattice", "screens"].contains(&shown["id"].as_str().unwrap()));
    let settings = h
        .ok("{ appearance { chosen fromEnvironment environment backgrounds { id } } }", json!({}))
        .await["appearance"]
        .clone();
    assert_eq!(settings["chosen"], "random");
    assert_eq!(settings["fromEnvironment"], false);
    assert_eq!(settings["backgrounds"].as_array().unwrap().len(), 3);

    // The admin's choice, for everyone, served from the binary.
    h.ok("mutation { setBackground(id: \"lattice\") }", json!({})).await;
    let shown = h.ok(bg, json!({})).await["background"].clone();
    assert_eq!(
        (shown["id"].clone(), shown["name"].clone(), shown["builtin"].clone()),
        (json!("lattice"), json!("Lattice"), json!(true))
    );
    let (status, _, headers) = h.get_raw(shown["url"].as_str().unwrap()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::CONTENT_TYPE], "image/webp");
    assert_eq!(
        h.call("GET", "/media/backgrounds/lattice", None).await.0,
        StatusCode::UNAUTHORIZED,
        "unsigned"
    );
    assert_eq!(h.err("mutation { setBackground(id: \"nope\") }", json!({})).await.0, "NOT_FOUND");

    // The admin's own, found by the next scan; `none` shows none.
    std::fs::create_dir_all(h.dir.join("backgrounds")).unwrap();
    std::fs::write(h.dir.join("backgrounds/sunset-beach.png"), b"\x89PNG not really").unwrap();
    h.ok("mutation { scanLibrary { found } }", json!({})).await;
    h.ok("mutation { setBackground(id: \"sunset-beach\") }", json!({})).await;
    let shown = h.ok(bg, json!({})).await["background"].clone();
    assert_eq!(
        (shown["name"].clone(), shown["builtin"].clone()),
        (json!("Sunset beach"), json!(false))
    );
    let (_, _, headers) = h.get_raw(shown["url"].as_str().unwrap()).await;
    assert_eq!(headers[header::CONTENT_TYPE], "image/png");
    h.ok("mutation { setBackground(id: \"none\") }", json!({})).await;
    assert_eq!(h.ok(bg, json!({})).await["background"], Value::Null);

    // System photos: built in, or the admin's own by the system's id.
    let photo =
        |systems: &Value, id: &str| system(systems, id)["photoUrl"].as_str().map(str::to_owned);
    let systems = h.ok("{ systems { id photoUrl } }", json!({})).await["systems"].clone();
    let nes = photo(&systems, "nes").expect("nes has a built-in photo");
    assert_eq!(h.get_raw(&nes).await.0, StatusCode::OK);
    std::fs::create_dir_all(h.dir.join("systems")).unwrap();
    std::fs::write(h.dir.join("systems/nes.jpg"), b"my nes").unwrap();
    h.ok("mutation { scanLibrary { found } }", json!({})).await;
    let (_, body, headers) = h.get_raw(&nes).await;
    assert_eq!(
        (headers[header::CONTENT_TYPE].to_str().unwrap(), body.as_slice()),
        ("image/jpeg", b"my nes".as_slice())
    );

    // A viewer sees the background, and cannot change it.
    h.switch_to_new_viewer("kid").await;
    assert_eq!(h.ok(bg, json!({})).await["background"], Value::Null, "the admin chose none");
    assert_eq!(h.err("mutation { setBackground(id: \"dither\") }", json!({})).await.0, "FORBIDDEN");
}

#[tokio::test]
async fn the_environment_locks_the_background() {
    let mut h =
        Harness::new_with("appearance-env", false, vec!["--background".into(), "screens".into()])
            .await;
    h.sign_up_admin().await;
    let settings = h.ok("{ appearance { chosen fromEnvironment environment } }", json!({})).await["appearance"].clone();
    assert_eq!(
        settings,
        json!({"chosen": "screens", "fromEnvironment": true, "environment": "SOVEMU_BACKGROUND=screens"})
    );
    assert_eq!(h.ok("{ background { id } }", json!({})).await["background"]["id"], "screens");
    assert_eq!(h.err("mutation { setBackground(id: \"dither\") }", json!({})).await.0, "CONFLICT");
}

#[tokio::test]
async fn soft_patches_apply_by_default_and_can_be_chosen() {
    let mut h = Harness::new("patches", true).await;
    h.sign_up_admin().await;
    // A translation beside the game (a real IPS patch: byte 0 becomes 1), and a damaged hack in
    // its folder.
    let roms = h.dir.join("roms/nes");
    let mut ips = b"PATCH".to_vec();
    ips.extend_from_slice(&[0, 0, 0, 0, 1, 1]);
    ips.extend_from_slice(b"EOF");
    std::fs::write(roms.join("Test Game (USA).ips"), ips).unwrap();
    std::fs::create_dir_all(roms.join("Test Game (USA)")).unwrap();
    std::fs::write(roms.join("Test Game (USA)/Hard Mode.bps"), [0u8; 8]).unwrap();
    h.ok("mutation { scanLibrary { found } }", json!({})).await;

    let games = h.games("nes").await;
    assert_eq!(games.as_array().unwrap().len(), 1, "patches are not games");
    let id = games[0]["id"].as_i64().unwrap();
    let q =
        "query($id: Int!) { game(id: $id) { patch patches { id name isDefault } saves { slot } } }";
    let game = h.ok(q, json!({"id": id})).await;
    assert_eq!(game["game"]["patch"], "IPS patch", "the patch beside it applies by default");
    let patches = game["game"]["patches"].as_array().unwrap().clone();
    assert_eq!(patches[0]["name"], "IPS patch");
    assert_eq!(patches[0]["isDefault"], true);
    assert_eq!(patches[1]["name"], "Hard Mode");

    // Played patched: its saves are the patched game's own.
    let sid =
        h.ok(START, json!({"game": id})).await["startSession"]["id"].as_str().unwrap().to_owned();
    let save = "mutation($s: ID!, $slot: Int!) { saveState(sessionId: $s, slot: $slot) }";
    h.ok(save, json!({"s": sid, "slot": 3})).await;
    assert_eq!(h.ok(q, json!({"id": id})).await["game"]["saves"], json!([{"slot": 3}]));
    let states = h.dir.join("saves/1/nes/states/Test Game (USA) [IPS patch]");
    assert!(states.join("slot3.state").is_file(), "{}", states.display());

    // The original: none of the patched game's saves.
    let set = "mutation($g: Int!, $p: Int) { setPatch(gameId: $g, patchId: $p) }";
    h.ok(set, json!({"g": id, "p": null})).await;
    let game = h.ok(q, json!({"id": id})).await;
    assert_eq!(game["game"]["patch"], Value::Null);
    assert_eq!(game["game"]["saves"], json!([]));
    h.ok(START, json!({"game": id})).await;

    // A damaged patch is refused, and the player is told why.
    h.ok(set, json!({"g": id, "p": patches[1]["id"]})).await;
    assert_eq!(h.games("nes").await[0]["patch"], "Hard Mode", "the choice shows in lists too");
    let (code, message) = h.err(START, json!({"game": id})).await;
    assert_eq!(code, "UNAVAILABLE");
    assert!(message.contains("not an IPS, UPS or BPS patch"), "{message}");

    let (code, _) = h.err(set, json!({"g": id, "p": 99_999})).await;
    assert_eq!(code, "NOT_FOUND", "a patch of another game, or none");

    // The admin's list: every patch with its game, and who chose it.
    let all = h
        .ok(
            "{ patches { name file format isDefault gameId gameTitle system chosenBy originalBy } }",
            json!({}),
        )
        .await["patches"]
        .clone();
    assert_eq!(
        all,
        json!([
            {"name": "IPS patch", "file": "nes/Test Game (USA).ips", "format": "IPS",
             "isDefault": true, "gameId": id, "gameTitle": "Test Game", "system": "nes",
             "chosenBy": 0, "originalBy": 0},
            {"name": "Hard Mode", "file": "nes/Test Game (USA)/Hard Mode.bps", "format": "BPS",
             "isDefault": false, "gameId": id, "gameTitle": "Test Game", "system": "nes",
             "chosenBy": 1, "originalBy": 0},
        ])
    );
}

#[tokio::test]
async fn decorations_come_from_the_chosen_set_and_are_served() {
    let mut h =
        Harness::new_with("decorations", false, vec!["--decoration".into(), "arcade".into()]).await;
    h.sign_up_admin().await;
    // A set in <data>/decorations, Batocera's layout: one for the system, one for the game.
    let set = h.dir.join("decorations/arcade");
    let info =
        r#"{"width":1920,"height":1080,"top":0,"left":300,"bottom":0,"right":300,"opacity":1}"#;
    // A PNG header is all the server reads of an image without a .info: 1280 x 960.
    let mut header = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    header.extend_from_slice(&1280u32.to_be_bytes());
    header.extend_from_slice(&960u32.to_be_bytes());
    for (path, bytes) in [
        ("systems/nes.png", b"system".as_slice()),
        ("systems/nes.info", info.as_bytes()),
        ("games/nes/Test Game (USA).png", &header),
        // Unreadable, without a .info: passed over.
        ("games/nes/Other.png", b"not a png"),
    ] {
        std::fs::create_dir_all(set.join(path).parent().unwrap()).unwrap();
        std::fs::write(set.join(path), bytes).unwrap();
    }
    h.state.reload_decorations();

    let q = "query($id: Int!) { game(id: $id) { decoration { url set width height left right top bottom } } }";
    let id = h.games("nes").await[0]["id"].as_i64().unwrap();
    let decoration = h.ok(q, json!({"id": id})).await["game"]["decoration"].clone();
    let url = decoration["url"].as_str().unwrap();
    assert!(
        url.starts_with(
            "/media/decorations/arcade/games/nes/Test%20Game%20%28USA%29.png?Sov-UserId=1&"
        ),
        "signed: {url}"
    );
    assert_eq!(decoration["set"], "arcade");
    // No .info of its own: the image's largest centred 4:3 window.
    assert_eq!((decoration["width"].clone(), decoration["left"].clone()), (json!(1280), json!(0)));
    assert_eq!(h.call("GET", decoration["url"].as_str().unwrap(), None).await.0, StatusCode::OK);
    let system = h.state.decorations().for_game("arcade", "nes", "Other").unwrap();
    assert_eq!(system.path, "systems/nes.png", "an unreadable image is passed over");
    assert_eq!(system.window.left, 300, "from its .info");
    let escape = h.signed(1, "/media/decorations/arcade/..%2F..%2Fsovemu.db");
    assert_eq!(h.call("GET", &escape, None).await.0, StatusCode::NOT_FOUND, "only listed files");
    assert_eq!(
        h.call("GET", "/media/decorations/arcade/systems/nes.png", None).await.0,
        StatusCode::UNAUTHORIZED,
        "unsigned"
    );
    // The admin's report: the set, what it holds, and each system's preview.
    let report = h
        .ok(
            "{ decorations { default folders sets { name builtin hasDefault systems games } \
             systems { system { id } set perSystem preview { set url } } } }",
            json!({}),
        )
        .await["decorations"]
        .clone();
    assert_eq!(report["default"], "arcade");
    assert_eq!(
        report["sets"],
        json!([{"name": "arcade", "builtin": false, "hasDefault": false, "systems": ["nes"], "games": 2}])
    );
    let nes = report["systems"].as_array().unwrap().iter().find(|s| s["system"]["id"] == "nes");
    let nes = nes.unwrap();
    assert_eq!((nes["set"].clone(), nes["perSystem"].clone()), (json!("arcade"), json!(false)));
    let preview = nes["preview"]["url"].as_str().unwrap();
    assert!(preview.starts_with("/media/decorations/arcade/systems/nes.png?Sov-"), "{preview}");
    let gba = report["systems"].as_array().unwrap().iter().find(|s| s["system"]["id"] == "gba");
    assert_eq!(gba.unwrap()["preview"], Value::Null, "no image for it, and no default");
    let listed = report["systems"].as_array().unwrap().len();
    assert_eq!(listed, 2, "only the systems with games");
    assert!(report["folders"][0].as_str().unwrap().ends_with("decorations"));

    // `none` turns them off for a system.
    let mut h =
        Harness::new_with("decorations-none", false, vec!["--decoration".into(), "none".into()])
            .await;
    h.sign_up_admin().await;
    let id = h.games("nes").await[0]["id"].as_i64().unwrap();
    assert_eq!(h.ok(q, json!({"id": id})).await["game"]["decoration"], Value::Null);
}

// --- Access: admins only, and each user's own data ------------------------------------------

/// Every admin-only query and mutation, with arguments that would otherwise be accepted.
const ADMIN_ONLY: &[&str] = &[
    "{ users { id } }",
    "{ activeSessions { id } }",
    "{ libraryStatus { __typename } }",
    "{ metadataStatus { __typename } }",
    "{ attributions { __typename } }",
    "{ telemetry { __typename } }",
    "{ storage { __typename } }",
    "{ network { __typename } }",
    "{ configuration { name } }",
    "{ cores { name } }",
    "{ libretroRelease }",
    "{ patches { id } }",
    "{ decorations { __typename } }",
    "{ biosDir }",
    "{ gpu { __typename } }",
    "{ jobs { __typename } }",
    "{ jobRuns { __typename } }",
    "{ serverResources { __typename } }",
    "{ resourceHistory(range: HOUR) { __typename } }",
    "{ emulatorSettings(system: \"nes\") { __typename } }",
    "{ controllerDefaults { __typename } }",
    "mutation { addUser(input: {username: \"mallory\", password: \"password123\"}) { id } }",
    "mutation { setUserRole(userId: 1, role: ADMIN) { id } }",
    "mutation { setUserPassword(userId: 1, password: \"password123\") }",
    "mutation { removeUser(userId: 1) }",
    "mutation { signOutEverywhere(userId: 1) }",
    "{ signIns { id } }",
    "{ user(id: 1) { joinedAt } }",
    "mutation { endSignIn(id: \"x\") }",
    "mutation { scanLibrary { found } }",
    "mutation { syncLibrary }",
    "mutation { fetchMetadata { __typename } }",
    "mutation { runJob(job: MAINTENANCE) { __typename } }",
    "mutation { changeEmulatorSettings(system: \"nes\", changes: []) { __typename } }",
    "mutation { setRunAhead(system: \"nes\", frames: 1) { __typename } }",
    "mutation { clearCache }",
    "mutation { saveControllerDefault(controller: \"xbox\", bindings: []) { __typename } }",
    "mutation { resetControllerDefault(controller: \"xbox\") }",
    "{ appearance { chosen } }",
    "{ sessionFailures { title } }",
    "{ streaming { testSeconds } }",
    "mutation { testConnection(sdp: \"v=0\") }",
    "{ clientEventGroups { count } }",
    "{ activity { next } }",
    "{ playStatistics(range: WEEK) { from } }",
    "{ logs { enabled } }",
    "{ gamesNeedingAttention(kind: DUPLICATE) { path } }",
    "{ attentionCounts { count } }",
    "mutation { lookUpGame(gameId: 1) { running } }",
    "{ backups { dir } }",
    "{ thirdPartyNotices { server { id } } }",
    "mutation { deleteBackup(name: \"x\") }",
    "mutation { createSharedCollection(name: \"x\") { id } }",
    "mutation { renameSharedCollection(id: 1, name: \"x\") }",
    "mutation { deleteSharedCollection(id: 1) }",
    "mutation { setInSharedCollection(collectionId: 1, gameId: 1, member: true) }",
    "{ clientEvents(kind: OTHER, client: \"web\") { at } }",
    "mutation { setBackground(id: \"random\") }",
];

#[tokio::test]
async fn viewers_and_strangers_are_refused_every_admin_resolver() {
    let mut h = Harness::new("admin-only", false).await;
    h.sign_up_admin().await;
    h.switch_to_new_viewer("kid").await;
    for doc in ADMIN_ONLY {
        assert_eq!(h.err(doc, json!({})).await.0, "FORBIDDEN", "a viewer ran {doc}");
    }
    h.sign_out();
    for doc in ADMIN_ONLY {
        assert_eq!(h.err(doc, json!({})).await.0, "UNAUTHENTICATED", "no one ran {doc}");
    }
    // Every admin resolver is in the list: a new one must be added to it.
    let in_source: usize = [
        include_str!("graphql/mod.rs"),
        include_str!("graphql/accounts.rs"),
        include_str!("graphql/appearance.rs"),
        include_str!("graphql/library.rs"),
        include_str!("graphql/sessions.rs"),
        include_str!("graphql/library_admin.rs"),
        include_str!("graphql/server.rs"),
        include_str!("graphql/jobs.rs"),
        include_str!("graphql/resources.rs"),
        include_str!("graphql/settings.rs"),
        include_str!("graphql/controllers.rs"),
        include_str!("graphql/streaming.rs"),
        include_str!("graphql/client_events.rs"),
        include_str!("graphql/activity.rs"),
        include_str!("graphql/statistics.rs"),
        include_str!("graphql/logs.rs"),
        include_str!("graphql/attention.rs"),
        include_str!("graphql/backups.rs"),
        include_str!("graphql/collections_admin.rs"),
    ]
    .iter()
    .map(|source| source.matches("admin(ctx).await?").count())
    .sum();
    assert_eq!(ADMIN_ONLY.len(), in_source, "ADMIN_ONLY lists every admin(ctx) resolver");
}

#[tokio::test]
async fn one_users_sessions_saves_and_lists_are_not_anothers() {
    let mut h = Harness::new("per-user", true).await;
    h.sign_up_admin().await;
    let game = h.games("nes").await[0]["id"].as_i64().unwrap();

    // The admin plays, saves, collects, favourites and cheats.
    let sid =
        h.ok(START, json!({"game": game})).await["startSession"]["id"].as_str().unwrap().to_owned();
    let slot = |s: &str, slot: i64| json!({"s": s, "slot": slot});
    h.ok("mutation($s: ID!, $slot: Int!) { saveState(sessionId: $s, slot: $slot) }", slot(&sid, 2))
        .await;
    let collection = h.ok("mutation { createCollection(name: \"Mine\") { id } }", json!({})).await
        ["createCollection"]["id"]
        .as_i64()
        .unwrap();
    let set_in = "mutation($c: Int!, $g: Int!, $m: Boolean!) { setInCollection(collectionId: $c, gameId: $g, member: $m) }";
    h.ok(set_in, json!({"c": collection, "g": game, "m": true})).await;
    h.ok("mutation($g: Int!) { setFavourite(gameId: $g, favourite: true) }", json!({"g": game}))
        .await;
    let cheat = h
        .ok(
            "mutation($g: Int!) { addCheat(gameId: $g, code: \"ABCD-1234\") { id } }",
            json!({"g": game}),
        )
        .await["addCheat"][0]["id"]
        .as_i64()
        .unwrap();

    h.switch_to_new_viewer("kid").await;
    // Their session: not found, and still running.
    for doc in
        ["mutation($s: ID!) { stopSession(id: $s) }", "mutation($s: ID!) { resetSession(id: $s) }"]
    {
        assert_eq!(h.err(doc, json!({"s": sid})).await.0, "NOT_FOUND", "{doc}");
    }
    for doc in [
        "mutation($s: ID!, $slot: Int!) { saveState(sessionId: $s, slot: $slot) }",
        "mutation($s: ID!, $slot: Int!) { loadState(sessionId: $s, slot: $slot) }",
    ] {
        assert_eq!(h.err(doc, slot(&sid, 2)).await.0, "NOT_FOUND", "{doc}");
    }
    assert!(h.state.workers.get(&sid).is_some(), "the admin's game plays on");

    // Their saves, lists, favourites, cheats and history: none of them show, none change.
    let mine = "query($g: Int!) { game(id: $g) { saves { slot } favourite collectionIds cheats { id } playCount } }";
    assert_eq!(
        h.ok(mine, json!({"g": game})).await["game"],
        json!({"saves": [], "favourite": false, "collectionIds": [], "cheats": [], "playCount": 0})
    );
    assert_eq!(h.ok("{ collections { id } }", json!({})).await["collections"], json!([]));
    assert_eq!(
        h.ok("{ home { recent { id } favourites { id } } }", json!({})).await["home"],
        json!({"recent": [], "favourites": []})
    );
    let delete_save = "mutation($g: Int!) { deleteSave(gameId: $g, slot: 2) }";
    assert_eq!(h.err(delete_save, json!({"g": game})).await.0, "NOT_FOUND");
    for (doc, vars) in [
        (
            "mutation($c: Int!) { renameCollection(id: $c, name: \"Ours\") }",
            json!({"c": collection}),
        ),
        ("mutation($c: Int!) { deleteCollection(id: $c) }", json!({"c": collection})),
        (set_in, json!({"c": collection, "g": game, "m": false})),
        (
            "mutation($g: Int!, $c: Int!) { setCheatEnabled(gameId: $g, cheatId: $c, enabled: false) { id } }",
            json!({"g": game, "c": cheat}),
        ),
        (
            "mutation($g: Int!, $c: Int!) { deleteCheat(gameId: $g, cheatId: $c) }",
            json!({"g": game, "c": cheat}),
        ),
    ] {
        assert_eq!(h.err(doc, vars).await.0, "NOT_FOUND", "{doc}");
    }

    // Back as the admin: everything as it was.
    h.sign_out();
    h.login("admin", "password123").await.unwrap();
    let theirs = h.ok(mine, json!({"g": game})).await["game"].clone();
    assert!(theirs["saves"].as_array().unwrap().contains(&json!({"slot": 2})));
    assert_eq!(
        (theirs["favourite"].clone(), theirs["collectionIds"].clone()),
        (json!(true), json!([collection]))
    );
    assert_eq!(theirs["cheats"], json!([{"id": cheat}]));
}

#[tokio::test]
async fn admins_see_the_games_that_failed_with_their_last_lines() {
    let mut h = Harness::new("failures", false).await;
    h.sign_up_admin().await;
    assert_eq!(
        h.ok("{ sessionFailures { title } }", json!({})).await["sessionFailures"],
        json!([])
    );
    // As the workers' observer records it (sovemu-sessions reports how each worker ended).
    use sovemu_sessions::Observer;
    let events = h.state.failures.clone();
    let lines = vec!["[pcsx2] missing BIOS".to_owned(), "failed to boot".to_owned()];
    let ended = |reason| sovemu_sessions::Ended {
        user_id: 1,
        game_id: 42,
        title: "Tony Hawk's Pro Skater 3",
        system: "ps2",
        core: "pcsx2_libretro",
        reason,
        ran: std::time::Duration::from_millis(1500),
        loaded: false,
        code: Some(1),
        signal: None,
        last_lines: &lines,
        start_error: None,
    };
    events.session_ended(&ended(sovemu_sessions::EndReason::Stopped));
    events.session_ended(&sovemu_sessions::Ended {
        start_error: Some("the core could not load the game"),
        ..ended(sovemu_sessions::EndReason::Error)
    });
    let q = "{ sessionFailures { username gameId title system core whilePlaying reason message exitCode signal lastLines ranSeconds } }";
    assert_eq!(
        h.ok(q, json!({})).await["sessionFailures"],
        json!([{
            "username": "admin", "gameId": 42, "title": "Tony Hawk's Pro Skater 3", "system": "ps2",
            "core": "pcsx2_libretro", "whilePlaying": false, "reason": "error",
            "message": "the core could not load the game", "exitCode": 1,
            "signal": null, "lastLines": ["[pcsx2] missing BIOS", "failed to boot"], "ranSeconds": 1.5,
        }]),
        "a quit is not a failure; the error is"
    );
}

#[tokio::test]
async fn a_start_the_server_refuses_is_listed_with_why() {
    // No core is installed in this harness, so every start is refused before a worker runs.
    let mut h = Harness::new("failures-refused", false).await;
    h.sign_up_admin().await;
    let game = h.games("nes").await[0]["id"].as_i64().unwrap();
    assert_eq!(h.err(START, json!({"game": game})).await.0, "UNAVAILABLE");
    let q = "{ sessionFailures { username title system core whilePlaying reason message exitCode signal lastLines } }";
    assert_eq!(
        h.ok(q, json!({})).await["sessionFailures"],
        json!([{
            "username": "admin", "title": "Test Game", "system": "nes", "core": null,
            "whilePlaying": false, "reason": "refused",
            "message": "no core installed for Nintendo Entertainment System",
            "exitCode": null, "signal": null, "lastLines": [],
        }])
    );
}

#[tokio::test]
async fn admins_see_the_streaming_settings_and_test_a_connection() {
    let mut h = Harness::new("streaming", true).await;
    h.sign_up_admin().await;
    let page = h.ok("{ streaming { testSeconds settings { name } } }", json!({})).await;
    assert_eq!(page["streaming"]["testSeconds"], 30);
    let names: Vec<&str> = page["streaming"]["settings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"SOVEMU_VIDEO_KBPS") && names.contains(&"SOVEMU_REWIND_SECONDS"));
    assert!(!names.contains(&"SOVEMU_HTTP"), "a network setting, on the Network page");

    // The startup encoder test ran before the server took a request: every encoder a session
    // could use is listed, as it would run, working or with why not.
    let test = h
        .ok(
            "{ streaming { encoderTest { tookMs budgetMs note testedAt encoders { codec encoder hardware threads works error timings { width height frames meanMs p95Ms } } } } }",
            json!({}),
        )
        .await["streaming"]["encoderTest"]
        .clone();
    assert_eq!((&test["budgetMs"], &test["note"]), (&json!(10_000), &Value::Null), "{test}");
    assert!(test["tookMs"].as_i64().unwrap() <= 10_000 && test["testedAt"].as_f64().is_some());
    let encoders = test["encoders"].as_array().unwrap();
    let vp8 = encoders.iter().find(|e| e["encoder"] == "libvpx").expect("libvpx is always tried");
    assert_eq!(
        (&vp8["codec"], &vp8["hardware"], &vp8["threads"]),
        (&json!("vp8"), &json!(false), &json!(4))
    );
    assert!(encoders.iter().any(|e| e["encoder"] == "x264" && e["codec"] == "h264"), "{test}");
    for e in encoders {
        assert_eq!(e["works"] == true, e["error"].is_null(), "{e}");
    }

    // A real connection-test worker on a game port reads the offer: one that is not SDP is
    // refused with why, and its port goes back to the pool.
    let (code, message) = h
        .err("mutation($sdp: String!) { testConnection(sdp: $sdp) }", json!({"sdp": "nonsense"}))
        .await;
    assert_eq!(code, "BAD_REQUEST");
    assert!(message.contains("bad offer"), "{message}");
    for _ in 0..100 {
        if h.state.workers.ports_in_use() == 0 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(h.state.workers.ports_in_use(), 0, "its port is back in the pool");
}

#[tokio::test]
async fn an_admin_stops_a_players_session() {
    // Through `Workers::stop`, as a player's own stop: the worker writes its saves before it is
    // ever killed (tested in sovemu-sessions).
    let mut h = Harness::new("admin-stop", true).await;
    h.sign_up_admin().await;
    let game = h.games("nes").await[0]["id"].as_i64().unwrap();
    h.switch_to_new_viewer("kid").await;
    let sid =
        h.ok(START, json!({"game": game})).await["startSession"]["id"].as_str().unwrap().to_owned();

    h.sign_out();
    h.login("admin", "password123").await.unwrap();
    let stop = "mutation($s: ID!) { stopSession(id: $s) }";
    h.ok(stop, json!({"s": sid})).await;
    assert!(h.state.workers.get(&sid).is_none(), "ended");
    assert_eq!(h.err(stop, json!({"s": sid})).await.0, "NOT_FOUND", "already ended");
}

#[tokio::test]
async fn a_hidden_systems_games_cannot_be_reached_by_id() {
    let mut h = Harness::new_with("hidden-by-id", false, Vec::new()).await;
    h.sign_up_admin().await;
    let gba = h.games("gba").await[0]["id"].as_i64().unwrap();
    let mut config = (*h.state.config).clone();
    config.systems = crate::structured::systems(&[(
        "SOVEMU_SYSTEMS__gba__ENABLED".to_owned(),
        "false".to_owned(),
    )])
    .unwrap();
    h.state.config = std::sync::Arc::new(config);

    assert_eq!(
        h.err("query($g: Int!) { game(id: $g) { id } }", json!({"g": gba})).await.0,
        "NOT_FOUND"
    );
    for doc in [
        "mutation($g: Int!) { setFavourite(gameId: $g, favourite: true) }",
        "mutation($g: Int!) { addCheat(gameId: $g, code: \"X\") { id } }",
        "mutation($g: Int!) { setPatch(gameId: $g, patchId: null) }",
    ] {
        assert_eq!(h.err(doc, json!({"g": gba})).await.0, "NOT_FOUND", "{doc}");
    }
    assert_eq!(h.err(START, json!({"game": gba})).await.0, "NOT_FOUND", "nor started");
    let thumb = h.signed(1, &format!("/media/games/{gba}/thumb"));
    assert_eq!(h.call("GET", &thumb, None).await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn save_slots_are_checked() {
    let mut h = Harness::new("slots", true).await;
    h.sign_up_admin().await;
    let game = h.games("nes").await[0]["id"].as_i64().unwrap();
    let sid =
        h.ok(START, json!({"game": game})).await["startSession"]["id"].as_str().unwrap().to_owned();
    let save = "mutation($s: ID!, $slot: Int!) { saveState(sessionId: $s, slot: $slot) }";
    let load = "mutation($s: ID!, $slot: Int!) { loadState(sessionId: $s, slot: $slot) }";
    // The auto save is the worker's; a slot past the last would never be listed.
    for slot in [0, 10, 4_000_000] {
        assert_eq!(
            h.err(save, json!({"s": sid, "slot": slot})).await.0,
            "BAD_REQUEST",
            "save {slot}"
        );
    }
    assert_eq!(h.err(load, json!({"s": sid, "slot": 10})).await.0, "BAD_REQUEST");
    let delete = "mutation($g: Int!, $slot: Int!) { deleteSave(gameId: $g, slot: $slot) }";
    assert_eq!(h.err(delete, json!({"g": game, "slot": 10})).await.0, "BAD_REQUEST");
    h.ok(save, json!({"s": sid, "slot": 9})).await;
}

// --- Sign-in (sovemu_identity: oauth-as with OpenID Connect over it) --------------------------

const VERIFIER: &str = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
const CHALLENGE: &str = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
const CALLBACK: &str = "http://sovemu.test/auth/callback/";

impl Harness {
    /// A request as a browser at `http://sovemu.test` sends it: status, headers, body text.
    /// Keeps a cookie the answer sets.
    async fn browse(
        &mut self,
        method: &str,
        uri: &str,
        form: Option<&[(&str, &str)]>,
    ) -> (StatusCode, header::HeaderMap, String) {
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::HOST, "sovemu.test")
            .header(header::ORIGIN, "http://sovemu.test");
        if let Some(cookie) = &self.cookie {
            request = request.header(header::COOKIE, cookie);
        }
        let body = match form {
            Some(pairs) => {
                request = request.header(header::CONTENT_TYPE, "application/x-www-form-urlencoded");
                Body::from(
                    url::form_urlencoded::Serializer::new(String::new())
                        .extend_pairs(pairs)
                        .finish(),
                )
            }
            None => Body::empty(),
        };
        let response =
            router(self.state.clone()).oneshot(request.body(body).unwrap()).await.unwrap();
        let (status, headers) = (response.status(), response.headers().clone());
        if let Some(set) = headers.get(header::SET_COOKIE) {
            let pair = set.to_str().unwrap().split(';').next().unwrap().to_owned();
            self.cookie = (!pair.ends_with('=')).then_some(pair);
        }
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (status, headers, String::from_utf8_lossy(&bytes).into_owned())
    }

    /// `/oauth/token` with a form: status and JSON.
    async fn token(&self, pairs: &[(&str, &str)]) -> (StatusCode, Value) {
        let (status, body, _) = self.form("/oauth/token", pairs).await;
        (status, body)
    }
}

fn authorize_url(extra: &[(&str, &str)]) -> String {
    let mut pairs = vec![
        ("response_type", "code"),
        ("client_id", "sovemu-web"),
        ("redirect_uri", CALLBACK),
        ("scope", "openid profile offline_access"),
        ("state", "xyz"),
        ("nonce", "n-1"),
        ("code_challenge", CHALLENGE),
        ("code_challenge_method", "S256"),
    ];
    for (k, v) in extra {
        pairs.retain(|(key, _)| key != k);
        if !v.is_empty() {
            pairs.push((k, v));
        }
    }
    let query = url::form_urlencoded::Serializer::new(String::new()).extend_pairs(pairs).finish();
    format!("/oauth/authorize?{query}")
}

/// Where a redirect goes (path for this site's, else the URL without its query) and its query.
fn location(headers: &header::HeaderMap) -> (String, std::collections::BTreeMap<String, String>) {
    let to = headers[header::LOCATION].to_str().unwrap();
    let local = to.starts_with('/');
    let url =
        url::Url::parse(&if local { format!("http://sovemu.test{to}") } else { to.to_owned() })
            .unwrap();
    let place =
        if local { url.path().to_owned() } else { to.split('?').next().unwrap().to_owned() };
    (place, url.query_pairs().into_owned().collect())
}

fn jwt_claims(token: &str) -> Value {
    use base64::Engine;
    let payload = token.split('.').nth(1).unwrap();
    serde_json::from_slice(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(payload).unwrap(),
    )
    .unwrap()
}

fn exchange<'a>(code: &'a str, verifier: &'a str) -> [(&'a str, &'a str); 5] {
    [
        ("grant_type", "authorization_code"),
        ("client_id", "sovemu-web"),
        ("code", code),
        ("redirect_uri", CALLBACK),
        ("code_verifier", verifier),
    ]
}

#[tokio::test]
async fn the_web_client_signs_in_with_the_code_flow_and_pkce() {
    let mut h = Harness::new("oidc-code", false).await;
    h.sign_up_admin().await;
    h.sign_out();

    // Discovery: oauth-as's metadata with OpenID Connect's fields added.
    let (status, _, body) = h.browse("GET", "/.well-known/openid-configuration", None).await;
    assert_eq!(status, StatusCode::OK);
    let discovery: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(discovery["issuer"], "http://sovemu.test");
    assert_eq!(discovery["authorization_endpoint"], "http://sovemu.test/oauth/authorize");
    assert_eq!(discovery["token_endpoint"], "http://sovemu.test/oauth/token");
    assert_eq!(discovery["userinfo_endpoint"], "http://sovemu.test/oauth/userinfo");
    assert_eq!(discovery["code_challenge_methods_supported"], json!(["S256"]));
    let (_, _, jwks) = h.browse("GET", "/oauth/jwks", None).await;
    assert_eq!(serde_json::from_str::<Value>(&jwks).unwrap()["keys"][0]["alg"], "ES256");

    // Signed out: to the sign-in page, which comes back here (without `prompt=login`).
    let (status, headers, _) = h.browse("GET", &authorize_url(&[("prompt", "login")]), None).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let (path, params) = location(&headers);
    assert_eq!(path, "/login/");
    let next = params["continue"].clone();
    assert!(next.starts_with("/oauth/authorize?") && !next.contains("prompt"), "{next}");

    // A wrong password goes back to the page with the error and the username.
    let form = |pw: &'static str, next: &str| {
        [("username", "admin"), ("password", pw), ("continue", next)]
            .map(|(k, v)| (k, v.to_owned()))
    };
    let wrong = form("wrong-password", &next);
    let pairs: Vec<(&str, &str)> = wrong.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let (status, headers, _) = h.browse("POST", "/oauth/signin", Some(&pairs)).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let (path, params) = location(&headers);
    assert_eq!(path, "/login/");
    assert_eq!(params["error"], "wrong username or password");
    assert_eq!(params["username"], "admin");
    assert_eq!(params["continue"], next);
    assert!(h.cookie.is_none());

    // The right one signs the browser in and continues.
    let right = form("password123", &next);
    let pairs: Vec<(&str, &str)> = right.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let (status, headers, _) = h.browse("POST", "/oauth/signin", Some(&pairs)).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(headers[header::LOCATION].to_str().unwrap(), next);
    let set = headers[header::SET_COOKIE].to_str().unwrap();
    assert!(set.contains("HttpOnly") && set.contains("SameSite=Lax"), "{set}");

    // Signed in at the server: oauth-as issues a code to the callback.
    let (_, headers, _) = h.browse("GET", &next, None).await;
    let (to, params) = location(&headers);
    assert_eq!(to, CALLBACK);
    assert_eq!(params["state"], "xyz");
    assert_eq!(params["iss"], "http://sovemu.test");
    let code = params["code"].clone();

    // The wrong verifier is refused. A code is single use, and redeeming it twice revokes the
    // tokens it gave (RFC 6749 s4.1.2): someone else had it.
    let wrong_verifier = "a-wrong-verifier-that-is-long-enough-to-be-valid-xxxx";
    assert_eq!(h.token(&exchange(&code, wrong_verifier)).await.1["error"], "invalid_grant");
    let (status, body) = h.token(&exchange(&code, VERIFIER)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    h.token = Some(body["access_token"].as_str().unwrap().to_owned());
    assert!(h.ok(VIEWER, json!({})).await["viewer"]["user"].is_object());
    assert_eq!(h.token(&exchange(&code, VERIFIER)).await.1["error"], "invalid_grant");
    assert_eq!(h.ok(VIEWER, json!({})).await["viewer"]["user"], Value::Null, "revoked");

    // A fresh code with the right verifier: tokens, and an ID token for this sign-in.
    let (_, headers, _) = h.browse("GET", &next, None).await;
    let code = location(&headers).1["code"].clone();
    let (status, body) = h.token(&exchange(&code, VERIFIER)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["token_type"], "Bearer");
    let claims = jwt_claims(body["id_token"].as_str().unwrap());
    assert_eq!(claims["iss"], "http://sovemu.test");
    assert_eq!(claims["aud"], "sovemu-web");
    assert_eq!(claims["nonce"], "n-1");
    assert_eq!(claims["preferred_username"], "admin");
    assert_eq!(claims["role"], "admin");
    h.token = Some(body["access_token"].as_str().unwrap().to_owned());
    let me = h.ok(VIEWER, json!({})).await["viewer"]["user"].clone();
    assert_eq!(me, json!({"username": "admin", "isAdmin": true}));

    // Refresh tokens rotate, and a replay ends the whole sign-in.
    let first = body["refresh_token"].as_str().unwrap().to_owned();
    let refresh = |token: &str, client: &'static str| {
        [
            ("grant_type", "refresh_token".to_owned()),
            ("client_id", client.to_owned()),
            ("refresh_token", token.to_owned()),
        ]
    };
    let call = |pairs: [(&'static str, String); 3]| {
        let h = &h;
        async move {
            let refs: Vec<(&str, &str)> = pairs.iter().map(|(k, v)| (*k, v.as_str())).collect();
            h.token(&refs).await
        }
    };
    assert_eq!(
        call(refresh(&first, "sovemu-native")).await.1["error"],
        "invalid_grant",
        "another app's"
    );
    let (status, body) = call(refresh(&first, "sovemu-web")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let second = body["refresh_token"].as_str().unwrap().to_owned();
    assert_ne!(second, first);
    assert!(body["id_token"].is_string(), "a refresh brings a new ID token");
    assert_eq!(call(refresh(&first, "sovemu-web")).await.1["error"], "invalid_grant", "a replay");
    assert_eq!(
        call(refresh(&second, "sovemu-web")).await.1["error"],
        "invalid_grant",
        "the replay ended the whole sign-in"
    );

    // Signing out at the server clears the cookie and goes home.
    let (status, headers, _) = h
        .browse("GET", "/oauth/logout?post_logout_redirect_uri=http%3A%2F%2Fsovemu.test%2F", None)
        .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(headers[header::LOCATION], "http://sovemu.test/");
    assert!(h.cookie.is_none());
    let (_, headers, _) = h.browse("GET", &authorize_url(&[]), None).await;
    assert_eq!(location(&headers).0, "/login/", "signed out at the server");
}

#[tokio::test]
async fn a_tv_signs_in_with_a_code_approved_in_a_browser() {
    let mut h = Harness::new("oidc-device", false).await;
    h.sign_up_admin().await;
    h.sign_out();

    // The TV asks for a code.
    let (status, grant, _) = h
        .form(
            "/oauth/device_authorization",
            &[("client_id", "sovemu-native"), ("scope", "openid profile offline_access")],
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{grant}");
    assert_eq!(grant["verification_uri"], "http://sovemu.test/device");
    let device_code = grant["device_code"].as_str().unwrap().to_owned();
    let user_code = grant["user_code"].as_str().unwrap().to_owned();
    let poll = [
        ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
        ("client_id", "sovemu-native"),
        ("device_code", device_code.as_str()),
    ];

    // On a phone: the device page wants a signed-in browser first.
    let (status, headers, _) = h.browse("GET", "/device", None).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location(&headers).1["continue"], "/device");
    let pairs = [("username", "admin"), ("password", "password123"), ("continue", "/device")];
    h.browse("POST", "/oauth/signin", Some(&pairs)).await;
    assert!(h.cookie.is_some());

    // Enter the code, then approve it, each form carrying its CSRF token.
    let csrf = |page: &str| {
        let at = page.find("name=\"csrf_token\" value=\"").expect("a CSRF token") + 25;
        page[at..].split('"').next().unwrap().to_owned()
    };
    let (status, _, page) = h.browse("GET", "/device", None).await;
    assert_eq!(status, StatusCode::OK);
    let token = csrf(&page);
    let entered = [("user_code", user_code.as_str()), ("csrf_token", token.as_str())];
    let (status, _, page) = h.browse("POST", "/device", Some(&entered)).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    let token = csrf(&page);
    let approve =
        [("user_code", user_code.as_str()), ("csrf_token", token.as_str()), ("action", "approve")];
    let (status, _, page) = h.browse("POST", "/device", Some(&approve)).await;
    assert_eq!(status, StatusCode::OK, "{page}");

    // The TV's poll gets its tokens, as the admin. (Polling sooner than `interval` is answered
    // `slow_down`, so this test polls once.)
    let (status, body) = h.token(&poll).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(jwt_claims(body["id_token"].as_str().unwrap())["aud"], "sovemu-native");
    h.cookie = None;
    h.token = Some(body["access_token"].as_str().unwrap().to_owned());
    assert_eq!(h.ok(VIEWER, json!({})).await["viewer"]["user"]["username"], "admin");
}

#[tokio::test]
async fn apps_learn_where_to_sign_in_from_the_api() {
    let mut h = Harness::new("bootstrap", false).await;
    let query = "{ bootstrapConfig { signInAvailable address issuer nativeClientId tokenEndpoint \
                 revocationEndpoint deviceAuthorizationEndpoint verificationUri passwordEndpoint } }";
    let body = json!({"query": query});
    let ask = |h: &Harness, host: &'static str| {
        let state = h.state.clone();
        let body = body.to_string();
        async move {
            let request = Request::builder()
                .method("POST")
                .uri("/graphql")
                .header(header::HOST, host)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body))
                .unwrap();
            let response = router(state).oneshot(request).await.unwrap();
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            serde_json::from_slice::<Value>(&bytes).unwrap()["data"]["bootstrapConfig"].clone()
        }
    };
    // Signed out, at a configured address: everything an app needs, as absolute URLs.
    assert_eq!(
        ask(&h, "sovemu.test").await,
        json!({
            "signInAvailable": true,
            "address": "sovemu.test",
            "issuer": "http://sovemu.test",
            "nativeClientId": "sovemu-native",
            "tokenEndpoint": "http://sovemu.test/oauth/token",
            "revocationEndpoint": "http://sovemu.test/oauth/revoke",
            "deviceAuthorizationEndpoint": "http://sovemu.test/oauth/device_authorization",
            "verificationUri": "http://sovemu.test/device",
            "passwordEndpoint": "http://sovemu.test/oauth/password",
        })
    );
    // Anywhere else: nothing can sign in, and the app can say which address to add.
    let elsewhere = ask(&h, "10.0.0.9:8080").await;
    assert_eq!(elsewhere["signInAvailable"], false);
    assert_eq!(elsewhere["address"], "10.0.0.9:8080");
    assert_eq!(elsewhere["tokenEndpoint"], Value::Null);
    // The endpoints it names are the ones that answer.
    h.sign_up_admin().await;
    assert!(h.token.is_some(), "signed in at the advertised password endpoint");

    // With the password fallback off, the apps are told so.
    let h = Harness::new_with(
        "bootstrap-no-password",
        false,
        ["--password-sign-in", "false"].map(String::from).to_vec(),
    )
    .await;
    assert_eq!(ask(&h, "sovemu.test").await["passwordEndpoint"], Value::Null);
}

#[tokio::test]
async fn the_provider_refuses_what_it_should() {
    let mut h = Harness::new("oidc-refuse", false).await;
    h.sign_up_admin().await;

    // A redirect anywhere but this server's callback is refused on the page, never followed.
    for bad in ["http://evil.test/auth/callback/", "http://sovemu.test/elsewhere"] {
        let (status, headers, _) =
            h.browse("GET", &authorize_url(&[("redirect_uri", bad)]), None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}");
        assert!(headers.get(header::LOCATION).is_none());
    }
    // PKCE is required, and only S256.
    for extra in [[("code_challenge", "")], [("code_challenge_method", "plain")]] {
        let (_, headers, _) = h.browse("GET", &authorize_url(&extra), None).await;
        let (to, params) = location(&headers);
        assert_eq!(to, CALLBACK);
        assert_eq!(params["error"], "invalid_request");
    }

    // OAuth 2.1 has no password grant, and the fallback is the apps' only.
    let (_, body) = h
        .token(&[
            ("grant_type", "password"),
            ("client_id", "sovemu-web"),
            ("username", "admin"),
            ("password", "password123"),
        ])
        .await;
    assert_eq!(body["error"], "unsupported_grant_type", "{body}");
    let (status, body, _) = h
        .form(
            "/oauth/password",
            &[("client_id", "sovemu-web"), ("username", "admin"), ("password", "password123")],
        )
        .await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::UNAUTHORIZED, Some("invalid_client"))
    );

    // Only the configured addresses sign in.
    let request = Request::builder()
        .uri("/.well-known/openid-configuration")
        .header(header::HOST, "elsewhere.test")
        .body(Body::empty())
        .unwrap();
    let response = router(h.state.clone()).oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    // A sign-in form on another site cannot sign a browser in here.
    let request = Request::builder()
        .method("POST")
        .uri("/oauth/signin")
        .header(header::HOST, "sovemu.test")
        .header(header::ORIGIN, "http://evil.test")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from("username=admin&password=password123&continue=%2F"))
        .unwrap();
    let response = router(h.state.clone()).oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(response.headers().get(header::SET_COOKIE).is_none());

    // The cookie signs nothing into the API: only a bearer token does. No sign-in mutation.
    h.token = None;
    assert!(h.cookie.is_some());
    assert_eq!(h.err(SYSTEMS, json!({})).await.0, "UNAUTHENTICATED");
    assert!(!h.err("mutation { logout }", json!({})).await.1.is_empty());
}

#[tokio::test]
async fn app_tokens_revoke_and_follow_role_changes() {
    let mut h = Harness::new("oidc-native", false).await;
    h.sign_up_admin().await;
    h.ok(
        "mutation { addUser(input: {username: \"kid\", password: \"password123\"}) { role } }",
        json!({}),
    )
    .await;
    h.sign_out();
    let kid = h.login("kid", "password123").await.unwrap();
    assert_eq!(kid["isAdmin"], false);
    assert_eq!(
        h.ok("{ viewer { user { role } } }", json!({})).await["viewer"]["user"]["role"],
        "VIEWER"
    );
    assert_eq!(h.err("{ users { username } }", json!({})).await.0, "FORBIDDEN");

    // A role change applies to the next request: tokens are looked up, not trusted.
    sqlx::query("UPDATE users SET role = 'admin' WHERE username = 'kid'")
        .execute(&h.state.db)
        .await
        .unwrap();
    assert_eq!(
        h.ok("{ users { username role } }", json!({})).await["users"].as_array().unwrap().len(),
        2
    );

    // userinfo.
    let request = Request::builder()
        .uri("/oauth/userinfo")
        .header(header::HOST, "sovemu.test")
        .header(header::AUTHORIZATION, format!("Bearer {}", h.token.clone().unwrap()))
        .body(Body::empty())
        .unwrap();
    let response = router(h.state.clone()).oneshot(request).await.unwrap();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let info: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        (info["preferred_username"].as_str(), info["role"].as_str()),
        (Some("kid"), Some("admin"))
    );

    // Revoking the access token ends the sign-in.
    let token = h.token.clone().unwrap();
    let (status, _, _) =
        h.form("/oauth/revoke", &[("token", &token), ("client_id", "sovemu-native")]).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(h.ok(VIEWER, json!({})).await["viewer"]["user"], Value::Null);

    // With the password fallback turned off, the apps sign in with a code instead.
    let h = Harness::new_with(
        "oidc-no-password",
        false,
        ["--password-sign-in", "false"].map(String::from).to_vec(),
    )
    .await;
    let (_, body, _) = h
        .form(
            "/oauth/password",
            &[("client_id", "sovemu-native"), ("username", "a"), ("password", "b")],
        )
        .await;
    assert_eq!(body["error"], "unsupported_grant_type");
}

#[tokio::test]
async fn every_page_path_gets_the_web_client_and_its_assets_are_files() {
    let mut h = Harness::new("spa", false).await;
    let web = h.dir.join("web");
    std::fs::create_dir_all(web.join("assets")).unwrap();
    std::fs::write(web.join("index.html"), "<!doctype html><div id=root></div>").unwrap();
    std::fs::write(web.join("assets/index-abc.js"), "console.log(1)").unwrap();
    std::fs::write(web.join("favicon.svg"), "<svg/>").unwrap();
    let mut config = (*h.state.config).clone();
    config.web_dir = Some(web);
    h.state.config = std::sync::Arc::new(config);

    let get = |path: &str| Request::builder().uri(path).body(Body::empty()).unwrap();
    for path in
        ["/", "/system/?id=snes&game=7", "/play/?game=1", "/auth/callback/?code=x", "/admin/users/"]
    {
        let response = router(h.state.clone()).oneshot(get(path)).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        assert_eq!(
            response.headers()[axum::http::header::CACHE_CONTROL],
            "no-cache",
            "{path}: the page always revalidates"
        );
        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert!(body.starts_with(b"<!doctype html>"), "{path}: the web client");
    }
    let asset = router(h.state.clone()).oneshot(get("/assets/index-abc.js")).await.unwrap();
    assert_eq!(asset.status(), StatusCode::OK);
    assert_eq!(
        asset.headers()[axum::http::header::CACHE_CONTROL],
        "public, max-age=31536000, immutable"
    );
    let gone = router(h.state.clone()).oneshot(get("/assets/index-old.js")).await.unwrap();
    assert_eq!(
        gone.status(),
        StatusCode::NOT_FOUND,
        "an old build's script is missing, not the page"
    );
    let icon = router(h.state.clone()).oneshot(get("/favicon.svg")).await.unwrap();
    assert_eq!(icon.status(), StatusCode::OK);
    // The API's own routes still answer for themselves.
    let health = router(h.state.clone()).oneshot(get("/health")).await.unwrap();
    assert_eq!(health.status(), StatusCode::OK);
}

#[tokio::test]
async fn admins_see_the_games_needing_attention_and_look_one_up_again() {
    let mut h = Harness::new("attention", false).await;
    h.sign_up_admin().await;
    for path in [
        "roms/gba/more/Other.gba",
        "roms/psx/Big (USA) (Disc 1).chd",
        "roms/psx/Big (USA) (Disc 3).chd",
        "roms/nes/notes.abc",
    ] {
        std::fs::create_dir_all(h.dir.join(path).parent().unwrap()).unwrap();
        std::fs::write(h.dir.join(path), [0u8; 16]).unwrap();
    }
    h.ok("mutation { scanLibrary { found } }", json!({})).await;

    let of = "query ($kind: AttentionKind!, $system: ID) {
        gamesNeedingAttention(kind: $kind, system: $system) { gameId system title path detail } }";
    let items = |data: Value| data["gamesNeedingAttention"].as_array().unwrap().clone();
    let duplicates = items(h.ok(of, json!({"kind": "DUPLICATE"})).await);
    let paths: Vec<&str> = duplicates.iter().map(|i| i["path"].as_str().unwrap()).collect();
    assert_eq!(paths, ["gba/Other.gba", "gba/more/Other.gba"]);
    let missing = items(h.ok(of, json!({"kind": "MISSING_PARTS"})).await);
    assert_eq!(
        (missing[0]["title"].as_str(), missing[0]["detail"].as_str()),
        (Some("Big"), Some("disc 2"))
    );
    let skipped = items(h.ok(of, json!({"kind": "UNRECOGNISED", "system": "nes"})).await);
    assert_eq!(
        (skipped[0]["path"].as_str(), skipped[0]["gameId"].is_null()),
        (Some("nes/notes.abc"), true)
    );
    // The artwork beside the ROM does not count: only what was downloaded.
    let no_art = items(h.ok(of, json!({"kind": "NO_BOX_ART", "system": "nes"})).await);
    assert_eq!(no_art[0]["path"], "nes/Test Game (USA).nes");
    h.give_box_art(&[0u8; 16]).await;
    let no_art = items(h.ok(of, json!({"kind": "NO_BOX_ART", "system": "nes"})).await);
    assert!(no_art.is_empty(), "the NES game has box art now: {no_art:?}");

    let counts = h.ok("{ attentionCounts { kind system count } }", json!({})).await;
    assert!(
        counts["attentionCounts"]
            .as_array()
            .unwrap()
            .contains(&json!({"kind": "DUPLICATE", "system": "gba", "count": 2})),
        "{counts}"
    );

    let game = duplicates[0]["gameId"].as_i64().unwrap();
    // The scan started a lookup in the background; another is refused while it runs (on a
    // loaded machine it can still be running here), so wait for it to finish.
    for _ in 0..200 {
        if !h.state.jobs.is_running(crate::jobs::Job::Metadata) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    let look_up = "mutation ($id: Int!) { lookUpGame(gameId: $id) { total } }";
    h.ok(look_up, json!({"id": game})).await;
    let (code, _) = h.err(look_up, json!({"id": 99_999})).await;
    assert_eq!(code, "NOT_FOUND");
}

#[tokio::test]
async fn admins_keep_shared_collections_that_everyone_sees_but_only_admins_change() {
    let mut h = Harness::new("shared-collections", false).await;
    h.sign_up_admin().await;
    let game: i64 = sqlx::query_scalar("SELECT id FROM games WHERE system = 'nes'")
        .fetch_one(&h.state.db)
        .await
        .unwrap();
    let made = h
        .ok(
            "mutation { createSharedCollection(name: \"Family  night\") { id name shared } }",
            json!({}),
        )
        .await["createSharedCollection"]
        .clone();
    assert_eq!(
        (made["name"].as_str(), made["shared"].as_bool()),
        (Some("Family night"), Some(true))
    );
    let id = made["id"].as_i64().unwrap();
    let set = "mutation ($c: Int!, $g: Int!) { setInSharedCollection(collectionId: $c, gameId: $g, member: true) }";
    h.ok(set, json!({"c": id, "g": game})).await;
    let (code, message) = h
        .err("mutation { createSharedCollection(name: \"Family night\") { id } }", json!({}))
        .await;
    assert_eq!(code, "CONFLICT");
    assert!(message.contains("already a shared collection"), "{message}");

    // A viewer sees it beside their own, with its games, and cannot change it.
    h.switch_to_new_viewer("kid").await;
    let shared = "{ sharedCollections { id name declared games { id sharedCollectionIds } } collections { id } }";
    let data = h.ok(shared, json!({})).await;
    assert_eq!(data["collections"], json!([]), "a user's own list stays their own");
    let first = &data["sharedCollections"][0];
    assert_eq!(
        (first["name"].as_str(), first["declared"].as_bool()),
        (Some("Family night"), Some(false))
    );
    assert_eq!(first["games"], json!([{"id": game, "sharedCollectionIds": [id]}]));
    let mine = "mutation ($c: Int!, $g: Int!) { setInCollection(collectionId: $c, gameId: $g, member: false) }";
    assert_eq!(h.err(mine, json!({"c": id, "g": game})).await.0, "NOT_FOUND");
    assert_eq!(
        h.err("mutation ($c: Int!) { deleteCollection(id: $c) }", json!({"c": id})).await.0,
        "NOT_FOUND"
    );

    // Declared in the environment: made and filled after a scan, locked, and gone once it is
    // no longer declared. A shared collection by the same name is taken over.
    h.sign_out();
    h.login("admin", "password123").await.unwrap();
    let mut config = (*h.state.config).clone();
    config.collections = crate::structured::collections(&[
        ("SOVEMU_COLLECTIONS__0__NAME".to_owned(), "Family night".to_owned()),
        ("SOVEMU_COLLECTIONS__0__GAMES__0".to_owned(), "gba/Other.gba".to_owned()),
        ("SOVEMU_COLLECTIONS__0__GAMES__1".to_owned(), "nes/Test Game (USA).nes".to_owned()),
        ("SOVEMU_COLLECTIONS__0__GAMES__2".to_owned(), "nes/Not Here.nes".to_owned()),
        ("SOVEMU_COLLECTIONS__1__NAME".to_owned(), "Racing".to_owned()),
    ])
    .unwrap();
    h.state.config = std::sync::Arc::new(config.clone());
    let declared = crate::collections::declare(&h.state.db, &config.collections).await.unwrap();
    assert_eq!(declared.missing, ["nes/Not Here.nes"]);
    let list = "{ sharedCollections { id name declared games { path } } }";
    let data = h.ok(list, json!({})).await["sharedCollections"].clone();
    assert_eq!(data[0]["id"].as_i64(), Some(id), "taken over");
    assert_eq!(data[0]["declared"], true);
    assert_eq!(
        data[0]["games"],
        json!([{"path": "gba/Other.gba"}, {"path": "nes/Test Game (USA).nes"}]),
        "in the order declared"
    );
    assert_eq!(data[1]["name"], "Racing");
    let rename = "mutation ($c: Int!) { renameSharedCollection(id: $c, name: \"x\") }";
    assert_eq!(h.err(rename, json!({"c": id})).await.0, "CONFLICT");
    assert_eq!(h.err(set, json!({"c": id, "g": game})).await.0, "CONFLICT");

    crate::collections::declare(&h.state.db, &config.collections[..1]).await.unwrap();
    let names = h.ok(list, json!({})).await["sharedCollections"].clone();
    assert_eq!(names.as_array().unwrap().len(), 1, "Racing is no longer declared: {names}");
}

#[tokio::test]
async fn admins_make_download_and_delete_backups_and_only_admins_get_them() {
    let mut h = Harness::new("backups", false).await;
    h.sign_up_admin().await;
    let list = "{ backups { dir intervalHours keep restoreFrom files { name bytes url } } }";
    let before = h.ok(list, json!({})).await["backups"].clone();
    assert_eq!(before["files"], json!([]));
    assert_eq!((before["intervalHours"].as_f64(), before["keep"].as_i64()), (Some(0.0), Some(7)));
    assert!(before["dir"].as_str().unwrap().ends_with("backups"));

    h.ok("mutation { runJob(job: BACKUP) { name } }", json!({})).await;
    let mut files = Value::Null;
    for _ in 0..300 {
        files = h.ok(list, json!({})).await["backups"]["files"].clone();
        if files.as_array().is_some_and(|f| !f.is_empty()) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let backup = files[0].clone();
    let name = backup["name"].as_str().unwrap().to_owned();
    assert!(name.starts_with("sovemu-backup-") && name.ends_with(".zip"), "{name}");

    let (status, bytes, headers) = h.get_raw(backup["url"].as_str().unwrap()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(&bytes[..2], b"PK", "a zip file");
    assert_eq!(bytes.len() as f64, backup["bytes"].as_f64().unwrap());
    assert!(headers[header::CONTENT_DISPOSITION].to_str().unwrap().contains(&name));
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    assert!(zip.by_name("sovemu.db").is_ok() && zip.by_name("backup.json").is_ok());

    // It holds every password hash: a viewer's signed URL for it is refused, and no URL
    // leaves the backup folder.
    h.switch_to_new_viewer("kid").await;
    let kid: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'kid'")
        .fetch_one(&h.state.db)
        .await
        .unwrap();
    let (status, _, _) = h.get_raw(&h.signed(kid, &format!("/media/backups/{name}"))).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    h.sign_out();
    h.login("admin", "password123").await.unwrap();
    let admin: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'admin'")
        .fetch_one(&h.state.db)
        .await
        .unwrap();
    let (status, _, _) = h.get_raw(&h.signed(admin, "/media/backups/..%2Fsovemu.db")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let delete = "mutation ($n: String!) { deleteBackup(name: $n) }";
    h.ok(delete, json!({"n": name})).await;
    assert_eq!(h.ok(list, json!({})).await["backups"]["files"], json!([]));
    assert_eq!(h.err(delete, json!({"n": name})).await.0, "NOT_FOUND");
}
