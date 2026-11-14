//! `sovemu-worker session`: one game session driven by `sovemu-server`.
//!
//! The server talks to the worker in JSON lines ([`sovemu_protocol::ipc`]): requests on stdin,
//! replies and events on stdout. Logs go to stderr, so stdout carries nothing but protocol.
//! The worker exits when stdin closes (the server went away), on `shutdown`, or after
//! `--idle-timeout` seconds with no client connected.

use std::io::{BufRead, Write};
use std::net::UdpSocket;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use sovemu_protocol::ipc::{Reply, Request};

use crate::SessionArgs;
use crate::stream::{self, Command, Session, SessionConfig, StreamEvent};

/// Writes one reply as a JSON line. Shared by the stream thread and the reply threads.
#[derive(Clone)]
struct Out(Arc<Mutex<std::io::Stdout>>);

impl Out {
    fn send(&self, reply: &Reply) {
        let line = serde_json::to_string(reply).expect("replies serialise");
        let mut out = self.0.lock().unwrap_or_else(|e| e.into_inner());
        // If stdout is gone the server is gone; stdin closing will stop us shortly.
        let _ = writeln!(out, "{line}").and_then(|_| out.flush());
    }
}

pub fn run(args: SessionArgs) -> Result<()> {
    str0m::crypto::from_feature_flags().install_process_default();
    let session = Session::open(&SessionConfig {
        core: args.core.clone(),
        rom: args.rom.clone(),
        system_dir: args.system_dir.clone(),
        save_dir: args.save_dir.clone(),
        variables: args.variables.clone(),
        video_kbps: args.video_kbps,
        audio_kbps: args.audio_kbps,
        rewind_seconds: args.rewind_seconds,
        rewind_max_mb: args.rewind_max_mb,
        fast_forward: args.fast_forward,
        keyframe_interval: args.keyframe_interval,
        gpu: args.gpu.0.clone(),
        state_dir: args.state_dir.clone(),
    })?;
    let (core, av) = session.describe();

    let socket = UdpSocket::bind(args.udp).with_context(|| format!("binding UDP {}", args.udp))?;
    let udp_addr = socket.local_addr()?;
    let advertised = stream::advertised_addr(args.advertise, udp_addr);
    tracing::info!("session ready: WebRTC on UDP {udp_addr}, advertising {advertised}");

    let out = Out(Arc::new(Mutex::new(std::io::stdout())));
    out.send(&Reply::Ready {
        udp_port: udp_addr.port(),
        core,
        width: av.base_width,
        height: av.base_height,
        fps: av.fps,
    });

    let (commands_tx, commands_rx) = mpsc::channel();
    let reader_out = out.clone();
    thread::Builder::new().name("stdin".into()).spawn(move || {
        for line in std::io::stdin().lock().lines() {
            let Ok(line) = line else { break };
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<Request>(&line) {
                Ok(request) => dispatch(request, &commands_tx, &reader_out),
                Err(e) => tracing::warn!("ignoring bad request {line:?}: {e}"),
            }
        }
        // Dropping the sender tells the stream loop to stop.
    })?;

    let event_out = out.clone();
    let idle = (args.idle_timeout > 0).then(|| Duration::from_secs(args.idle_timeout));
    stream::run(
        session,
        socket,
        advertised,
        commands_rx,
        &mut |event| {
            event_out.send(&match event {
                StreamEvent::ClientConnected => Reply::ClientConnected,
                StreamEvent::ClientDisconnected => Reply::ClientDisconnected,
            })
        },
        idle,
    )
}

/// Hands a request to the stream loop and writes its reply when it comes back.
fn dispatch(request: Request, commands: &mpsc::Sender<Command>, out: &Out) {
    let id = request.id();
    let out = out.clone();
    match request {
        Request::Offer { offer, .. } => {
            let (reply, answer) = mpsc::channel();
            if commands.send(Command::Offer { offer, reply }).is_err() {
                return;
            }
            thread::spawn(move || match answer.recv() {
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
            thread::spawn(move || match done.recv() {
                Ok(Ok(())) => out.send(&Reply::Done { id }),
                Ok(Err(message)) => out.send(&Reply::Error { id, message }),
                Err(_) => {}
            });
        }
    }
}
