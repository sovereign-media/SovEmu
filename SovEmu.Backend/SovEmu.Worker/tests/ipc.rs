//! `sovemu-worker session` over its real stdin/stdout: readiness, control requests and save
//! states, against `SovEmu.TestCore`.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use sovemu_protocol::ipc::{Reply, Request};

fn test_core() -> PathBuf {
    if let Some(path) = std::env::var_os("SOVEMU_TEST_CORE") {
        return PathBuf::from(path);
    }
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let status = Command::new(cargo)
        .args(["build", "--quiet", "-p", "sovemu-test-core"])
        .current_dir(&workspace)
        .status()
        .expect("run cargo");
    assert!(status.success());
    std::env::var_os("CARGO_TARGET_DIR")
        .map_or_else(|| workspace.join("target"), PathBuf::from)
        .join("debug")
        .join(format!(
            "{}sovemu_test_core{}",
            std::env::consts::DLL_PREFIX,
            std::env::consts::DLL_SUFFIX
        ))
}

struct Worker {
    child: Child,
    stdin: ChildStdin,
    replies: mpsc::Receiver<Reply>,
}

impl Worker {
    fn start(dir: &std::path::Path) -> (Self, Reply) {
        let rom = dir.join("Game (USA).sovtest");
        std::fs::write(&rom, [0u8; 32]).unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_sovemu-worker"))
            .arg("session")
            .arg("--core")
            .arg(test_core())
            .arg("--rom")
            .arg(&rom)
            .args(["--udp", "127.0.0.1:0", "--idle-timeout", "0"])
            .arg("--system-dir")
            .arg(dir.join("system"))
            .arg("--save-dir")
            .arg(dir.join("saves"))
            .arg("--state-dir")
            .arg(dir.join("states"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let (tx, replies) = mpsc::channel();
        std::thread::spawn(move || {
            for line in stdout.lines() {
                let line = line.unwrap();
                let reply: Reply =
                    serde_json::from_str(&line).expect("stdout carries only protocol");
                if tx.send(reply).is_err() {
                    break;
                }
            }
        });
        let ready = replies.recv_timeout(Duration::from_secs(10)).expect("ready");
        (Self { child, stdin, replies }, ready)
    }

    fn send(&mut self, request: Request) -> Reply {
        writeln!(self.stdin, "{}", serde_json::to_string(&request).unwrap()).unwrap();
        self.replies.recv_timeout(Duration::from_secs(10)).expect("a reply")
    }
}

#[test]
fn control_requests_and_save_states() {
    let dir = std::env::temp_dir().join(format!("sovemu-ipc-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let (mut worker, ready) = Worker::start(&dir);
    assert!(
        matches!(ready, Reply::Ready { ref core, width: 256, height: 224, .. } if core == "SovEmu Test Core"),
        "{ready:?}"
    );

    assert_eq!(worker.send(Request::Reset { id: 1 }), Reply::Done { id: 1 });
    match worker.send(Request::LoadState { id: 2, slot: 3 }) {
        Reply::Error { id: 2, message } => {
            assert!(message.contains("slot 3 is empty"), "{message}")
        }
        other => panic!("{other:?}"),
    }
    match worker.send(Request::SaveState { id: 3, slot: 42 }) {
        Reply::Error { id: 3, message } => assert!(message.contains("out of range"), "{message}"),
        other => panic!("{other:?}"),
    }
    assert_eq!(worker.send(Request::SaveState { id: 4, slot: 1 }), Reply::Done { id: 4 });
    assert_eq!(std::fs::metadata(dir.join("states/slot1.state")).unwrap().len(), 16);
    assert!(!dir.join("states/slot1.png").exists(), "no frame has been shown yet");
    assert_eq!(worker.send(Request::LoadState { id: 5, slot: 1 }), Reply::Done { id: 5 });

    // Shutdown replies, then exits. Nothing was played, so there is no auto state.
    assert_eq!(worker.send(Request::Shutdown { id: 6 }), Reply::Done { id: 6 });
    let status = worker.child.wait().unwrap();
    assert!(status.success());
    assert!(!dir.join("states/slot0.state").exists());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn exits_when_stdin_closes() {
    let dir = std::env::temp_dir().join(format!("sovemu-ipc-eof-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (worker, _) = Worker::start(&dir);
    let Worker { mut child, stdin, .. } = worker;
    drop(stdin);
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        assert!(std::time::Instant::now() < deadline, "worker kept running without a server");
        std::thread::sleep(Duration::from_millis(50));
    }
    std::fs::remove_dir_all(&dir).ok();
}
