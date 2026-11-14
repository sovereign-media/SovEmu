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
        let dir = std::env::temp_dir().join(format!("sovemu-server-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for (path, bytes) in [("roms/nes/Test Game (USA).nes", 64), ("roms/gba/Other.gba", 32)] {
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
        let state = build_state(Config::parse_from(args)).await.unwrap();
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
async fn browses_the_library() {
    let mut h = Harness::new("library", false).await;
    h.sign_up_admin().await;
    let (_, systems) = h.call("GET", "/api/systems", None).await;
    assert_eq!(system(&systems, "nes")["game_count"], 1);
    assert_eq!(system(&systems, "nes")["name"], "Nintendo Entertainment System");
    assert_eq!(system(&systems, "nes")["playable"], false, "no cores installed");
    assert_eq!(system(&systems, "snes")["game_count"], 0);

    let (_, games) = h.call("GET", "/api/systems/nes/games", None).await;
    assert_eq!(games[0]["title"], "Test Game");
    assert_eq!(games[0]["tags"], "USA");
    assert_eq!(games[0]["last_played_at"], Value::Null);
    let id = games[0]["id"].as_i64().unwrap();
    assert_eq!(h.call("GET", &format!("/api/games/{id}"), None).await.1["system"], "nes");
    assert_eq!(h.call("GET", "/api/systems/psx/games", None).await.0, StatusCode::NOT_FOUND);
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
