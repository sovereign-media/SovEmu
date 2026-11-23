//! The admin's connection test: a `sovemu-worker connection-test` on a port from the same pool
//! as the game sessions, answered as a game is, so a browser can check it reaches the streams.
//! The worker echoes the browser's data channel and leaves on its own (the browser disconnects,
//! or its time is up); its port goes back to the pool when it does.

use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use sovemu_protocol::SessionDescription;
use sovemu_protocol::ipc::{Reply, Request};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::Error;
use crate::workers::Workers;

/// How long a test may last, connected or not: long enough for a browser on a slow network to
/// connect through a relay and time a few round trips.
pub const TEST_SECONDS: u64 = 30;

/// How long the worker gets to answer the offer.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(10);

/// After its own time is up, how long the worker gets to go before it is killed.
const EXIT_GRACE: Duration = Duration::from_secs(5);

impl Workers {
    /// Answers a browser's offer with a connection-test worker on a free game port. The worker
    /// echoes the browser's data channel for up to [`TEST_SECONDS`].
    pub async fn test_connection(
        self: &Arc<Self>,
        offer: SessionDescription,
    ) -> Result<SessionDescription, Error> {
        let port = self.take_port()?;
        let mut command = self.worker_command("connection-test", port);
        command
            .arg("--timeout")
            .arg(TEST_SECONDS.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // Its few log lines go straight to the server's own log.
            .stderr(Stdio::inherit())
            .kill_on_drop(true);
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(e) => {
                self.release_port(port);
                return Err(Error::Spawn(self.worker_bin(), e));
            }
        };
        let mut stdin = child.stdin.take().expect("piped");
        let stdout = child.stdout.take().expect("piped");

        let answered = tokio::time::timeout(ANSWER_TIMEOUT, async {
            let mut line = serde_json::to_string(&Request::Offer { id: 1, offer, codec: None })?;
            line.push('\n');
            // A worker that has already gone (it could not start) has closed its stdin: that is
            // it exiting, not a fault of the pipe's ("Broken pipe" would tell the admin nothing).
            stdin.write_all(line.as_bytes()).await.map_err(|_| Error::Exited)?;
            let mut reply = String::new();
            BufReader::new(stdout).read_line(&mut reply).await?;
            match serde_json::from_str::<Reply>(&reply) {
                Ok(Reply::Answer { answer, .. }) => Ok(answer),
                Ok(Reply::Error { message, .. }) => Err(Error::Worker(message)),
                _ if reply.is_empty() => Err(Error::Exited),
                _ => Err(Error::Unexpected(reply.trim().to_owned())),
            }
        })
        .await
        .unwrap_or(Err(Error::Timeout("the connection test did not answer")));

        // The worker owns the port until it exits; it is killed if it overstays. Its stdin is
        // held open meanwhile: closing it tells the worker the server has gone.
        let workers = self.clone();
        tokio::spawn(async move {
            let limit = Duration::from_secs(TEST_SECONDS) + EXIT_GRACE;
            if tokio::time::timeout(limit, child.wait()).await.is_err() {
                tracing::warn!(udp.port = port, "connection test overstayed; killing it");
                let _ = child.kill().await;
            }
            drop(stdin);
            workers.release_port(port);
        });
        answered
    }
}
