//! `sovemu-worker session`: one game session driven by `sovemu-server`.
//!
//! The server talks to the worker in JSON lines ([`sovemu_protocol::ipc`]): requests on stdin,
//! replies and events on stdout. Logs go to stderr, so stdout carries nothing but protocol:
//! before the core loads, stdout is kept for the replies and descriptor 1 (where a core's
//! `printf` goes) is pointed at stderr.
//! The worker exits when stdin closes (the server went away), on `shutdown`, or after
//! `--idle-timeout` seconds with no client connected.

use std::io::{BufRead, Write};
use std::net::UdpSocket;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use sovemu_protocol::ipc::{ExitReason, Reply, Request};

use crate::SessionArgs;
use crate::stream::{self, Command, Session, SessionConfig, StreamEvent};

/// Writes one reply as a JSON line. Shared by the stream thread and the reply threads.
#[derive(Clone)]
struct Out(Arc<Mutex<std::fs::File>>);

impl Out {
    fn send(&self, reply: &Reply) {
        let line = serde_json::to_string(reply).expect("replies serialise");
        let mut out = self.0.lock().unwrap_or_else(|e| e.into_inner());
        // If stdout is gone the server is gone; stdin closing will stop us shortly.
        let _ = writeln!(out, "{line}").and_then(|_| out.flush());
    }
}

pub(crate) fn run(args: SessionArgs) -> Result<()> {
    str0m::crypto::from_feature_flags().install_process_default();
    let stdout = sovemu_retro_host::take_stdout().context("keeping stdout for replies")?;
    let out = Out(Arc::new(Mutex::new(stdout)));
    let report_out = out.clone();
    let report = move |phase, read, total| {
        report_out.send(&Reply::Progress { phase, read, total });
    };
    // Loading failed: the server is told why (the player sees it), then the worker exits.
    let failed = |e: anyhow::Error| {
        out.send(&Reply::Failed { message: format!("{e:#}") });
        e
    };
    // A patch is applied to a copy, which lives until the session ends.
    let patched = match (&args.patch, &args.rom) {
        (Some(patch), Some(rom)) => {
            Some(crate::patch::PatchedRom::write(rom, patch, &args.patch_name).map_err(failed)?)
        }
        _ => None,
    };
    let rom = patched.as_ref().map(|p| p.path.clone()).or_else(|| args.rom.clone());
    let mut session = Session::open(
        &SessionConfig {
            core: args.core.clone(),
            rom,
            system_dir: args.system_dir.clone(),
            bios_path: args.bios_path.clone(),
            save_dir: args.save_dir.clone(),
            variables: args.variables.clone(),
            video_kbps: args.video_kbps,
            video_min_kbps: args.video_min_kbps,
            video_max_kbps: args.video_max_kbps,
            audio_kbps: args.audio_kbps,
            rewind_seconds: args.rewind_seconds,
            rewind_max_mb: args.rewind_max_mb,
            fast_forward: args.fast_forward,
            run_ahead: args.run_ahead,
            health_interval: args.health_interval,
            keyframe_interval: args.keyframe_interval,
            gpu: args.gpu.0.clone(),
            video_codec: args.video_codec,
            video_encoder: args.video_encoder,
            state_dir: args.state_dir.clone(),
        },
        &report,
    )
    .map_err(failed)?;
    let (core, av) = session.describe();
    let discs = session.discs();
    let (options, categories) = session.options();

    let socket = UdpSocket::bind(args.udp)
        .with_context(|| format!("binding UDP {}", args.udp))
        .map_err(failed)?;
    let udp_addr = socket.local_addr()?;
    let advertised = stream::advertised_addr(args.advertise, udp_addr);
    tracing::info!(udp = %udp_addr, advertised = %advertised, "session ready");

    out.send(&Reply::Options { options, categories });
    out.send(&Reply::Ready {
        udp_port: udp_addr.port(),
        core,
        width: av.base_width,
        height: av.base_height,
        fps: av.fps,
        discs,
    });

    let (commands_tx, commands_rx) = mpsc::channel();
    let reader_out = out.clone();
    let replies = Replies::default();
    let reader_replies = replies.clone();
    // The session's span (its id, from the server) carries over to this thread's log lines.
    let span = tracing::Span::current();
    thread::Builder::new().name("stdin".into()).spawn(move || {
        let _span = span.enter();
        for line in std::io::stdin().lock().lines() {
            let Ok(line) = line else { break };
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<Request>(&line) {
                Ok(request) => dispatch(request, &commands_tx, &reader_out, &reader_replies),
                Err(e) => tracing::warn!(line = %line, error = %e, "ignoring a bad request"),
            }
        }
        // Dropping the sender tells the stream loop to stop.
    })?;

    let event_out = out.clone();
    let idle = (args.idle_timeout > 0).then(|| Duration::from_secs(args.idle_timeout));
    let result = stream::run(
        session,
        socket,
        advertised,
        commands_rx,
        &mut |event| {
            event_out.send(&match event {
                StreamEvent::ClientConnected => Reply::ClientConnected,
                StreamEvent::ClientDisconnected => Reply::ClientDisconnected,
                StreamEvent::Pipeline(pipeline) => Reply::Pipeline { pipeline },
                StreamEvent::Health(health) => Reply::Health { health },
                StreamEvent::ClientStats(stats) => Reply::ClientStats { stats },
            })
        },
        idle,
    );
    // Every reply the stream loop gave before it stopped, `shutdown`'s above all, is written
    // before the process exits: the server waits for it, and would otherwise wait out its stop
    // timeout. Each reply thread has its answer, or sees the loop gone, so this is quick.
    replies.finish();
    // The session (moved into the loop) is gone, so its saves are written: say why we stop.
    let reason = result.as_ref().map_or(ExitReason::Error, |reason| *reason);
    out.send(&Reply::Exiting { reason });
    result.map(drop)
}

/// The threads writing replies, kept so none is lost when the worker exits.
#[derive(Clone, Default)]
struct Replies(Arc<Mutex<Vec<thread::JoinHandle<()>>>>);

impl Replies {
    fn spawn(&self, write: impl FnOnce() + Send + 'static) {
        let mut threads = self.0.lock().unwrap_or_else(|e| e.into_inner());
        threads.retain(|t| !t.is_finished());
        threads.push(thread::spawn(write));
    }

    fn finish(&self) {
        let threads = std::mem::take(&mut *self.0.lock().unwrap_or_else(|e| e.into_inner()));
        for t in threads {
            let _ = t.join();
        }
    }
}

/// Hands a request to the stream loop and writes its reply when it comes back.
fn dispatch(request: Request, commands: &mpsc::Sender<Command>, out: &Out, replies: &Replies) {
    let id = request.id();
    let out = out.clone();
    match request {
        Request::Offer { offer, codec, .. } => {
            let (reply, answer) = mpsc::channel();
            if commands.send(Command::Offer { offer, codec, reply }).is_err() {
                return;
            }
            replies.spawn(move || match answer.recv() {
                Ok(Ok(answer)) => out.send(&Reply::Answer { id, answer }),
                Ok(Err(message)) => out.send(&Reply::Error { id, message }),
                Err(_) => {}
            });
        }
        request => {
            let (reply, done) = mpsc::channel();
            if commands.send(Command::Control { request, reply }).is_err() {
                return;
            }
            replies.spawn(move || match done.recv() {
                Ok(Ok(())) => out.send(&Reply::Done { id }),
                Ok(Err(message)) => out.send(&Reply::Error { id, message }),
                Err(_) => {}
            });
        }
    }
}
