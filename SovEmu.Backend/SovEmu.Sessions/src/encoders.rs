//! Which video encoders work on this host and what a frame costs with each, found once at
//! startup.
//!
//! As for the GPU ([`crate::gpu`]), presence is not proof: the server runs `sovemu-worker
//! encoders`, which runs every encoder for real on a moving picture and times it, and keeps the
//! answer. (The server never loads a codec itself; the test runs in a worker like everything
//! else that does.) The server waits for it before it listens, so no game starts without it,
//! and the whole test may take at most [`BUDGET`].

use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

/// The longest the whole test may take.
pub const BUDGET: Duration = Duration::from_secs(10);
/// How long past [`BUDGET`] a worker that has not answered is given before it is stopped.
const GRACE: Duration = Duration::from_secs(3);

/// One picture size's result for an encoder.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Timing {
    pub width: u32,
    pub height: u32,
    /// Frames timed.
    pub frames: u32,
    /// Milliseconds a frame: colour conversion, any upload to a GPU, and the encode.
    pub mean_ms: f64,
    pub p95_ms: f64,
}

/// One encoder the worker tried.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Encoder {
    /// `vp8` or `h264`.
    pub codec: String,
    /// `libvpx`, `vaapi`, `nvenc` or `x264`.
    pub encoder: String,
    /// It encodes on a GPU.
    pub hardware: bool,
    /// CPU threads a frame is encoded on; none for a GPU's encoder.
    pub threads: Option<u32>,
    pub works: bool,
    /// Why it does not work: it did not open, failed, held frames back, or ran out of time.
    pub error: Option<String>,
    /// A row for each size it finished, smallest first.
    #[serde(default)]
    pub timings: Vec<Timing>,
}

/// What `sovemu-worker encoders` prints.
#[derive(Debug, Clone, Default, Deserialize)]
struct Report {
    #[serde(default)]
    encoders: Vec<Encoder>,
    #[serde(default)]
    took_ms: u64,
}

/// The test's answer, kept for the life of the process and shown to admins.
#[derive(Debug, Clone, Default, Serialize)]
pub struct EncoderTest {
    pub encoders: Vec<Encoder>,
    /// How long the test took.
    pub took_ms: u64,
    /// Why the test gave no answer (the worker could not be run, or did not answer in time).
    pub note: Option<String>,
    /// When it ran (once, at startup).
    #[serde(skip)]
    pub tested_at: Option<SystemTime>,
}

impl EncoderTest {
    /// A test that gave no answer, and why.
    fn failed(note: String) -> Self {
        Self { note: Some(note), ..Self::default() }
    }

    /// The result in lines, for the startup log.
    pub fn summary(&self) -> String {
        let mut lines: Vec<String> = self
            .encoders
            .iter()
            .map(|e| {
                let how = match e.threads {
                    Some(threads) => format!("{threads} threads"),
                    None => "GPU".to_owned(),
                };
                let result = if e.works {
                    let sizes: Vec<String> = e
                        .timings
                        .iter()
                        .map(|t| format!("{}x{} {:.2} ms", t.width, t.height, t.mean_ms))
                        .collect();
                    format!("works: {}", sizes.join(", "))
                } else {
                    format!("does not work: {}", e.error.as_deref().unwrap_or("no reason given"))
                };
                format!("  {} {} ({how}): {result}", e.codec, e.encoder)
            })
            .collect();
        if let Some(note) = &self.note {
            lines.push(format!("  the encoder test gave no answer: {note}"));
        }
        lines.push(format!("  took {} ms", self.took_ms));
        lines.join("\n")
    }
}

/// Runs the test in a worker (with the workers' cleared environment). `gpu` is what sessions
/// get as `--gpu`: a render node is where VA-API encodes.
pub async fn test(worker: &Path, gpu: &str, env: &[(String, String)]) -> EncoderTest {
    let mut command = tokio::process::Command::new(worker);
    command
        .arg("encoders")
        .arg("--gpu")
        .arg(gpu)
        .arg("--budget-ms")
        .arg(BUDGET.as_millis().to_string())
        .env_clear()
        .envs(env.iter().cloned())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let started = std::time::Instant::now();
    let output = match tokio::time::timeout(BUDGET + GRACE, command.output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(e)) => return EncoderTest::failed(format!("running the encoder test: {e}")),
        Err(_) => return EncoderTest::failed("the encoder test did not answer in time".into()),
    };
    let mut test = match serde_json::from_slice::<Report>(&output.stdout) {
        Ok(report) => EncoderTest {
            encoders: report.encoders,
            took_ms: report.took_ms,
            note: None,
            tested_at: None,
        },
        Err(e) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let detail = stderr.lines().last().unwrap_or_default().to_owned();
            EncoderTest {
                took_ms: started.elapsed().as_millis() as u64,
                ..EncoderTest::failed(format!("the encoder test failed ({e}): {detail}"))
            }
        }
    };
    test.tested_at = Some(SystemTime::now());
    test
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_workers_report_reads_and_summarises() {
        let json = r#"{
            "encoders": [
                {"codec": "vp8", "encoder": "libvpx", "hardware": false, "threads": 4, "works": true,
                 "error": null, "timings": [
                    {"width": 256, "height": 224, "frames": 60, "mean_ms": 0.79, "p95_ms": 0.98},
                    {"width": 640, "height": 448, "frames": 60, "mean_ms": 2.63, "p95_ms": 3.23}]},
                {"codec": "h264", "encoder": "vaapi", "hardware": true, "threads": null,
                 "works": false, "error": "holds 1 frame(s) back", "timings": []}
            ],
            "took_ms": 1033, "budget_ms": 10000
        }"#;
        let report: Report = serde_json::from_str(json).unwrap();
        let test = EncoderTest {
            encoders: report.encoders,
            took_ms: report.took_ms,
            note: None,
            tested_at: None,
        };
        assert_eq!(
            test.summary(),
            "  vp8 libvpx (4 threads): works: 256x224 0.79 ms, 640x448 2.63 ms\n  \
             h264 vaapi (GPU): does not work: holds 1 frame(s) back\n  took 1033 ms"
        );
    }

    #[tokio::test]
    async fn a_worker_that_cannot_be_run_leaves_a_note_not_an_error() {
        let test = super::test(Path::new("/no/such/sovemu-worker"), "auto", &[]).await;
        assert!(test.encoders.is_empty());
        assert!(test.note.as_deref().unwrap().starts_with("running the encoder test"));
        assert!(test.summary().contains("gave no answer"));
    }
}
