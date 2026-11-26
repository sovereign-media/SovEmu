//! Runs the `sovemu-worker headless` binary end to end against `SovEmu.TestCore`.

use std::process::Command;

/// Builds the test core and returns the path of its shared library (see the RetroHost tests).
#[path = "../../SovEmu.TestCore/locate.rs"]
mod locate;
use locate::test_core;

#[test]
fn headless_run_writes_frames_audio_and_summary() {
    let out = std::env::temp_dir().join(format!("sovemu-headless-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&out);

    let output = Command::new(env!("CARGO_BIN_EXE_sovemu-worker"))
        .arg("headless")
        .arg("--core")
        .arg(test_core())
        .arg("--out")
        .arg(&out)
        .arg("--system-dir")
        .arg(out.join("system"))
        .args(["--frames", "30", "--png-every", "10", "--hold", "5-9:select"])
        .args(["--var", "sovemu_test_pattern=solid"])
        .output()
        .expect("run sovemu-worker");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));

    for frame in ["frame_000000.png", "frame_000010.png", "frame_000020.png", "last.png"] {
        assert!(out.join(frame).is_file(), "missing {frame}");
    }
    assert!(!out.join("frame_000030.png").exists());

    let summary: serde_json::Value =
        serde_json::from_slice(&std::fs::read(out.join("summary.json")).unwrap()).unwrap();
    assert_eq!(summary["core"]["name"], "SovEmu Test Core");
    assert_eq!(summary["frames_run"], 30);
    assert_eq!(summary["frames_duped"], 5, "SELECT held for frames 5-9 dupes them");
    assert_eq!(summary["av"]["width"], 256);
    assert_eq!(summary["audio"]["stereo_frames"], 30 * 800);
    assert_eq!(summary["core"]["variables"][0][1], "solid");

    let wav = hound::WavReader::open(out.join("audio.wav")).unwrap();
    assert_eq!(wav.spec().sample_rate, 48_000);
    assert_eq!(wav.spec().channels, 2);
    assert_eq!(wav.duration(), 30 * 800);

    let png = png::Decoder::new(std::io::BufReader::new(
        std::fs::File::open(out.join("last.png")).unwrap(),
    ));
    let mut reader = png.read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    let frame = reader.next_frame(&mut buf).unwrap();
    assert_eq!((frame.width, frame.height), (256, 224));
    assert_eq!(&buf[..4], &[0x20, 0x40, 0x80, 255], "solid pattern from --var");

    std::fs::remove_dir_all(&out).ok();
}

/// The red of the test core's top-left pixel: its frame number, mod 256.
fn frame_shown(png: &std::path::Path) -> u8 {
    let decoder = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(png).unwrap()));
    let mut reader = decoder.read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    reader.next_frame(&mut buf).unwrap();
    buf[0]
}

#[test]
fn run_ahead_shows_frames_ahead_and_keeps_the_real_timeline() {
    let out = std::env::temp_dir().join(format!("sovemu-runahead-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&out);
    let output = Command::new(env!("CARGO_BIN_EXE_sovemu-worker"))
        .arg("headless")
        .arg("--core")
        .arg(test_core())
        .arg("--out")
        .arg(&out)
        .arg("--system-dir")
        .arg(out.join("system"))
        .args(["--frames", "10", "--png-every", "1", "--run-ahead", "2"])
        .output()
        .expect("run sovemu-worker");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    // Real frame n draws frame n; two frames ahead, the picture is frame n + 2's.
    for n in [0u8, 5, 9] {
        assert_eq!(frame_shown(&out.join(format!("frame_{n:06}.png"))), n + 2, "frame {n}");
    }
    // Each picture is one frame on from the last: the state was put back after every look
    // ahead, or the game would race ahead by three frames a tick.
    for n in 1..10u8 {
        let (before, now) = (
            frame_shown(&out.join(format!("frame_{:06}.png", n - 1))),
            frame_shown(&out.join(format!("frame_{n:06}.png"))),
        );
        assert_eq!(now, before + 1, "frame {n}");
    }
    let summary: serde_json::Value =
        serde_json::from_slice(&std::fs::read(out.join("summary.json")).unwrap()).unwrap();
    assert_eq!(summary["frames_run"], 10);
    // The sound is the real frames' only: the ones run ahead add none.
    assert_eq!(summary["audio"]["stereo_frames"], 10 * 800);
    std::fs::remove_dir_all(&out).ok();
}

#[test]
fn probe_reports_cores_and_fails_on_a_bad_one() {
    let core = test_core();
    let output = Command::new(env!("CARGO_BIN_EXE_sovemu-worker"))
        .arg("probe")
        .arg(core)
        .output()
        .expect("run sovemu-worker probe");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report[0]["name"], "SovEmu Test Core");
    assert_eq!(report[0]["valid_extensions"][0], "sovtest");
    assert_eq!(report[0]["supports_no_game"], true);

    // A directory expands to the libraries in it; a library that is not a core fails the probe.
    let dir = std::env::temp_dir().join(format!("sovemu-probe-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let not_a_core = dir.join(format!("broken.{}", std::env::consts::DLL_EXTENSION));
    std::fs::write(&not_a_core, b"not a shared library").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_sovemu-worker"))
        .arg("probe")
        .arg(core)
        .arg(&dir)
        .output()
        .expect("run sovemu-worker probe");
    std::fs::remove_dir_all(&dir).ok();
    assert!(!output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report.as_array().unwrap().len(), 2);
    assert!(report[1]["error"].as_str().unwrap().contains("cannot open core"));
}

#[test]
fn a_bad_core_path_fails_cleanly() {
    let out = std::env::temp_dir().join(format!("sovemu-headless-bad-{}", std::process::id()));
    let output = Command::new(env!("CARGO_BIN_EXE_sovemu-worker"))
        .args(["headless", "--core", "/nonexistent/core.so", "--frames", "1"])
        .arg("--out")
        .arg(&out)
        .arg("--system-dir")
        .arg(out.join("system"))
        .output()
        .expect("run sovemu-worker");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("loading core"));
    std::fs::remove_dir_all(&out).ok();
}

#[test]
fn json_logs_carry_the_session_id() {
    let out = std::env::temp_dir().join(format!("sovemu-headless-json-{}", std::process::id()));
    let output = Command::new(env!("CARGO_BIN_EXE_sovemu-worker"))
        .args(["headless", "--core", "/nonexistent/core.so", "--frames", "1"])
        .args(["--log-format", "json", "--session-id", "abc123"])
        .arg("--out")
        .arg(&out)
        .arg("--system-dir")
        .arg(out.join("system"))
        .output()
        .expect("run sovemu-worker");
    std::fs::remove_dir_all(&out).ok();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    let line = stderr.lines().last().expect("a log line");
    let event: serde_json::Value = serde_json::from_str(line).expect(line);
    assert_eq!(event["level"], "ERROR");
    assert_eq!(event["message"], "worker failed");
    assert!(event["error"].as_str().unwrap().contains("loading core"), "{line}");
    assert_eq!(event["span"]["session.id"], "abc123", "{line}");
}
