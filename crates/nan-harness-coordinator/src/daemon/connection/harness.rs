//! Loopback helpers shared by the connection lifecycle tests.
//!
//! Every wait is bounded by an outer deadline so a lifecycle regression fails
//! instead of hanging the suite, and every "this must not happen" observation is
//! paired with a production delay far larger than the observation window, so no
//! assertion depends on tight wall-clock timing.

use super::super::lifecycle::serve;
use crate::CoordinatorError;
use crate::protocol::{
    AttemptOutcome, AttemptPhase, ClientMessage, EndpointKind, PROTOCOL_VERSION, RequestLane,
    RequestPriority, ServerMessage, read_frame, write_frame,
};
use std::io::ErrorKind;
use std::net::SocketAddr;
use std::time::Duration;
use tempfile::TempDir;
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

/// Upper bound for a step that must happen.
pub(super) const STEP_DEADLINE: Duration = Duration::from_secs(5);

/// Observation window for a step that must not happen.
pub(super) const QUIET_WINDOW: Duration = Duration::from_millis(150);

/// Retry-after that outlasts [`QUIET_WINDOW`] by two orders of magnitude, so a
/// grant observed inside that window cannot be a scheduling artefact.
pub(super) const LONG_RETRY_AFTER_MS: u64 = 60_000;

/// Header timing well inside the healthy threshold the scheduler applies.
pub(super) const HEALTHY_HEADERS_MS: u64 = 10;

pub(super) const TOKEN: &str = "coordinator-token";

/// A coordinator served on loopback with its own private capacity cache.
pub(super) struct TestDaemon {
    address: SocketAddr,
    task: JoinHandle<Result<(), CoordinatorError>>,
    _directory: TempDir,
}

impl TestDaemon {
    pub(super) async fn start() -> Self {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("test coordinator should bind");
        let address = listener
            .local_addr()
            .expect("listener address should exist");
        let task = tokio::spawn(serve(
            listener,
            TOKEN.to_owned(),
            directory.path().join("capacity.json"),
        ));
        Self {
            address,
            task,
            _directory: directory,
        }
    }

    pub(super) fn address(&self) -> SocketAddr {
        self.address
    }

    /// Connects and requests foreground inference capacity for `scope`.
    pub(super) async fn request(&self, scope: &str) -> TcpStream {
        self.request_as(
            scope,
            RequestLane::Inference,
            RequestPriority::Foreground,
            TOKEN,
        )
        .await
    }

    pub(super) async fn request_as(
        &self,
        scope: &str,
        lane: RequestLane,
        priority: RequestPriority,
        token: &str,
    ) -> TcpStream {
        let mut stream = connect(self.address).await;
        send(
            &mut stream,
            &acquire(scope, lane, priority, PROTOCOL_VERSION, token),
        )
        .await;
        stream
    }

    /// Connects and requests foreground control capacity for `scope`.
    pub(super) async fn control_lease(&self, scope: &str) -> TcpStream {
        self.request_as(
            scope,
            RequestLane::Control,
            RequestPriority::Foreground,
            TOKEN,
        )
        .await
    }

    /// Connects, requests capacity and waits for the grant.
    pub(super) async fn lease(&self, scope: &str) -> (TcpStream, u64) {
        let mut stream = self.request(scope).await;
        let lease_id = expect_granted(&mut stream).await;
        (stream, lease_id)
    }
}

impl Drop for TestDaemon {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub(super) async fn connect(address: SocketAddr) -> TcpStream {
    TcpStream::connect(address)
        .await
        .expect("client should connect")
}

pub(super) fn acquire(
    scope: &str,
    lane: RequestLane,
    priority: RequestPriority,
    protocol_version: u8,
    token: &str,
) -> ClientMessage {
    ClientMessage::Acquire {
        protocol_version,
        token: token.to_owned(),
        scope: scope.to_owned(),
        launch_id: "codex".to_owned(),
        endpoint: EndpointKind::Inference,
        model: Some("model".to_owned()),
        lane,
        priority,
    }
}

pub(super) fn progress(lease_id: u64, elapsed_ms: u64) -> ClientMessage {
    ClientMessage::Progress {
        lease_id,
        phase: AttemptPhase::HeadersReceived,
        elapsed_ms,
    }
}

pub(super) fn observe(
    lease_id: u64,
    outcome: AttemptOutcome,
    retry_after_ms: Option<u64>,
) -> ClientMessage {
    ClientMessage::Observe {
        lease_id,
        outcome,
        retry_after_ms,
    }
}

pub(super) async fn send(stream: &mut TcpStream, message: &ClientMessage) {
    write_frame(stream, message)
        .await
        .expect("client frame should write");
}

pub(super) async fn expect_message(stream: &mut TcpStream) -> ServerMessage {
    tokio::time::timeout(STEP_DEADLINE, read_frame::<ServerMessage>(stream))
        .await
        .expect("the coordinator should answer within the step deadline")
        .expect("a server frame should read")
}

pub(super) async fn expect_granted(stream: &mut TcpStream) -> u64 {
    match expect_message(stream).await {
        ServerMessage::Granted { lease_id, .. } => {
            assert!(lease_id > 0, "a granted lease needs an identifier");
            lease_id
        }
        other => panic!("expected a grant, observed {other:?}"),
    }
}

/// Asserts that the coordinator neither answers nor closes within the window.
pub(super) async fn expect_quiet(stream: &mut TcpStream, reason: &str) {
    let observed = tokio::time::timeout(QUIET_WINDOW, read_frame::<ServerMessage>(stream)).await;
    assert!(observed.is_err(), "{reason}");
}

/// Asserts that the coordinator ends the connection without any directive.
pub(super) async fn expect_closed_without_directive(stream: &mut TcpStream, reason: &str) {
    let error = tokio::time::timeout(STEP_DEADLINE, read_frame::<ServerMessage>(stream))
        .await
        .expect("the coordinator should not leave the connection open")
        .expect_err(reason);
    assert!(
        matches!(
            error.kind(),
            ErrorKind::UnexpectedEof | ErrorKind::ConnectionReset
        ),
        "{reason}: unexpected transport error {error:?}"
    );
}
