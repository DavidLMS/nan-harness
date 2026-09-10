use super::{CoordinatorClient, canonical_origin, fingerprint, load_or_create_salt};
use crate::protocol::{ClientMessage, PROTOCOL_VERSION, Receipt, read_frame};
use crate::{CoordinatorError, EndpointKind};
use nan_harness_private_fs::open_private_new;
use std::io::Write as _;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::net::TcpListener;

#[test]
fn scope_fingerprint_is_stable_and_origin_scoped() {
    let salt = [7_u8; 32];
    let first = fingerprint(&salt, "https://api.example.com", "secret");
    assert_eq!(
        first,
        fingerprint(&salt, "https://api.example.com", "secret")
    );
    assert_ne!(
        first,
        fingerprint(&salt, "https://other.example.com", "secret")
    );
    assert_eq!(
        canonical_origin("https://api.example.com/v1"),
        Some("https://api.example.com".to_owned())
    );
}

#[test]
fn scope_salt_waits_for_a_concurrent_creator_to_finish_writing() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let path = temporary.path().join("scope.salt");
    let mut file = open_private_new(&path).expect("salt placeholder");
    let expected = vec![7_u8; 32];
    let published = expected.clone();
    let writer = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(30));
        file.write_all(&published).expect("salt should be written");
        file.sync_all().expect("salt should be durable");
    });

    let loaded = load_or_create_salt(temporary.path()).expect("salt should become readable");

    writer.join().expect("writer should finish");
    assert_eq!(loaded, expected);
}

#[tokio::test]
async fn capacity_wait_timeout_is_an_error_instead_of_uncoordinated_fallback() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener should bind");
    let receipt = Receipt {
        protocol_version: PROTOCOL_VERSION,
        port: listener.local_addr().expect("listener address").port(),
        token: "test-token".to_owned(),
        generation: "test-generation".to_owned(),
        pid: std::process::id(),
    };
    std::fs::write(
        temporary.path().join("receipt.json"),
        serde_json::to_vec(&receipt).expect("receipt should encode"),
    )
    .expect("receipt should be written");
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("client should connect");
        let _: ClientMessage = read_frame(&mut stream)
            .await
            .expect("capacity request should arrive");
        tokio::time::sleep(Duration::from_secs(1)).await;
    });
    let client = CoordinatorClient {
        directory: temporary.path().to_owned(),
        scope: "scope".to_owned(),
        launch_id: Arc::from("launch"),
        session_max_tokens: None,
        retry_probe_at: Arc::new(Mutex::new(Instant::now())),
    };

    let result = client
        .acquire(
            EndpointKind::Inference,
            Some("model"),
            Duration::from_millis(20),
        )
        .await;
    assert!(matches!(result, Err(CoordinatorError::QueueTimeout)));
    server.abort();
}
