//! `sovemu-worker session` over its real stdin/stdout: readiness, control requests and save
//! states, against `SovEmu.TestCore`.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use sovemu_protocol::ipc::{ExitReason, LoadPhase, Reply, Request};

#[path = "../../SovEmu.TestCore/locate.rs"]
mod locate;
use locate::test_core;

struct Worker {
    child: Child,
    stdin: ChildStdin,
    replies: mpsc::Receiver<Reply>,
    /// The worker's stderr, kept so a test that loses it can say why.
    log: std::path::PathBuf,
}

impl Worker {
    fn start(dir: &std::path::Path) -> (Self, Reply) {
        let rom = dir.join("Game (USA).sovtest");
        let log = dir.join("worker.log");
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
            .stderr(std::fs::File::create(&log).unwrap())
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
        // Progress first (opening the core, then the game), the core's options, then ready.
        let mut phases = Vec::new();
        let mut options = None;
        let ready = loop {
            match replies
                .recv_timeout(Duration::from_secs(10))
                .unwrap_or_else(|e| panic!("no ready ({e}); the worker said:\n{}", read_log(&log)))
            {
                Reply::Progress { phase, .. } => phases.push(phase),
                Reply::Options { options: o, categories } => options = Some((o, categories)),
                other => break other,
            }
        };
        phases.dedup();
        assert_eq!(phases, [LoadPhase::Core, LoadPhase::Content], "loading progress before ready");
        let (options, categories) = options.expect("the core's options before ready");
        let keys: Vec<&str> = options.iter().map(|o| o.key.as_str()).collect();
        assert_eq!(keys, ["sovemu_test_pattern", "sovemu_test_render"]);
        assert_eq!(options[0].labels, ["Gradient", "Solid colour", "Noise"]);
        assert_eq!(categories[0].description, "Video");
        (Self { child, stdin, replies, log }, ready)
    }

    fn send(&mut self, request: Request) -> Reply {
        writeln!(self.stdin, "{}", serde_json::to_string(&request).unwrap()).unwrap();
        self.replies.recv_timeout(Duration::from_secs(10)).unwrap_or_else(|e| {
            let status = self.child.try_wait().ok().flatten();
            panic!(
                "no reply ({e}; worker exit: {status:?}); the worker said:\n{}",
                read_log(&self.log)
            )
        })
    }
}

fn read_log(path: &std::path::Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| format!("(no log: {e})"))
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
    let codes = vec!["SXIOPO".to_owned(), "AAAAAA".to_owned()];
    assert_eq!(worker.send(Request::SetCheats { id: 7, codes }), Reply::Done { id: 7 });
    assert_eq!(worker.send(Request::SetCheats { id: 8, codes: vec![] }), Reply::Done { id: 8 });
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
    // Its last word says why, so the server can tell this from a crash.
    let last = worker.replies.recv_timeout(Duration::from_secs(10)).expect("an exiting reply");
    assert_eq!(last, Reply::Exiting { reason: ExitReason::Stopped });
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
