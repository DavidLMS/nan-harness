//! Active-connection accounting. The connection guard tracks how many handlers
//! are alive so the daemon knows when it is idle; it is not the scheduler's
//! capacity permit, and it must be released exactly once on every exit path.

use super::handle_connection;
use super::harness::{
    STEP_DEADLINE, TOKEN, acquire, expect_closed_without_directive, expect_granted, expect_message,
    expect_quiet, observe, send,
};
use crate::CaptureSink;
use crate::protocol::{
    AttemptOutcome, PROTOCOL_VERSION, RequestLane, RequestPriority, ServerMessage,
};
use crate::scheduler::Scheduler;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Instant;
use tempfile::TempDir;
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

/// Drives `handle_connection` the way the daemon's accept loop does, except
/// that the active-connection counter belongs to the test and can be read.
struct CountedDaemon {
    listener: TcpListener,
    address: SocketAddr,
    scheduler: Scheduler,
    connections: Arc<AtomicUsize>,
    last_activity: Arc<AtomicU64>,
    capture: CaptureSink,
    started: Instant,
    _directory: TempDir,
}

impl CountedDaemon {
    async fn start() -> Self {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("test coordinator should bind");
        let address = listener
            .local_addr()
            .expect("listener address should exist");
        Self {
            listener,
            address,
            scheduler: Scheduler::start(directory.path().join("capacity.json")),
            connections: Arc::new(AtomicUsize::new(0)),
            last_activity: Arc::new(AtomicU64::new(0)),
            capture: CaptureSink::new("coordinator_test"),
            started: Instant::now(),
            _directory: directory,
        }
    }

    async fn connect(&self) -> (TcpStream, JoinHandle<()>) {
        let client = TcpStream::connect(self.address)
            .await
            .expect("client should connect");
        let (server, _) = self
            .listener
            .accept()
            .await
            .expect("the coordinator should accept");
        // The accept loop counts a connection before handing it to the handler.
        self.connections.fetch_add(1, Ordering::Relaxed);
        let task = tokio::spawn(handle_connection(
            server,
            TOKEN.to_owned(),
            self.scheduler.clone(),
            Arc::clone(&self.connections),
            Arc::clone(&self.last_activity),
            self.capture.clone(),
            self.started,
        ));
        (client, task)
    }

    async fn lease(&self, scope: &str) -> (TcpStream, JoinHandle<()>, u64) {
        let (mut client, task) = self.connect().await;
        send(
            &mut client,
            &acquire(
                scope,
                RequestLane::Inference,
                RequestPriority::Foreground,
                PROTOCOL_VERSION,
                TOKEN,
            ),
        )
        .await;
        let lease_id = expect_granted(&mut client).await;
        (client, task, lease_id)
    }

    fn active(&self) -> usize {
        self.connections.load(Ordering::Relaxed)
    }
}

async fn finish(task: JoinHandle<()>) {
    tokio::time::timeout(STEP_DEADLINE, task)
        .await
        .expect("the handler should finish within the step deadline")
        .expect("the handler should not panic");
}

#[tokio::test]
async fn every_exit_path_releases_its_active_connection_exactly_once() {
    let daemon = CountedDaemon::start().await;
    let (mut completing, completing_task, completing_lease) = daemon.lease("completing").await;
    let (mut abandoning, abandoning_task, abandoning_lease) = daemon.lease("abandoning").await;
    let (disconnecting, disconnecting_task, _) = daemon.lease("disconnecting").await;
    assert_eq!(
        daemon.active(),
        3,
        "three handlers are alive, one per accepted connection"
    );

    send(
        &mut completing,
        &observe(completing_lease, AttemptOutcome::Success, None),
    )
    .await;
    assert!(matches!(
        expect_message(&mut completing).await,
        ServerMessage::Complete
    ));

    send(
        &mut abandoning,
        &observe(
            abandoning_lease.wrapping_add(1),
            AttemptOutcome::Success,
            None,
        ),
    )
    .await;
    expect_closed_without_directive(&mut abandoning, "the abandoned attempt ends the connection")
        .await;

    drop(disconnecting);

    finish(completing_task).await;
    finish(abandoning_task).await;
    finish(disconnecting_task).await;
    // A missed release would leave a positive count; a second release would
    // wrap the unsigned counter far above zero.
    assert_eq!(daemon.active(), 0);
}

#[tokio::test]
async fn a_rejected_connection_releases_its_active_connection() {
    let daemon = CountedDaemon::start().await;
    let (mut client, task) = daemon.connect().await;
    send(
        &mut client,
        &acquire(
            "rejected",
            RequestLane::Inference,
            RequestPriority::Foreground,
            PROTOCOL_VERSION,
            "wrong-token",
        ),
    )
    .await;
    assert!(matches!(
        expect_message(&mut client).await,
        ServerMessage::Rejected { .. }
    ));

    finish(task).await;
    assert_eq!(daemon.active(), 0);
}

#[tokio::test]
async fn a_connection_that_sends_nothing_releases_its_active_connection() {
    let daemon = CountedDaemon::start().await;
    let (client, task) = daemon.connect().await;
    assert_eq!(daemon.active(), 1);

    drop(client);

    finish(task).await;
    assert_eq!(daemon.active(), 0);
}

#[tokio::test]
async fn a_client_that_disconnects_while_queued_releases_its_active_connection() {
    let scope = "queued";
    let daemon = CountedDaemon::start().await;
    let (_first, _first_task, _) = daemon.lease(scope).await;
    let (_second, _second_task, _) = daemon.lease(scope).await;

    let (mut queued, queued_task) = daemon.connect().await;
    send(
        &mut queued,
        &acquire(
            scope,
            RequestLane::Inference,
            RequestPriority::Foreground,
            PROTOCOL_VERSION,
            TOKEN,
        ),
    )
    .await;
    expect_quiet(
        &mut queued,
        "both permits are held, so the third client waits",
    )
    .await;
    assert_eq!(daemon.active(), 3);

    drop(queued);

    finish(queued_task).await;
    assert_eq!(
        daemon.active(),
        2,
        "a client abandoning the queue releases only its own connection"
    );
}
