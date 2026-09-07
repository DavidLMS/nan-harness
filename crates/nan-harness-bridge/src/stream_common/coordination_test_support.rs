use crate::upstream::UpstreamResponse;
use nan_harness_coordinator::{AttemptOutcome, CoordinatorClient, EndpointKind};
use nan_harness_core::SecretValue;
use serde_json::{Value, json};
use std::time::Duration;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::{TcpListener, TcpStream};

const CHILD_TEST: &str = "NAN_TEST_SSE_COORDINATION";
const TIMEOUT: Duration = Duration::from_secs(10);

/// Only the child can point the real coordinator client at synthetic state.
pub(crate) async fn in_isolated_child(test_name: &str) -> bool {
    if std::env::var(CHILD_TEST).as_deref() == Ok(test_name) {
        return true;
    }
    let directory = tempfile::tempdir().expect("isolated coordination state");
    let mut command = tokio::process::Command::new(std::env::current_exe().expect("test binary"));
    command
        .args(["--exact", test_name, "--nocapture"])
        .env_clear()
        .env(CHILD_TEST, test_name)
        .env("NAN_HARNESS_CONFIG_DIR", directory.path())
        .env("NAN_HARNESS_INTERNAL_MANAGED_PROCESS", "1")
        .kill_on_drop(true);
    if let Some(profile) = std::env::var_os("LLVM_PROFILE_FILE") {
        command.env("LLVM_PROFILE_FILE", profile);
    }
    let mut child = command.spawn().expect("isolated coordinator test");
    let result = tokio::time::timeout(TIMEOUT, child.wait()).await;
    if result.is_err() {
        child.kill().await.expect("terminate timed-out child");
        child.wait().await.expect("reap timed-out child");
    }
    assert!(
        result
            .expect("child deadline")
            .expect("child status")
            .success()
    );
    false
}

pub(crate) async fn response(
    wire: String,
) -> (UpstreamResponse, tokio::task::JoinHandle<AttemptOutcome>) {
    let key = SecretValue::new("synthetic-provider").expect("test key");
    let client = CoordinatorClient::try_new("http://127.0.0.1", &key, "framing-test")
        .expect("client configuration")
        .expect("managed child must coordinate");
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("synthetic coordinator");
    let receipt = json!({
        "protocol_version": 2,
        "port": listener.local_addr().expect("listener address").port(),
        "token": "synthetic-token",
        "generation": "framing-test",
        "pid": std::process::id()
    });
    let directory = nan_harness_coordinator::config_directory().expect("test state directory");
    std::fs::write(
        directory.join("coordinator/v1/receipt.json"),
        receipt.to_string(),
    )
    .expect("synthetic receipt in isolated private directory");
    let observed = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("coordinator connection");
        let acquire = read_message(&mut stream).await;
        assert_eq!(acquire["type"], "acquire");
        write_message(
            &mut stream,
            json!({"type": "granted", "lease_id": 1, "queued_ms": 0}),
        )
        .await;
        let message = read_message(&mut stream).await;
        assert_eq!(message["type"], "observe");
        let outcome = serde_json::from_value(message["outcome"].clone()).expect("typed outcome");
        write_message(&mut stream, json!({"type": "complete"})).await;
        outcome
    });
    let lease = client
        .acquire(EndpointKind::Inference, Some("qwen3.6"), TIMEOUT)
        .await
        .expect("acquire real request lease")
        .expect("coordination enabled");
    // A single body chunk preserves the raw-DONE-before-framing regression.
    let response = reqwest::Response::from(
        axum::http::Response::builder()
            .header("content-type", "text/event-stream")
            .body(reqwest::Body::from(wire))
            .expect("provider response"),
    );
    (UpstreamResponse::new(response, Some(lease), None), observed)
}

async fn read_message(stream: &mut TcpStream) -> Value {
    let length = stream.read_u32().await.expect("frame length");
    assert!(length < 64 * 1024);
    let mut bytes = vec![0; length as usize];
    stream.read_exact(&mut bytes).await.expect("frame bytes");
    serde_json::from_slice(&bytes).expect("frame JSON")
}

async fn write_message(stream: &mut TcpStream, message: Value) {
    let bytes = serde_json::to_vec(&message).expect("frame JSON");
    stream
        .write_u32(u32::try_from(bytes.len()).expect("frame length fits"))
        .await
        .expect("write length");
    stream.write_all(&bytes).await.expect("write frame");
}
