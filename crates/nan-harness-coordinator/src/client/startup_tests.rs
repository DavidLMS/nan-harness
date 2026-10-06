use super::child_process::{ChildScenario, scenario_completed};
use super::{
    CoordinatorClient, FAILED_PROBE_COOLDOWN, PROTOCOL_VERSION, Receipt, SALT_PUBLICATION_BUDGET,
    STARTUP_BUDGET, connect_from_receipt, load_or_create_salt,
};
use crate::CoordinatorError;
use nan_harness_private_fs::open_private_new;
use std::io::{ErrorKind, Write as _};
use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, Mutex};
use std::time::{Duration, Instant};
use tokio::net::{TcpListener, TcpSocket};

/// Protocol version of a daemon started by an older nan-harness installation.
const LEGACY_PROTOCOL_VERSION: u8 = 1;
const TEST_TOKEN: &str = "startup-test-token";
const TEST_GENERATION: &str = "startup-test-generation";
const TEST_PID: u32 = 4_242;
const SALT_BYTES: usize = 32;
/// Exact path of the child scenario that waits for a salt that is never published.
const INVALID_SALT_SCENARIO: &str = "client::startup_tests::an_invalid_salt_is_reported_in_a_child";
const SALT_DIRECTORY_ENVIRONMENT: &str = "NAN_HARNESS_TEST_SALT_DIRECTORY";
/// Comfortably above the one-second publication budget, and far below the mutation
/// harness cap, so a wait that never expires fails as an ordinary test outcome.
const CHILD_SCENARIO_BUDGET: Duration = Duration::from_secs(30);

#[tokio::test]
async fn an_absent_receipt_is_not_a_coordination_failure() {
    let temporary = tempfile::tempdir().expect("temporary directory");

    let connection = connect_from_receipt(temporary.path())
        .await
        .expect("an absent receipt should not be an error");

    assert!(connection.is_none());
}

#[tokio::test]
async fn a_malformed_receipt_is_ignored_instead_of_reported() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    publish_bytes(&temporary.path().join("receipt.json"), b"{\"port\":");

    let connection = connect_from_receipt(temporary.path())
        .await
        .expect("a malformed receipt should not be an error");

    assert!(connection.is_none());
}

#[tokio::test]
async fn a_live_matching_receipt_is_reused_over_loopback() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener should bind");
    let port = listener.local_addr().expect("listener address").port();
    publish_receipt(temporary.path(), &receipt_for(port, PROTOCOL_VERSION));

    let (stream, receipt) = connect_from_receipt(temporary.path())
        .await
        .expect("a live daemon should be reachable")
        .expect("a live daemon should be reused");

    assert_eq!(receipt.protocol_version, PROTOCOL_VERSION);
    assert_eq!(receipt.port, port);
    assert_eq!(receipt.token, TEST_TOKEN);
    assert_eq!(receipt.generation, TEST_GENERATION);
    assert_eq!(receipt.pid, TEST_PID);
    let peer = stream.peer_addr().expect("peer address");
    assert_eq!(peer.ip(), IpAddr::V4(Ipv4Addr::LOCALHOST));
    assert_eq!(peer.port(), port);
}

#[tokio::test]
async fn a_matching_receipt_for_a_dead_endpoint_is_ignored() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let (_reservation, port) = reserved_dead_endpoint();
    publish_receipt(temporary.path(), &receipt_for(port, PROTOCOL_VERSION));

    let connection = connect_from_receipt(temporary.path())
        .await
        .expect("a stale receipt should not be an error");

    assert!(connection.is_none());
}

#[tokio::test]
async fn a_live_incompatible_daemon_is_reported_with_its_version() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener should bind");
    let port = listener.local_addr().expect("listener address").port();
    publish_receipt(
        temporary.path(),
        &receipt_for(port, LEGACY_PROTOCOL_VERSION),
    );

    let error = connect_from_receipt(temporary.path())
        .await
        .expect_err("a live incompatible daemon should be reported");

    assert!(matches!(
        &error,
        CoordinatorError::IncompatibleDaemon { detected } if *detected == LEGACY_PROTOCOL_VERSION
    ));
    assert_eq!(error.code(), "NH-COORD-007");
}

#[tokio::test]
async fn an_incompatible_receipt_for_a_dead_endpoint_is_ignored() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let (_reservation, port) = reserved_dead_endpoint();
    publish_receipt(
        temporary.path(),
        &receipt_for(port, LEGACY_PROTOCOL_VERSION),
    );

    let connection = connect_from_receipt(temporary.path())
        .await
        .expect("an exited incompatible daemon should not be reported");

    assert!(connection.is_none());
}

#[tokio::test]
async fn a_running_daemon_is_reused_without_starting_another() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener should bind");
    let port = listener.local_addr().expect("listener address").port();
    publish_receipt(temporary.path(), &receipt_for(port, PROTOCOL_VERSION));
    let client = client_for(temporary.path());
    let cooldown_before = cooldown_deadline(&client);
    let starts = Arc::new(AtomicUsize::new(0));

    let (_stream, receipt) = bounded_startup(client.connect_or_spawn(counting_start(&starts)))
        .await
        .expect("a running daemon should be reused");

    assert_eq!(receipt.token, TEST_TOKEN);
    assert_eq!(starts.load(Ordering::SeqCst), 0);
    assert_eq!(cooldown_deadline(&client), cooldown_before);
}

#[tokio::test]
async fn a_daemon_that_publishes_its_receipt_is_used_within_the_startup_budget() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener should bind");
    let port = listener.local_addr().expect("listener address").port();
    let client = client_for(temporary.path());
    let cooldown_before = cooldown_deadline(&client);
    let starts = Arc::new(AtomicUsize::new(0));
    let started_at = Instant::now();
    let publish = {
        let directory = temporary.path().to_owned();
        let starts = Arc::clone(&starts);
        move || {
            starts.fetch_add(1, Ordering::SeqCst);
            publish_receipt(&directory, &receipt_for(port, PROTOCOL_VERSION));
        }
    };

    let (_stream, receipt) = bounded_startup(client.connect_or_spawn(publish))
        .await
        .expect("a started daemon should be reached");

    assert_eq!(receipt.port, port);
    assert_eq!(starts.load(Ordering::SeqCst), 1);
    assert!(started_at.elapsed() < STARTUP_BUDGET);
    assert_eq!(
        cooldown_deadline(&client),
        cooldown_before,
        "a successful start must not arm the failed-probe cooldown"
    );
}

#[tokio::test]
async fn a_failed_start_arms_the_cooldown_and_refuses_the_next_attempt_without_starting() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let client = client_for(temporary.path());
    let starts = Arc::new(AtomicUsize::new(0));
    let attempted_at = Instant::now();

    let error = bounded_startup(client.connect_or_spawn(counting_start(&starts)))
        .await
        .expect_err("a daemon that never answers should fail");
    let failed_at = Instant::now();

    assert_eq!(error.code(), "NH-COORD-005");
    assert_eq!(starts.load(Ordering::SeqCst), 1);
    assert!(attempted_at.elapsed() >= STARTUP_BUDGET);
    let cooldown = cooldown_deadline(&client);
    assert!(cooldown > failed_at);
    assert!(cooldown <= failed_at + FAILED_PROBE_COOLDOWN);

    let refused_at = Instant::now();
    let refused = bounded_startup(client.connect_or_spawn(counting_start(&starts)))
        .await
        .expect_err("the cooldown should refuse a second attempt");

    assert_eq!(refused.code(), "NH-COORD-005");
    assert_eq!(starts.load(Ordering::SeqCst), 1);
    assert!(refused_at.elapsed() < STARTUP_BUDGET);
    assert_eq!(cooldown_deadline(&client), cooldown);
    assert!(!temporary.path().join("receipt.json").exists());
}

#[tokio::test]
async fn an_incompatible_daemon_is_reported_instead_of_arming_the_cooldown() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let (_reservation, port) = reserved_dead_endpoint();
    publish_receipt(
        temporary.path(),
        &receipt_for(port, LEGACY_PROTOCOL_VERSION),
    );
    let client = client_for(temporary.path());
    let cooldown_before = cooldown_deadline(&client);
    let starts = Arc::new(AtomicUsize::new(0));

    let error = bounded_startup(client.connect_or_spawn(counting_start(&starts)))
        .await
        .expect_err("an incompatible receipt should be reported");

    assert!(matches!(
        &error,
        CoordinatorError::IncompatibleDaemon { detected } if *detected == LEGACY_PROTOCOL_VERSION
    ));
    assert_eq!(starts.load(Ordering::SeqCst), 1);
    assert_eq!(
        cooldown_deadline(&client),
        cooldown_before,
        "an incompatible daemon is a distinct failure from a daemon that did not start"
    );
}

#[test]
fn a_missing_salt_is_created_privately_and_then_reused() {
    let temporary = tempfile::tempdir().expect("temporary directory");

    let created = load_or_create_salt(temporary.path()).expect("a salt should be created");
    let reused = load_or_create_salt(temporary.path()).expect("the salt should be reused");

    assert_eq!(created.len(), SALT_BYTES);
    assert_ne!(created, vec![0_u8; SALT_BYTES]);
    assert_eq!(reused, created);
    let path = temporary.path().join("scope.salt");
    assert_eq!(
        std::fs::read(&path).expect("the salt should be published"),
        created
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = std::fs::metadata(&path)
            .expect("salt metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "a scope salt must stay owner-only");
    }
}

#[test]
fn concurrent_creators_agree_on_the_single_published_salt() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let directory = temporary.path().to_owned();
    let barrier = Arc::new(Barrier::new(4));
    let creators: Vec<_> = (0..4)
        .map(|_| {
            let directory = directory.clone();
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                load_or_create_salt(&directory)
            })
        })
        .collect();

    let salts: Vec<Vec<u8>> = creators
        .into_iter()
        .map(|creator| {
            creator
                .join()
                .expect("creator should finish")
                .expect("every creator should observe a salt")
        })
        .collect();

    let published =
        std::fs::read(directory.join("scope.salt")).expect("a salt should be published");
    assert_eq!(published.len(), SALT_BYTES);
    for salt in &salts {
        assert_eq!(salt, &published);
    }
}

#[test]
fn an_invalid_salt_is_reported_after_the_publication_budget_without_being_replaced() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let fixture = temporary.path().join("state");
    std::fs::create_dir(&fixture).expect("state fixture should be created");
    let path = fixture.join("scope.salt");
    let truncated = vec![9_u8; SALT_BYTES - 1];
    publish_bytes(&path, &truncated);

    let marker = temporary.path().join("scenario.marker");

    ChildScenario::new(INVALID_SALT_SCENARIO, &marker)
        .with_env(SALT_DIRECTORY_ENVIRONMENT, &fixture)
        .run(CHILD_SCENARIO_BUDGET);

    assert_eq!(
        std::fs::read(&path).expect("the salt should remain"),
        truncated
    );
}

/// Waits for a salt that is never published; the parent above bounds this externally
/// because the publication wait sleeps synchronously and cannot be cancelled.
#[test]
#[ignore = "bounded child scenario of the salt publication wait"]
fn an_invalid_salt_is_reported_in_a_child() {
    let directory = PathBuf::from(
        std::env::var_os(SALT_DIRECTORY_ENVIRONMENT).expect("the parent should name the fixture"),
    );
    let path = directory.join("scope.salt");
    let existing = std::fs::read(&path).expect("the fixture salt should be readable");
    let waited_from = Instant::now();

    let error =
        load_or_create_salt(&directory).expect_err("an unpublished salt should be reported");

    assert_eq!(error.kind(), ErrorKind::InvalidData);
    assert!(waited_from.elapsed() >= SALT_PUBLICATION_BUDGET);
    assert_eq!(
        std::fs::read(&path).expect("the salt should remain"),
        existing
    );
    scenario_completed();
}

#[test]
fn a_missing_state_directory_fails_without_publishing_a_salt() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let missing = temporary.path().join("absent");
    let attempted_at = Instant::now();

    let error =
        load_or_create_salt(&missing).expect_err("a missing directory cannot publish a salt");

    assert_eq!(error.kind(), ErrorKind::NotFound);
    assert!(
        attempted_at.elapsed() < SALT_PUBLICATION_BUDGET,
        "a creation failure that no other writer can resolve must not poll for a publication"
    );
    assert!(!missing.exists());
}

#[cfg(unix)]
#[test]
fn a_salt_path_that_is_not_a_file_propagates_its_error_kind() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let path = temporary.path().join("scope.salt");
    std::fs::create_dir(&path).expect("directory fixture should be created");
    let attempted_at = Instant::now();

    let error = load_or_create_salt(temporary.path()).expect_err("a directory is not a salt");

    assert_eq!(error.kind(), ErrorKind::InvalidInput);
    assert!(
        attempted_at.elapsed() < SALT_PUBLICATION_BUDGET,
        "a non-retryable read failure must not poll for a publication"
    );
    assert!(path.is_dir(), "the existing path must be left untouched");
}

fn receipt_for(port: u16, protocol_version: u8) -> Receipt {
    Receipt {
        protocol_version,
        port,
        token: TEST_TOKEN.to_owned(),
        generation: TEST_GENERATION.to_owned(),
        pid: TEST_PID,
    }
}

fn publish_receipt(directory: &Path, receipt: &Receipt) {
    let encoded = serde_json::to_vec(receipt).expect("receipt fixture should encode");
    publish_bytes(&directory.join("receipt.json"), &encoded);
}

fn publish_bytes(path: &Path, bytes: &[u8]) {
    let mut file = open_private_new(path).expect("private fixture should be created");
    file.write_all(bytes).expect("fixture should be written");
    file.sync_all().expect("fixture should be durable");
}

fn reserved_dead_endpoint() -> (TcpSocket, u16) {
    // Keep the port bound without listening: probes are refused, and another
    // parallel test cannot turn this stale receipt into a live endpoint.
    let socket = TcpSocket::new_v4().expect("socket should be created");
    socket
        .bind("127.0.0.1:0".parse().expect("loopback address"))
        .expect("socket should bind");
    let port = socket.local_addr().expect("socket address").port();
    (socket, port)
}

#[tokio::test]
async fn a_dead_endpoint_reservation_prevents_parallel_listener_reuse() {
    let (_reservation, port) = reserved_dead_endpoint();
    assert!(
        TcpListener::bind((Ipv4Addr::LOCALHOST, port))
            .await
            .is_err()
    );
    assert!(
        tokio::net::TcpStream::connect((Ipv4Addr::LOCALHOST, port))
            .await
            .is_err()
    );
}

fn client_for(directory: &Path) -> CoordinatorClient {
    CoordinatorClient {
        directory: directory.to_owned(),
        scope: "startup-test-scope".to_owned(),
        launch_id: Arc::from("startup-test-launch"),
        session_max_tokens: None,
        retry_probe_at: Arc::new(Mutex::new(Instant::now())),
    }
}

fn cooldown_deadline(client: &CoordinatorClient) -> Instant {
    *client.retry_probe_at.lock().expect("cooldown state")
}

fn counting_start(starts: &Arc<AtomicUsize>) -> impl FnOnce() {
    let starts = Arc::clone(starts);
    move || {
        starts.fetch_add(1, Ordering::SeqCst);
    }
}

async fn bounded_startup<T>(future: impl Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(10), future)
        .await
        .expect("startup operation must finish within the outer test deadline")
}
