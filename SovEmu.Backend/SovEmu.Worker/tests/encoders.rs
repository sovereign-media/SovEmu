//! `sovemu-worker encoders` as the server runs it at startup: every encoder run for real and
//! timed, inside a budget.

use std::process::Command;
use std::time::{Duration, Instant};

use serde_json::Value;

fn run(budget_ms: u64) -> (Value, Duration) {
    let started = Instant::now();
    let output = Command::new(env!("CARGO_BIN_EXE_sovemu-worker"))
        .args(["encoders", "--gpu", "auto", "--budget-ms", &budget_ms.to_string()])
        .output()
        .expect("run sovemu-worker");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let report = serde_json::from_slice(&output.stdout).expect("a JSON report, alone on stdout");
    (report, started.elapsed())
}

fn encoder<'a>(report: &'a Value, name: &str) -> &'a Value {
    report["encoders"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["encoder"] == name)
        .unwrap_or_else(|| panic!("no {name} in {report}"))
}

#[test]
fn every_encoder_is_run_as_a_session_runs_it_and_timed() {
    let (report, _) = run(10_000);
    assert!(report["took_ms"].as_u64().unwrap() <= 10_000, "{report}");

    // The software encoders are in every build: VP8 on libvpx's four threads, H.264 on x264's.
    for (name, codec) in [("libvpx", "vp8"), ("x264", "h264")] {
        let e = encoder(&report, name);
        assert_eq!(
            (&e["codec"], &e["hardware"], &e["threads"]),
            (&codec.into(), &false.into(), &4.into())
        );
        // A busy machine may not finish every size in its share; what it finished is timed.
        if e["works"] == true {
            let sizes: Vec<_> = e["timings"]
                .as_array()
                .unwrap()
                .iter()
                .map(|t| (t["width"].as_u64(), t["height"].as_u64()))
                .collect();
            assert_eq!(
                sizes,
                [(Some(256), Some(224)), (Some(640), Some(448)), (Some(1280), Some(720))]
            );
            for t in e["timings"].as_array().unwrap() {
                assert_eq!(t["frames"], 60);
                let (mean, p95) = (t["mean_ms"].as_f64().unwrap(), t["p95_ms"].as_f64().unwrap());
                assert!(mean > 0.0 && p95 >= mean * 0.5, "{t}");
            }
        } else {
            assert!(e["error"].as_str().unwrap().contains("ran out of time"), "{e}");
        }
    }
    // A GPU's encoder is tried where there may be one, and either works or says why not.
    for e in report["encoders"].as_array().unwrap() {
        assert_eq!(e["works"] == true, e["error"].is_null(), "{e}");
        if e["hardware"] == true {
            assert!(e["threads"].is_null(), "{e}");
        }
    }
}

#[test]
fn an_encoder_out_of_time_is_reported_as_not_working() {
    // No time at all: nothing can finish, and the report still comes at once.
    let (report, took) = run(0);
    assert!(took < Duration::from_secs(8), "the report waited {took:?}");
    for e in report["encoders"].as_array().unwrap() {
        assert_eq!(e["works"], false, "{e}");
        assert!(e["error"].is_string(), "{e}");
    }
    assert_eq!(encoder(&report, "libvpx")["error"], "ran out of time: too slow to be of use");
}
