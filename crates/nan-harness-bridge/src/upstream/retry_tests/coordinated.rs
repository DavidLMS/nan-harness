use super::*;
use serde_json::json;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpStream;

const SCENARIO: &str = "NAN_TEST_RETRY_WAIT_COORDINATOR";
const TEST_NAME: &str =
    "upstream::retry_tests::coordinated::retry_coordinated_wait_preserves_hints_and_known_status";

#[tokio::test]
async fn retry_coordinated_wait_preserves_hints_and_known_status() {
    if std::env::var_os(SCENARIO).is_some() {
        exercise().await;
        return;
    }
    let directory = tempfile::tempdir().expect("isolated configuration");
    let mut command =
        tokio::process::Command::new(std::env::current_exe().expect("test executable"));
    command
        .args(["--exact", TEST_NAME, "--nocapture"])
        .env_clear()
        .env(SCENARIO, "1")
        .env("NAN_HARNESS_CONFIG_DIR", directory.path())
        .env("NAN_HARNESS_INTERNAL_MANAGED_PROCESS", "1")
        .kill_on_drop(true);
    if let Some(profile) = std::env::var_os("LLVM_PROFILE_FILE") {
        command.env("LLVM_PROFILE_FILE", profile);
    }
    let result = tokio::time::timeout(Duration::from_secs(20), command.status())
        .await
        .expect("isolated retry test deadline")
        .expect("child status");
    assert!(result.success());
}

async fn exercise() {
    for (status, hint, delay_ms, expected_hint_ms) in [
        (503, "46", 0, 46_000),
        (429, "18446744073709551615", 0, u64::MAX),
        (503, "0", 46_000, 0),
    ] {
        let (mut client, sends, provider_task) = provider(status, hint, "known failure").await;
        let (coordinator_task, observation) = coordinator(delay_ms).await;
        client.coordinator =
            CoordinatorClient::try_new(&client.chat_endpoint, &client.api_key, "synthetic-retry")
                .expect("coordinator setup");
        assert!(client.coordinator.is_some());
        let response =
            tokio::time::timeout(Duration::from_secs(2), client.send(&Value::Null, b"{}"))
                .await
                .expect("unaffordable pause must return promptly")
                .expect("known response");
        assert_eq!(response.status().as_u16(), status);
        assert!(matches!(response.read_final_error_body().await,
            FinalErrorBody::Complete(body) if body == "known failure"));
        assert_eq!(sends.load(Ordering::SeqCst), 1);
        let observed = observation.await.expect("observed outcome");
        assert_eq!(observed["retry_after_ms"], expected_hint_ms);
        assert_eq!(
            observed["outcome"],
            if status == 429 {
                "rate_limited"
            } else {
                "server_error"
            }
        );
        coordinator_task.await.expect("coordinator completed");
        provider_task.abort();
    }
    let (coordinator_task, observation) = coordinator(0).await;
    let key = SecretValue::new("synthetic-key").expect("secret");
    let client = CoordinatorClient::try_new("http://127.0.0.1/", &key, "fractional-retry")
        .expect("client setup")
        .expect("managed client");
    let lease = client
        .acquire(EndpointKind::Inference, None, Duration::from_secs(2))
        .await
        .expect("grant");
    let mut lease = RetryLease::new(lease);
    lease.headers_received(Duration::ZERO).await;
    let _ = lease
        .delay_for_retry(
            AttemptOutcome::RateLimited,
            Some(Duration::from_nanos(1)),
            1,
        )
        .await;
    assert_eq!(
        observation.await.expect("fractional hint")["retry_after_ms"],
        1
    );
    coordinator_task.await.expect("coordinator completed");
}

async fn coordinator(
    delay_ms: u64,
) -> (
    tokio::task::JoinHandle<()>,
    tokio::sync::oneshot::Receiver<Value>,
) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("coordinator listener");
    let directory = nan_harness_coordinator::config_directory()
        .expect("configuration")
        .join("coordinator/v1");
    std::fs::create_dir_all(&directory).expect("test directory");
    std::fs::write(
        directory.join("receipt.json"),
        serde_json::to_vec(&json!({
            "protocol_version": 2,
            "port": listener.local_addr().expect("address").port(),
            "token": "synthetic-token",
            "generation": "retry-wait-test",
            "pid": std::process::id(),
        }))
        .expect("receipt JSON"),
    )
    .expect("test receipt");
    let (sender, observation) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("coordinator connection");
        assert_eq!(read_json(&mut stream).await["type"], "acquire");
        write_json(
            &mut stream,
            json!({"type":"granted", "lease_id":1, "queued_ms":0}),
        )
        .await;
        assert_eq!(read_json(&mut stream).await["type"], "progress");
        let message = read_json(&mut stream).await;
        assert_eq!(message["type"], "observe");
        sender.send(message).expect("observation receiver");
        write_json(&mut stream, json!({"type":"retry", "delay_ms":delay_ms})).await;
    });
    (task, observation)
}

async fn read_json(stream: &mut TcpStream) -> Value {
    let length = stream.read_u32().await.expect("frame length");
    assert!(length < 16 * 1024);
    let mut payload = vec![0; length as usize];
    stream.read_exact(&mut payload).await.expect("frame");
    serde_json::from_slice(&payload).expect("JSON frame")
}

async fn write_json(stream: &mut TcpStream, message: Value) {
    let payload = serde_json::to_vec(&message).expect("JSON frame");
    stream
        .write_u32(u32::try_from(payload.len()).expect("length"))
        .await
        .expect("frame length");
    stream.write_all(&payload).await.expect("frame");
}
