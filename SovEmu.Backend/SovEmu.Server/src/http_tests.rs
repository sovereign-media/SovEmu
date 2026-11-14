//! The API end to end through the router: accounts, the library and a real game session.
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
            format!(
                "{}-{}",
                41000 + (std::process::id() % 1000) as u16 * 2,
                41001 + (std::process::id() % 1000) as u16 * 2
            ),
        ];
        if with_core {
            let (worker, core) = binaries();
            let ext = core.extension().unwrap().to_string_lossy().into_owned();
            std::fs::copy(core, cores.join(format!("fceumm_libretro.{ext}"))).unwrap();
            args.extend(["--worker".into(), worker.display().to_string()]);
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

    async fn sign_up_admin(&mut self) {
        let (status, _) = self
            .call(
                "POST",
                "/api/auth/setup",
                Some(json!({"username": "admin", "password": "password123"})),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn system<'a>(systems: &'a Value, id: &str) -> &'a Value {
    systems.as_array().unwrap().iter().find(|s| s["id"] == id).unwrap()
}

#[tokio::test]
async fn first_run_setup_then_sign_in() {
    let mut h = Harness::new("auth", false).await;
    let (_, status) = h.call("GET", "/api/auth/status", None).await;
    assert_eq!(status, json!({"setup_required": true, "user": null}));
    assert_eq!(h.call("GET", "/api/systems", None).await.0, StatusCode::UNAUTHORIZED);

    h.sign_up_admin().await;
    let (_, status) = h.call("GET", "/api/auth/status", None).await;
    assert_eq!(status["setup_required"], false);
    assert_eq!(status["user"]["username"], "admin");
    assert_eq!(status["user"]["is_admin"], true);

    // Setup only works once.
    let again = json!({"username": "eve", "password": "password123"});
    assert_eq!(h.call("POST", "/api/auth/setup", Some(again)).await.0, StatusCode::CONFLICT);

    // The admin adds a player, who can sign in but not administer.
    let player = json!({"username": "kid", "password": "password123"});
    assert_eq!(h.call("POST", "/api/users", Some(player.clone())).await.0, StatusCode::CREATED);
    assert_eq!(h.call("POST", "/api/users", Some(player)).await.0, StatusCode::CONFLICT);
    assert_eq!(h.call("POST", "/api/auth/logout", None).await.0, StatusCode::NO_CONTENT);
    h.cookie = None;

    let wrong = json!({"username": "kid", "password": "nope-nope"});
    assert_eq!(h.call("POST", "/api/auth/login", Some(wrong)).await.0, StatusCode::UNAUTHORIZED);
    let nobody = json!({"username": "ghost", "password": "password123"});
    assert_eq!(h.call("POST", "/api/auth/login", Some(nobody)).await.0, StatusCode::UNAUTHORIZED);
    let right = json!({"username": "KID", "password": "password123"});
    let (status, user) = h.call("POST", "/api/auth/login", Some(right)).await;
    assert_eq!((status, user["is_admin"].clone()), (StatusCode::OK, json!(false)));
    assert_eq!(h.call("GET", "/api/users", None).await.0, StatusCode::FORBIDDEN);
    assert_eq!(h.call("POST", "/api/library/scan", None).await.0, StatusCode::FORBIDDEN);
    assert_eq!(h.call("GET", "/api/systems", None).await.0, StatusCode::OK);
}

#[tokio::test]
async fn throttles_guessing_and_serves_ops_routes() {
    let mut h = Harness::new("ops", false).await;
    h.sign_up_admin().await;
    h.cookie = None;
    let wrong = json!({"username": "admin", "password": "wrong-password"});
    for _ in 0..crate::ops::MAX_FAILURES {
        assert_eq!(
            h.call("POST", "/api/auth/login", Some(wrong.clone())).await.0,
            StatusCode::UNAUTHORIZED
        );
    }
    let (status, body) = h.call("POST", "/api/auth/login", Some(wrong)).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert!(body["error"].as_str().unwrap().contains("try again"));
    // Locked out even with the right password, until the window passes.
    let right = json!({"username": "admin", "password": "password123"});
    assert_eq!(
        h.call("POST", "/api/auth/login", Some(right)).await.0,
        StatusCode::TOO_MANY_REQUESTS
    );

    let health = Request::builder().uri("/health").body(Body::empty()).unwrap();
    let response = router(h.state.clone()).oneshot(health).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["x-content-type-options"], "nosniff");
    assert_eq!(response.headers()["x-frame-options"], "DENY");
    assert_eq!(
        h.call("GET", "/metrics", None).await.0,
        StatusCode::NOT_FOUND,
        "off without a token"
    );
    assert_eq!(h.call("GET", "/api/config", None).await.0, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn browses_the_library() {
    let mut h = Harness::new("library", false).await;
    h.sign_up_admin().await;
    assert_eq!(h.call("GET", "/api/config", None).await.1, json!({"ice_servers": []}));
    let (_, systems) = h.call("GET", "/api/systems", None).await;
    assert_eq!(system(&systems, "nes")["game_count"], 1);
    assert_eq!(system(&systems, "nes")["name"], "Nintendo Entertainment System");
    assert_eq!(system(&systems, "nes")["playable"], false, "no cores installed");
    assert_eq!(system(&systems, "snes")["game_count"], 0);

    let (_, games) = h.call("GET", "/api/systems/nes/games", None).await;
    assert_eq!(games[0]["title"], "Test Game");
    assert_eq!(games[0]["tags"], "USA");
    assert_eq!(games[0]["last_played_at"], Value::Null);
    assert_eq!(games[0]["media"], json!(["thumb"]));
    let id = games[0]["id"].as_i64().unwrap();
    assert_eq!(
        h.call("GET", &format!("/api/games/{id}/media/thumb"), None).await.0,
        StatusCode::OK
    );
    assert_eq!(
        h.call("GET", &format!("/api/games/{id}/media/video"), None).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        h.call("GET", &format!("/api/games/{id}/media/../../etc"), None).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(h.call("GET", &format!("/api/games/{id}"), None).await.1["system"], "nes");
    assert_eq!(h.call("GET", "/api/systems/saturn/games", None).await.0, StatusCode::NOT_FOUND);
    assert_eq!(h.call("GET", "/api/games/9999", None).await.0, StatusCode::NOT_FOUND);

    // A new file appears after a rescan; a deleted one goes.
    std::fs::write(h.dir.join("roms/nes/Second (Japan).nes"), [0u8; 8]).unwrap();
    std::fs::remove_file(h.dir.join("roms/gba/Other.gba")).unwrap();
    let (_, report) = h.call("POST", "/api/library/scan", None).await;
    assert_eq!(report, json!({"found": 2, "added": 1, "removed": 1}));

    // No core: starting a game says so.
    let (status, body) = h.call("POST", "/api/sessions", Some(json!({"game_id": id}))).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(body["error"].as_str().unwrap().contains("no core"));
    assert_eq!(h.call("GET", "/api/nope", None).await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn runs_a_game_session_in_a_worker() {
    let mut h = Harness::new("session", true).await;
    h.sign_up_admin().await;
    let (_, systems) = h.call("GET", "/api/systems", None).await;
    assert_eq!(system(&systems, "nes")["playable"], true);
    let (_, games) = h.call("GET", "/api/systems/nes/games", None).await;
    let id = games[0]["id"].as_i64().unwrap();

    let (status, session) = h.call("POST", "/api/sessions", Some(json!({"game_id": id}))).await;
    assert_eq!(status, StatusCode::CREATED, "{session}");
    assert_eq!(session["core"], "SovEmu Test Core");
    assert_eq!((session["width"].clone(), session["height"].clone()), (json!(256), json!(224)));
    assert_eq!(session["title"], "Test Game");
    let sid = session["id"].as_str().unwrap().to_owned();

    let (_, current) = h.call("GET", "/api/sessions/current", None).await;
    assert_eq!(current["id"], sid.as_str());
    assert_eq!(
        h.call("POST", &format!("/api/sessions/{sid}/reset"), None).await.0,
        StatusCode::NO_CONTENT
    );
    let bad = json!({"type": "offer", "sdp": "not sdp"});
    assert_eq!(
        h.call("POST", &format!("/api/sessions/{sid}/offer"), Some(bad)).await.0,
        StatusCode::BAD_REQUEST
    );

    // Save states: save slot 2, see it listed, load it; an empty slot cannot be loaded.
    let states = format!("/api/games/{id}/states");
    assert_eq!(h.call("GET", &states, None).await.1, json!([]));
    assert_eq!(
        h.call("POST", &format!("/api/sessions/{sid}/states/2"), None).await.0,
        StatusCode::NO_CONTENT
    );
    let (_, slots) = h.call("GET", &states, None).await;
    assert_eq!(slots[0]["slot"], 2);
    assert_eq!(slots[0]["screenshot"], false, "no frame streamed yet");
    assert_eq!(
        h.call("POST", &format!("/api/sessions/{sid}/states/2/load"), None).await.0,
        StatusCode::NO_CONTENT
    );
    let (status, body) = h.call("POST", &format!("/api/sessions/{sid}/states/5/load"), None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().unwrap().contains("empty"));
    assert_eq!(
        h.call("GET", &format!("{states}/2/screenshot"), None).await.0,
        StatusCode::NOT_FOUND
    );
    assert!(h.dir.join("saves/1/nes/states/Test Game (USA)/slot2.state").is_file());
    assert_eq!(h.call("DELETE", &format!("{states}/2"), None).await.0, StatusCode::NO_CONTENT);
    assert_eq!(h.call("GET", &states, None).await.1, json!([]));
    assert_eq!(h.call("DELETE", &format!("{states}/2"), None).await.0, StatusCode::NOT_FOUND);

    // Cheats: added, toggled and removed while the game runs (each change reaches the worker).
    let cheats = format!("/api/games/{id}/cheats");
    let (status, list) = h
        .call("POST", &cheats, Some(json!({"code": " SXIOPO ", "description": "Infinite lives"})))
        .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(list[0]["code"], "SXIOPO");
    assert_eq!(list[0]["enabled"], true);
    let cheat = list[0]["id"].as_i64().unwrap();
    let (_, list) =
        h.call("PATCH", &format!("{cheats}/{cheat}"), Some(json!({"enabled": false}))).await;
    assert_eq!(list[0]["enabled"], false);
    assert_eq!(
        h.call("POST", &cheats, Some(json!({"code": "  "}))).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        h.call("DELETE", &format!("{cheats}/{cheat}"), None).await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(h.call("DELETE", &format!("{cheats}/{cheat}"), None).await.0, StatusCode::NOT_FOUND);
    assert_eq!(
        h.call("POST", "/api/games/9999/cheats", Some(json!({"code": "X"}))).await.0,
        StatusCode::NOT_FOUND
    );

    // Play history is recorded.
    let (_, games) = h.call("GET", "/api/systems/nes/games", None).await;
    assert_eq!(games[0]["play_count"], 1);

    // Starting another game replaces the session; stopping ends it.
    let (_, second) = h.call("POST", "/api/sessions", Some(json!({"game_id": id}))).await;
    assert_ne!(second["id"], sid.as_str());
    assert_eq!(h.state.workers.count(), 1);
    let second_id = second["id"].as_str().unwrap().to_owned();
    assert_eq!(
        h.call("DELETE", &format!("/api/sessions/{second_id}"), None).await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(h.state.workers.count(), 0);
    assert_eq!(h.call("GET", "/api/sessions/current", None).await.1, Value::Null);
    assert_eq!(
        h.call("DELETE", &format!("/api/sessions/{sid}"), None).await.0,
        StatusCode::NOT_FOUND
    );
}

/// A stand-in for ScreenScraper and TheGamesDB on a local port. ScreenScraper answers for the
/// NES game (checking the exact query a real client must send) and not for the GBA one;
/// TheGamesDB answers for the GBA one, with box art. `quota` makes ScreenScraper refuse.
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
        let (_, status) = h.call("GET", "/api/metadata/status", None).await;
        if status["running"] == false && status["done"].as_u64() > Some(0) {
            return status;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("the metadata job did not finish");
}

#[tokio::test]
async fn fetches_metadata_from_both_services() {
    let base = mock_services(false).await;
    let mut h = Harness::new_with("metadata", false, metadata_args(&base)).await;
    h.sign_up_admin().await;
    let (_, status) = h.call("GET", "/api/metadata/status", None).await;
    assert_eq!(status["sources"], json!(["fbneo", "screenscraper", "thegamesdb"]));

    assert_eq!(h.call("POST", "/api/metadata/fetch", None).await.0, StatusCode::ACCEPTED);
    let status = wait_for_job(&mut h).await;
    assert_eq!(
        (status["found"].clone(), status["failed"].clone()),
        (json!(2), json!(0)),
        "{status}"
    );

    // ScreenScraper named the NES game and described it; the file's own title is kept too.
    let (_, nes) = h.call("GET", "/api/systems/nes/games", None).await;
    assert_eq!(nes[0]["title"], "Castlevania");
    assert_eq!(nes[0]["file_title"], "Test Game");
    assert_eq!(nes[0]["developer"], "Konami");
    assert_eq!(nes[0]["release_date"], "1987-05-01");
    assert_eq!(nes[0]["rating"], 0.8);

    // TheGamesDB filled in the GBA game, and its box art stands in for the missing thumb.
    let (_, gba) = h.call("GET", "/api/systems/gba/games", None).await;
    assert_eq!(gba[0]["description"], "From TGDB");
    assert_eq!(gba[0]["developer"], "Dev Co");
    assert_eq!(gba[0]["players"], "2");
    assert_eq!(gba[0]["media"], json!(["thumb"]));
    let id = gba[0]["id"].as_i64().unwrap();
    assert_eq!(
        h.call("GET", &format!("/api/games/{id}/media/thumb"), None).await.0,
        StatusCode::OK
    );

    // Looked-up games are not asked for again.
    assert_eq!(
        h.call("POST", "/api/metadata/fetch", Some(json!({}))).await.0,
        StatusCode::ACCEPTED
    );
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert_eq!(h.call("GET", "/api/metadata/status", None).await.1["total"], 0);
}

#[tokio::test]
async fn stops_a_service_that_is_out_of_quota() {
    let base = mock_services(true).await;
    let mut h = Harness::new_with("metadata-quota", false, metadata_args(&base)).await;
    h.sign_up_admin().await;
    h.call("POST", "/api/metadata/fetch", Some(json!({"system": "nes"}))).await;
    let status = wait_for_job(&mut h).await;
    assert_eq!(status["total"], 1, "only the system asked for");
    assert!(status["notes"][0].as_str().unwrap().contains("quota"), "{status}");
    // A failed lookup is not stored, so the next job retries it.
    let (_, nes) = h.call("GET", "/api/systems/nes/games", None).await;
    assert_eq!(nes[0]["description"], Value::Null);
    h.call("POST", "/api/metadata/fetch", Some(json!({"system": "nes"}))).await;
    assert_eq!(wait_for_job(&mut h).await["total"], 1);
}

#[tokio::test]
async fn mirror_mode_copies_the_library_and_plays_from_it() {
    let mut h =
        Harness::new_with("mirror", false, vec!["--library-mode".into(), "mirror".into()]).await;
    h.sign_up_admin().await;
    assert_eq!(h.state.config.roms_dir, h.dir.join("library"), "the server plays from the mirror");
    let (_, status) = h.call("GET", "/api/library/status", None).await;
    assert_eq!((status["mode"].clone(), status["games"].clone()), (json!("mirror"), json!(0)));

    crate::admin::sync_then_scan(h.state.clone()).await;
    assert!(
        h.dir.join("library/nes/images/Test Game (USA)-thumb.png").is_file(),
        "artwork is mirrored too"
    );
    let (_, status) = h.call("GET", "/api/library/status", None).await;
    assert_eq!(status["games"], 2);
    assert_eq!(status["mirror"]["files_done"], 3);
    assert_eq!(status["mirror"]["running"], false);
    assert_eq!(h.call("POST", "/api/library/sync", None).await.0, StatusCode::ACCEPTED);
}

#[tokio::test]
async fn library_status_reports_direct_mode() {
    let mut h = Harness::new("direct", false).await;
    h.sign_up_admin().await;
    let (_, status) = h.call("GET", "/api/library/status", None).await;
    assert_eq!(status, json!({"mode": "direct", "games": 2, "cache_bytes": null, "mirror": null}));
    assert_eq!(h.call("POST", "/api/library/sync", None).await.0, StatusCode::BAD_REQUEST);
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
    let (_, status) = h.call("GET", "/api/auth/status", None).await;
    assert_eq!(status["setup_required"], false, "no first-run page when the admin is configured");
    let login = |pw: &str| json!({"username": "boss", "password": pw});
    let (code, user) = h.call("POST", "/api/auth/login", Some(login("first-password"))).await;
    assert_eq!((code, user["is_admin"].clone()), (StatusCode::OK, json!(true)));

    // Restarting with a new password rotates it and signs existing browsers out.
    let config = Config::parse_from(
        ["sovemu-server", "--data-dir", &h.dir.display().to_string()]
            .into_iter()
            .map(str::to_owned)
            .chain(admin("second-password")),
    );
    let state = build_state(config).await.unwrap();
    h.state = state;
    assert_eq!(
        h.call("GET", "/api/systems", None).await.0,
        StatusCode::UNAUTHORIZED,
        "old session gone"
    );
    h.cookie = None;
    assert_eq!(
        h.call("POST", "/api/auth/login", Some(login("first-password"))).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        h.call("POST", "/api/auth/login", Some(login("second-password"))).await.0,
        StatusCode::OK
    );
}

#[tokio::test]
async fn declared_users_are_reconciled_and_pruned() {
    let mut h = Harness::new("env-users", false).await;
    h.sign_up_admin().await;
    let dir = h.dir.display().to_string();
    let restart = |users: Vec<crate::structured::UserSpec>, prune: bool| {
        let mut config = Config::parse_from(["sovemu-server", "--data-dir", &dir]);
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
    let (code, me) = h
        .call(
            "POST",
            "/api/auth/login",
            Some(json!({"username": "kid", "password": "password-kid"})),
        )
        .await;
    assert_eq!((code, me["is_admin"].clone()), (StatusCode::OK, json!(false)));
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
    let (_, systems) = h.call("GET", "/api/systems", None).await;
    let ids: Vec<&str> =
        systems.as_array().unwrap().iter().map(|s| s["id"].as_str().unwrap()).collect();
    assert!(!ids.contains(&"gba"), "disabled systems are hidden");
    assert_eq!(h.call("GET", "/api/systems/gba/games", None).await.0, StatusCode::NOT_FOUND);

    // NES now needs nestopia: an fceumm core does not count.
    let cores = h.state.config.cores_dir.clone();
    std::fs::write(cores.join("fceumm_libretro.so"), b"").unwrap();
    let (_, systems) = h.call("GET", "/api/systems", None).await;
    let nes = systems.as_array().unwrap().iter().find(|s| s["id"] == "nes").unwrap().clone();
    assert_eq!(nes["playable"], false);
    std::fs::write(cores.join("nestopia_libretro.so"), b"").unwrap();
    let (_, systems) = h.call("GET", "/api/systems", None).await;
    let nes = systems.as_array().unwrap().iter().find(|s| s["id"] == "nes").unwrap().clone();
    assert_eq!(nes["playable"], true);
}
