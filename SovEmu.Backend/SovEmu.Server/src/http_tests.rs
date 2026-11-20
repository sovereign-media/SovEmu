//! The API (GraphQL) end to end through the router: accounts, the library and a real game
//! session, plus the plain-HTTP media and ops routes.
//!
//! The session test starts a real `sovemu-worker session`, with `SovEmu.TestCore` installed as
//! the "NES core" and an empty file as the ROM (the test core accepts any content).

use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;

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
    cookie: Option<String>,
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
        // Never the real LaunchBox server: a test that wants it points it at a mock.
        if !extra.iter().any(|a| a.starts_with("--launchbox")) {
            args.extend(["--launchbox".into(), "false".into()]);
        }
        args.extend(extra);
        let state = build_state(Config::parse_from(args)).await.unwrap();
        crate::library::scan(&state.db, &state.config.roms_dir).await.unwrap();
        Self { state, dir, cookie: None }
    }

    async fn call(&mut self, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
        let mut request = Request::builder().method(method).uri(uri);
        if let Some(cookie) = &self.cookie {
            request = request.header(header::COOKIE, cookie);
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

    async fn sign_up_admin(&mut self) {
        self.ok(SETUP, json!({"u": "admin", "p": "password123"})).await;
    }

    async fn login(&mut self, username: &str, password: &str) -> Result<Value, String> {
        let (data, errors) = self.gql(LOGIN, json!({"u": username, "p": password})).await;
        if errors.is_null() {
            Ok(data["login"].clone())
        } else {
            Err(errors[0]["extensions"]["code"].as_str().unwrap_or_default().to_owned())
        }
    }

    async fn systems(&mut self) -> Value {
        self.ok(SYSTEMS, json!({})).await["systems"].clone()
    }

    /// A file URL signed for `user_id`, as the API would hand it out.
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
fn udp_ports() -> String {
    use std::sync::atomic::{AtomicU16, Ordering};
    static NEXT: AtomicU16 = AtomicU16::new(0);
    let base =
        41000 + (std::process::id() % 400) as u16 * 40 + NEXT.fetch_add(2, Ordering::Relaxed);
    format!("{base}-{}", base + 1)
}

const SETUP: &str = "mutation($u: String!, $p: String!) { setup(username: $u, password: $p) { id username isAdmin } }";
const LOGIN: &str = "mutation($u: String!, $p: String!) { login(username: $u, password: $p) { id username isAdmin } }";
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
                        artwork { running total done downloaded failed } } }";
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
    assert_eq!(h.ok("mutation { logout }", json!({})).await["logout"], true);
    assert_eq!(
        h.ok(VIEWER, json!({})).await["viewer"]["user"],
        Value::Null,
        "the cookie is cleared"
    );

    assert_eq!(h.login("kid", "nope-nope").await, Err("UNAUTHENTICATED".into()));
    assert_eq!(h.login("ghost", "password123").await, Err("UNAUTHENTICATED".into()));
    assert_eq!(h.login("KID", "password123").await.unwrap()["isAdmin"], false);
    assert_eq!(h.err("{ users { username } }", json!({})).await.0, "FORBIDDEN");
    assert_eq!(h.err("mutation { scanLibrary { found } }", json!({})).await.0, "FORBIDDEN");
    assert!(h.systems().await.as_array().unwrap().len() > 5);
}

#[tokio::test]
async fn throttles_guessing_and_serves_ops_routes() {
    let mut h = Harness::new("ops", false).await;
    h.sign_up_admin().await;
    h.cookie = None;
    for _ in 0..crate::ops::MAX_FAILURES {
        assert_eq!(h.login("admin", "wrong-password").await, Err("UNAUTHENTICATED".into()));
    }
    let (code, message) = h.err(LOGIN, json!({"u": "admin", "p": "wrong-password"})).await;
    assert_eq!(code, "TOO_MANY_REQUESTS");
    assert!(message.contains("try again"), "{message}");
    // Locked out even with the right password, until the window passes.
    assert_eq!(h.login("admin", "password123").await, Err("TOO_MANY_REQUESTS".into()));

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
    assert_eq!(system(&systems, "snes")["gameCount"], 0);

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
    let cookie = h.cookie.take();
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
    let forged = crate::assets::AssetSigner::new(b"other key".to_vec(), 24)
        .sign(1, &format!("/media/games/{id}/thumb"));
    assert_eq!(h.call("GET", &forged, None).await.0, StatusCode::FORBIDDEN, "another key");
    h.cookie = cookie;
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
    let reset = "mutation($id: ID!) { resetSession(id: $id) }";
    assert_eq!(h.ok(reset, json!({"id": sid})).await["resetSession"], true);
    let offer = "mutation($id: ID!) { sessionOffer(id: $id, sdp: \"not sdp\") }";
    assert_eq!(h.err(offer, json!({"id": sid})).await.0, "BAD_REQUEST");

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

/// A stand-in for ScreenScraper and TheGamesDB on a local port. ScreenScraper answers for the
/// NES game (checking the exact query a real client must send) and not for the GBA one;
/// TheGamesDB answers for the GBA one, with box art. `quota` makes ScreenScraper refuse.
/// `/lb/Metadata.zip` is a LaunchBox dump that knows the NES game.
async fn mock_services(quota: bool) -> String {
    use axum::extract::Query;
    use axum::routing::get;
    use std::collections::HashMap;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let img_base = format!("{base}/img/");
    let app = axum::Router::new()
        .route(
            "/api2/jeuInfos.php",
            get(move |Query(q): Query<HashMap<String, String>>| async move {
                if quota {
                    return (StatusCode::from_u16(430).unwrap(), "Quota exceeded".to_owned());
                }
                assert_eq!(q["devid"], "dev");
                assert_eq!(q["devpassword"], "pw");
                assert_eq!(q["output"], "xml");
                match q["systemeid"].as_str() {
                    "3" => {
                        assert_eq!(q["romnom"], "Test Game (USA).nes");
                        assert_eq!(q["romtaille"], "64");
                        // MD5 of the fixture's 64 zero bytes.
                        assert_eq!(q["md5"], "3b5d3c7d207e37dceeedd301e35e2e58");
                        (StatusCode::OK, crate::metadata::screenscraper::tests::SAMPLE.to_owned())
                    }
                    _ => (StatusCode::NOT_FOUND, "Erreur : Rom/Iso/Dossier non trouvée !".to_owned()),
                }
            }),
        )
        .route(
            "/v1/Games/ByGameName",
            get(|Query(q): Query<HashMap<String, String>>| async move {
                assert_eq!(q["apikey"], "key");
                let games = if q["name"] == "Other" && q["filter[platform]"] == "5" {
                    json!([{"id": 5, "game_title": "Other", "overview": "From TGDB", "developers": [1], "players": 2}])
                } else {
                    json!([])
                };
                axum::Json(json!({"data": {"games": games}}))
            }),
        )
        .route("/v1/Developers", get(|| async { axum::Json(json!({"data": {"developers": {"1": {"id": 1, "name": "Dev Co"}}}})) }))
        .route("/v1/Publishers", get(|| async { axum::Json(json!({"data": {"publishers": {}}})) }))
        .route("/v1/Genres", get(|| async { axum::Json(json!({"data": {"genres": {}}})) }))
        .route(
            "/v1/Games/Images",
            get(move || {
                let img_base = img_base.clone();
                async move {
                    axum::Json(json!({"data": {"base_url": {"large": img_base},
                        "images": {"5": [{"type": "boxart", "side": "front", "filename": "box.png"}]}}}))
                }
            }),
        )
        .route(
            "/lb/Metadata.zip",
            get(|| async { crate::metadata::launchbox::tests::zip_bytes(LAUNCHBOX_DUMP) }),
        )
        .route("/img/box.png", get(|| async { ([(header::CONTENT_TYPE, "image/png")], vec![0x89u8, b'P', b'N', b'G']) }));
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    base
}

fn metadata_args(base: &str) -> Vec<String> {
    [
        "--screenscraper-devid",
        "dev",
        "--screenscraper-devpassword",
        "pw",
        "--screenscraper-url",
        &format!("{base}/api2"),
        "--thegamesdb-key",
        "key",
        "--thegamesdb-url",
        &format!("{base}/v1"),
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
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

#[tokio::test]
async fn asks_again_about_games_not_found_once_a_service_is_added() {
    // Both games were looked up before any service was configured: FBNeo alone, nothing found.
    let base = mock_services(false).await;
    let mut h = Harness::new_with("metadata-retry", false, metadata_args(&base)).await;
    h.sign_up_admin().await;
    sqlx::query(
        "INSERT INTO game_metadata (game_id, status, sources, tried) SELECT id, 'not_found', '', 'fbneo' FROM games",
    )
    .execute(&h.state.db)
    .await
    .unwrap();

    // The services are configured now, so an ordinary (not forced) job asks them.
    h.ok(FETCH, json!({})).await;
    let status = wait_for_job(&mut h).await;
    assert_eq!(
        (status["total"].clone(), status["found"].clone()),
        (json!(2), json!(2)),
        "{status}"
    );
    assert_eq!(h.games("nes").await[0]["title"], "Castlevania");

    // Once every configured service has been asked, a game is not asked again.
    sqlx::query(
        "UPDATE game_metadata SET status = 'not_found', tried = 'fbneo,screenscraper,thegamesdb'",
    )
    .execute(&h.state.db)
    .await
    .unwrap();
    h.ok(FETCH, json!({})).await;
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert_eq!(h.ok(METADATA, json!({})).await["metadataStatus"]["total"], 0);
}

#[tokio::test]
async fn fetches_metadata_from_both_services() {
    let base = mock_services(false).await;
    let mut h = Harness::new_with("metadata", false, metadata_args(&base)).await;
    h.sign_up_admin().await;
    let status = h.ok(METADATA, json!({})).await["metadataStatus"].clone();
    assert_eq!(status["sources"], json!(["fbneo", "screenscraper", "thegamesdb"]));

    h.ok(FETCH, json!({})).await;
    let status = wait_for_job(&mut h).await;
    assert_eq!(
        (status["found"].clone(), status["failed"].clone()),
        (json!(2), json!(0)),
        "{status}"
    );
    // Box art in the background, only for the GBA game (the NES one has a local thumb).
    assert_eq!(
        status["artwork"],
        json!({"running": false, "total": 1, "done": 1, "downloaded": 1, "failed": 0})
    );

    // ScreenScraper named the NES game and described it; the file's own title is kept too.
    let nes = h.games("nes").await;
    assert_eq!(nes[0]["title"], "Castlevania");
    assert_eq!(nes[0]["fileTitle"], "Test Game");
    assert_eq!(nes[0]["developer"], "Konami");
    assert_eq!(nes[0]["releaseDate"], "1987-05-01");
    assert_eq!(nes[0]["rating"], 0.8);

    // TheGamesDB filled in the GBA game, and its box art stands in for the missing thumb.
    let gba = h.games("gba").await;
    assert_eq!(gba[0]["description"], "From TGDB");
    assert_eq!(gba[0]["developer"], "Dev Co");
    assert_eq!(gba[0]["players"], "2");
    assert_eq!(gba[0]["media"], json!(["THUMB"]));
    let url = gba[0]["mediaUrl"].as_str().unwrap().to_owned();
    assert_eq!(h.call("GET", &url, None).await.0, StatusCode::OK);

    // Looked-up games are not asked for again.
    h.ok(FETCH, json!({})).await;
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert_eq!(h.ok(METADATA, json!({})).await["metadataStatus"]["total"], 0);
}

const LAUNCHBOX_DUMP: &str = r#"<?xml version="1.0" standalone="yes"?>
<LaunchBox>
  <Game>
    <Name>Test Game</Name>
    <ReleaseDate>1990-01-01T00:00:00+00:00</ReleaseDate>
    <Overview>From LaunchBox</Overview>
    <DatabaseID>1</DatabaseID>
    <CommunityRating>4</CommunityRating>
    <Platform>Nintendo Entertainment System</Platform>
  </Game>
  <GameImage><DatabaseID>1</DatabaseID><FileName>box.png</FileName><Type>Box - Front</Type></GameImage>
</LaunchBox>"#;

#[tokio::test]
async fn launchbox_answers_before_screenscraper() {
    // ScreenScraper is out of quota: asking it at all would leave a note.
    let base = mock_services(true).await;
    let mut args = metadata_args(&base);
    args.extend(
        [
            "--launchbox",
            "true",
            "--launchbox-url",
            &format!("{base}/lb/Metadata.zip"),
            "--launchbox-images-url",
            &format!("{base}/img"),
        ]
        .map(String::from),
    );
    let mut h = Harness::new_with("metadata-launchbox", false, args).await;
    h.sign_up_admin().await;
    let status = h.ok(METADATA, json!({})).await["metadataStatus"].clone();
    assert_eq!(status["sources"], json!(["fbneo", "launchbox", "screenscraper", "thegamesdb"]));

    h.ok(FETCH, json!({"s": "nes"})).await;
    let status = wait_for_job(&mut h).await;
    assert_eq!(status["found"], 1, "{status}");
    assert_eq!(status["notes"], json!([]), "ScreenScraper was not asked: {status}");
    assert!(h.dir.join("metadata/launchbox/Metadata.zip").is_file(), "the dump is kept");
    let nes = h.games("nes").await;
    assert_eq!(nes[0]["description"], "From LaunchBox");
    assert_eq!(nes[0]["releaseDate"], "1990-01-01");
    assert_eq!(nes[0]["rating"], 0.8);
    assert_eq!(nes[0]["media"], json!(["THUMB"]), "the scraped thumb, kept over the box art");
    let id = nes[0]["id"].as_i64().unwrap();
    assert!(
        !h.dir.join(format!("metadata/media/{id}-box.png")).exists(),
        "no box art downloaded for a game with a local thumb"
    );
}

#[tokio::test]
async fn stops_a_service_that_is_out_of_quota() {
    let base = mock_services(true).await;
    let mut h = Harness::new_with("metadata-quota", false, metadata_args(&base)).await;
    h.sign_up_admin().await;
    h.ok(FETCH, json!({"s": "nes"})).await;
    let status = wait_for_job(&mut h).await;
    assert_eq!(status["total"], 1, "only the system asked for");
    assert!(status["notes"][0].as_str().unwrap().contains("quota"), "{status}");
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

    crate::admin::sync_then_scan(h.state.clone()).await;
    assert!(
        h.dir.join("library/nes/images/Test Game (USA)-thumb.png").is_file(),
        "artwork is mirrored too"
    );
    let status = h.ok(LIBRARY, json!({})).await["libraryStatus"].clone();
    assert_eq!(status["games"], 2);
    assert_eq!(status["mirror"], json!({"running": false, "filesDone": 3}));
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

#[tokio::test]
async fn checks_bios_files_per_system() {
    let mut h = Harness::new("bios", false).await;
    h.sign_up_admin().await;
    let bios = h.dir.join("bios");
    assert!(bios.join("ps2").is_dir() && bios.join("snes").is_dir(), "a folder per system");
    // A PCSX-ReARMed BIOS in the wrong case, and a Neo Geo set beside the games.
    std::fs::write(bios.join("psx/SCPH1001.BIN"), vec![0u8; 512 * 1024]).unwrap();
    std::fs::create_dir_all(h.dir.join("roms/neogeo")).unwrap();
    std::fs::write(h.dir.join("roms/neogeo/neogeo.zip"), b"zip").unwrap();

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
    assert_eq!(psx["state"], "INCOMPLETE");
    assert_eq!(psx["folder"], "psx");
    assert_eq!(psx["groups"][0]["importance"], "RECOMMENDED");
    assert_eq!(psx["groups"][0]["anyOne"], true);
    let files = psx["groups"][0]["files"].as_array().unwrap();
    let scph1001 = files.iter().find(|f| f["name"] == "scph1001.bin").unwrap();
    assert_eq!(scph1001["state"], "MISPLACED");
    assert_eq!(scph1001["foundAt"], "psx/SCPH1001.BIN");
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
        ]
        .into_iter()
        .map(str::to_owned)
        .chain(admin("second-password")),
    );
    h.state = build_state(config).await.unwrap();
    assert_eq!(h.err(SYSTEMS, json!({})).await.0, "UNAUTHENTICATED", "old session gone");
    h.cookie = None;
    assert_eq!(h.login("boss", "first-password").await, Err("UNAUTHENTICATED".into()));
    assert!(h.login("boss", "second-password").await.is_ok());
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
        ]);
        config.users = users;
        config.users_prune = prune;
        config
    };
    let user = |name: &str, pw: &str, admin: bool| crate::structured::UserSpec {
        username: name.into(),
        password: pw.into(),
        admin,
    };
    // Declared users are created; the hand-made admin stays without pruning.
    let config =
        restart(vec![user("mum", "password-mum", true), user("kid", "password-kid", false)], false);
    h.state = build_state(config).await.unwrap();
    h.cookie = None;
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
    let admin: bool = sqlx::query_scalar("SELECT is_admin FROM users WHERE username = 'kid'")
        .fetch_one(&h.state.db)
        .await
        .unwrap();
    assert!(admin);

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
    assert_eq!(pattern["choices"], json!(["gradient", "solid"]));
    assert_eq!(pattern["labels"], json!(["Gradient", "Solid colour"]));
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
    h.cookie = None;
    assert_eq!(h.err(get, json!({"s": "nes"})).await.0, "UNAUTHENTICATED");
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
    h.ok("mutation { logout }", json!({})).await;
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
    h.ok("mutation { logout }", json!({})).await;
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
    // `none` turns them off for a system.
    let mut h =
        Harness::new_with("decorations-none", false, vec!["--decoration".into(), "none".into()])
            .await;
    h.sign_up_admin().await;
    let id = h.games("nes").await[0]["id"].as_i64().unwrap();
    assert_eq!(h.ok(q, json!({"id": id})).await["game"]["decoration"], Value::Null);
}
