//! `sovemu-worker serve`: run a core and stream it to one WebRTC client, standalone.
//!
//! For development and tests: the worker also serves the web client (`--web-dir`) and takes
//! offers on `POST /api/offer` itself. In production `sovemu-server` serves those and drives
//! workers through `sovemu-worker session` instead; both use the same streaming loop
//! ([`crate::stream`]).

use std::net::{SocketAddr, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use sovemu_protocol::{SdpKind, SessionDescription};

use crate::ServeArgs;
use crate::stream::{self, Command, Session, SessionConfig};
use crate::web;

/// How long the HTTP thread waits for the session loop to answer an offer.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) fn run(args: ServeArgs) -> Result<()> {
    str0m::crypto::from_feature_flags().install_process_default();
    let session = Session::open(
        &SessionConfig {
            core: args.core.clone(),
            rom: args.rom.clone(),
            system_dir: args.system_dir.clone(),
            bios_path: args.bios_path.clone(),
            save_dir: args.save_dir.clone(),
            variables: args.variables.clone(),
            video_kbps: args.video_kbps,
            audio_kbps: args.audio_kbps,
            rewind_seconds: args.rewind_seconds,
            rewind_max_mb: args.rewind_max_mb,
            fast_forward: args.fast_forward,
            run_ahead: args.run_ahead,
            keyframe_interval: args.keyframe_interval,
            gpu: args.gpu.0.clone(),
            state_dir: Some(args.save_dir.join("states")),
        },
        &crate::loading::ignore,
    )?;

    let socket = UdpSocket::bind(args.udp).with_context(|| format!("binding UDP {}", args.udp))?;
    let udp_addr = socket.local_addr()?;
    let advertised = stream::advertised_addr(args.advertise, udp_addr);
    tracing::info!("WebRTC on UDP {udp_addr}, advertising {advertised}");

    let (commands_tx, commands_rx) = mpsc::channel();
    let http_addr = spawn_http(args.http, args.web_dir.clone(), commands_tx)?;
    // Machine-readable lines for whoever started us (tests).
    println!("http={http_addr}");
    println!("udp={udp_addr}");
    tracing::info!("open http://{http_addr}/ in a browser");

    stream::run(session, socket, advertised, commands_rx, &mut |_| {}, None)
}

/// Serves the web client and `POST /api/offer` on a background thread.
fn spawn_http(
    addr: SocketAddr,
    web_dir: Option<PathBuf>,
    offers: Sender<Command>,
) -> Result<SocketAddr> {
    let server = tiny_http::Server::http(addr).map_err(|e| anyhow!("binding HTTP {addr}: {e}"))?;
    let bound = server
        .server_addr()
        .to_ip()
        .ok_or_else(|| anyhow!("HTTP server is not on an IP socket"))?;
    thread::Builder::new().name("http".into()).spawn(move || {
        for mut request in server.incoming_requests() {
            let method = request.method().clone();
            let url = request.url().to_owned();
            let mut body = String::new();
            let body =
                std::io::Read::read_to_string(request.as_reader(), &mut body).ok().map(|_| body);
            if let Err(e) = handle_http(&method, &url, body, request, web_dir.as_deref(), &offers) {
                tracing::debug!("HTTP response failed: {e}");
            }
        }
    })?;
    Ok(bound)
}

fn handle_http(
    method: &tiny_http::Method,
    url: &str,
    body: Option<String>,
    request: tiny_http::Request,
    web_dir: Option<&Path>,
    offers: &Sender<Command>,
) -> std::io::Result<()> {
    use tiny_http::{Header, Method, Response};
    let json = Header::from_bytes("Content-Type", "application/json").expect("valid header");
    let html =
        Header::from_bytes("Content-Type", "text/html; charset=utf-8").expect("valid header");
    let path = url.split('?').next().unwrap_or("/");

    match (method, path) {
        (Method::Post, "/api/offer") => {
            let offer = body
                .and_then(|b| serde_json::from_str::<SessionDescription>(&b).ok())
                .filter(|d| d.kind == SdpKind::Offer);
            let Some(offer) = offer else {
                return request
                    .respond(Response::from_string("expected an SDP offer").with_status_code(400));
            };
            let (reply, answer) = mpsc::channel();
            if offers.send(Command::Offer { offer, reply }).is_err() {
                return request
                    .respond(Response::from_string("session ended").with_status_code(503));
            }
            match answer.recv_timeout(ANSWER_TIMEOUT) {
                Ok(Ok(answer)) => request.respond(
                    Response::from_string(serde_json::to_string(&answer).expect("serialisable"))
                        .with_header(json),
                ),
                Ok(Err(e)) => request.respond(Response::from_string(e).with_status_code(400)),
                Err(_) => request.respond(Response::from_string("no answer").with_status_code(503)),
            }
        }
        (Method::Get | Method::Head, path) if !path.starts_with("/api/") => {
            match web_dir.and_then(|dir| web::lookup(dir, path)) {
                Some(file) => {
                    let content_type = Header::from_bytes("Content-Type", file.content_type)
                        .expect("valid header");
                    let cache = Header::from_bytes("Cache-Control", file.cache_control)
                        .expect("valid header");
                    request.respond(
                        Response::from_data(file.body).with_header(content_type).with_header(cache),
                    )
                }
                None if web_dir.is_none() && path == "/" => {
                    request.respond(Response::from_string(web::NOT_INSTALLED).with_header(html))
                }
                None => request.respond(Response::from_string("not found").with_status_code(404)),
            }
        }
        _ => request.respond(Response::from_string("not found").with_status_code(404)),
    }
}
