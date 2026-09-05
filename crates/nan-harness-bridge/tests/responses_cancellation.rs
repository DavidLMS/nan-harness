use axum::{Router, body::Body, extract::State, response::Response, routing::post};
use nan_harness_bridge::{CodexModelCatalog, ResponsesBridgeConfig};
use nan_harness_coordinator::{CoordinatorClient, EndpointKind, RequestLease};
use nan_harness_core::SecretValue;
use serde_json::{Value, json};
use std::{convert::Infallible, future::Future, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::{TcpListener, TcpStream},
    sync::mpsc,
};

const STEP_TIMEOUT: Duration = Duration::from_secs(10);
const CHILD_SCENARIO: &str = "NAN_TEST_CANCELLATION_SCENARIO";

async fn bounded<T>(condition: &str, future: impl Future<Output = T>) -> T {
    tokio::time::timeout(STEP_TIMEOUT, future)
        .await
        .unwrap_or_else(|_| panic!("{condition} exceeded ten seconds"))
}

#[tokio::test]
async fn responses_disconnect_while_queued() {
    run_isolated("queued").await;
}

#[tokio::test]
async fn responses_disconnect_during_upstream_stream() {
    run_isolated("stream").await;
}

async fn run_isolated(scenario: &str) {
    if let Ok(selected) = std::env::var(CHILD_SCENARIO) {
        assert_eq!(selected, scenario);
        exercise(scenario == "stream").await;
        return;
    }
    let directory = tempfile::tempdir().expect("private test directory");
    let name = if scenario == "stream" {
        "responses_disconnect_during_upstream_stream"
    } else {
        "responses_disconnect_while_queued"
    };
    let mut child = tokio::process::Command::new(std::env::current_exe().expect("test binary"))
        .args(["--exact", name, "--nocapture"])
        .env_clear()
        .env(CHILD_SCENARIO, scenario)
        .env("NAN_HARNESS_CONFIG_DIR", directory.path())
        .env("NAN_HARNESS_INTERNAL_MANAGED_PROCESS", "1")
        .kill_on_drop(true)
        .spawn()
        .expect("isolated test process");
    let result = tokio::time::timeout(Duration::from_mins(1), child.wait()).await;
    if result.is_err() {
        child.kill().await.expect("terminate timed-out child");
        child.wait().await.expect("reap timed-out child");
    }
    assert!(
        result
            .expect("child exceeded sixty seconds")
            .expect("child status")
            .success()
    );
}

#[derive(Debug, PartialEq, Eq)]
enum CoordinationEvent {
    Acquire(usize),
    Closed(usize),
}

async fn start_relay() -> (
    tokio::task::JoinHandle<()>,
    mpsc::UnboundedReceiver<CoordinationEvent>,
) {
    let receipt_path = nan_harness_coordinator::config_directory()
        .expect("configuration")
        .join("coordinator/v1/receipt.json");
    let mut receipt: Value = bounded("daemon receipt", async {
        loop {
            if let Ok(bytes) = std::fs::read(&receipt_path)
                && let Ok(value) = serde_json::from_slice(&bytes)
            {
                break value;
            }
            tokio::task::yield_now().await;
        }
    })
    .await;
    let daemon_port =
        u16::try_from(receipt["port"].as_u64().expect("daemon port")).expect("port fits");
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("relay listener");
    receipt["port"] = json!(listener.local_addr().expect("relay address").port());
    // Preserve the daemon-created private file's permissions; only redirect this child's receipt.
    std::fs::write(
        &receipt_path,
        serde_json::to_vec(&receipt).expect("receipt JSON"),
    )
    .expect("redirect receipt");
    let (events, receiver) = mpsc::unbounded_channel();
    let task = tokio::spawn(async move {
        let next_id = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut connections = tokio::task::JoinSet::new();
        loop {
            let (mut client, _) = listener.accept().await.expect("relay accept");
            let events = events.clone();
            let next_id = Arc::clone(&next_id);
            connections.spawn(async move {
                let mut daemon = TcpStream::connect(("127.0.0.1", daemon_port))
                    .await
                    .expect("real daemon");
                let Ok(length) = client.read_u32().await else {
                    return;
                };
                assert!(length < 64 * 1024);
                let mut payload = vec![0; length as usize];
                client
                    .read_exact(&mut payload)
                    .await
                    .expect("Acquire payload");
                let message: Value = serde_json::from_slice(&payload).expect("Acquire JSON");
                assert_eq!(message["type"], "acquire");
                let connection_id = next_id.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                daemon.write_u32(length).await.expect("forward length");
                daemon.write_all(&payload).await.expect("forward Acquire");
                events
                    .send(CoordinationEvent::Acquire(connection_id))
                    .expect("Acquire observer");
                let _ = tokio::io::copy_bidirectional(&mut client, &mut daemon).await;
                events
                    .send(CoordinationEvent::Closed(connection_id))
                    .expect("close observer");
            });
        }
    });
    (task, receiver)
}

#[derive(Clone)]
struct Provider {
    stall_first: bool,
    requests: Arc<std::sync::atomic::AtomicUsize>,
    events: mpsc::UnboundedSender<&'static str>,
}

struct DroppedBody(mpsc::UnboundedSender<&'static str>);

impl Drop for DroppedBody {
    fn drop(&mut self) {
        let _ = self.0.send("dropped");
    }
}

async fn provider(State(state): State<Provider>) -> Response {
    let index = state
        .requests
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let body = if state.stall_first && index == 0 {
        let guard = DroppedBody(state.events.clone());
        Body::from_stream(async_stream::stream! {
            let _guard = guard;
            yield Ok::<_, Infallible>("data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\n");
            state.events.send("streaming").expect("stream observer");
            std::future::pending::<()>().await;
        })
    } else {
        Body::from(
            "data: {\"choices\":[{\"delta\":{\"content\":\"done\"},\"finish_reason\":null}]}\n\ndata: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n",
        )
    };
    Response::builder()
        .header("content-type", "text/event-stream")
        .body(body)
        .expect("provider response")
}

async fn hold(client: &CoordinatorClient) -> RequestLease {
    bounded(
        "held permit",
        client.acquire(EndpointKind::Inference, Some("qwen3.6"), STEP_TIMEOUT),
    )
    .await
    .expect("acquire permit")
    .expect("coordination enabled")
}

async fn request(base_url: &str) -> TcpStream {
    let mut socket = TcpStream::connect(base_url.trim_start_matches("http://"))
        .await
        .expect("bridge client");
    let body = json!({"model":"qwen3.6", "input":[{"type":"message", "role":"user", "content":[{"type":"input_text", "text":"Synthetic cancellation test"}]}], "stream":true})
        .to_string();
    socket.write_all(format!("POST /v1/responses HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer synthetic-session\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.expect("Responses request");
    let mut headers = Vec::new();
    bounded("HTTP response headers", async {
        while !headers.ends_with(b"\r\n\r\n") {
            headers.push(socket.read_u8().await.expect("HTTP response headers"));
        }
    })
    .await;
    assert!(
        headers.starts_with(b"HTTP/1.1 200"),
        "{}",
        String::from_utf8_lossy(&headers)
    );
    socket
}

async fn start_bridge(
    provider_url: String,
    key: Arc<SecretValue>,
) -> nan_harness_bridge::RunningBridge {
    nan_harness_bridge::spawn_responses(
        TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bridge listener"),
        ResponsesBridgeConfig {
            launch_id: "cancellation-test".to_owned(),
            provider_base_url: provider_url,
            models: CodexModelCatalog::from_provider_ids(["qwen3.6".to_owned()], "qwen3.6")
                .expect("catalog"),
            provider_api_key: key,
            session_token: Arc::new(SecretValue::new("synthetic-session").expect("session")),
            web_search_enabled: false,
        },
    )
    .expect("bridge")
}

async fn exercise(streaming: bool) {
    let daemon = tokio::spawn(nan_harness_coordinator::run_daemon());
    let (relay, mut coordination) = start_relay().await;
    let (events, mut provider_events) = mpsc::unbounded_channel();
    let state = Provider {
        stall_first: streaming,
        requests: Arc::default(),
        events,
    };
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("provider listener");
    let provider_url = format!(
        "http://{}/v1",
        listener.local_addr().expect("provider address")
    );
    let app = Router::new()
        .route("/v1/chat/completions", post(provider))
        .with_state(state.clone());
    let upstream = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("provider server");
    });
    let key = Arc::new(SecretValue::new("synthetic-provider").expect("key"));
    let client = CoordinatorClient::try_new(&provider_url, &key, "held-permits")
        .expect("coordinator client")
        .expect("managed child");
    let held = hold(&client).await;
    assert_eq!(
        bounded("first held Acquire", coordination.recv()).await,
        Some(CoordinationEvent::Acquire(0))
    );
    let second = if streaming {
        None
    } else {
        Some(hold(&client).await)
    };
    if !streaming {
        assert_eq!(
            bounded("second held Acquire", coordination.recv()).await,
            Some(CoordinationEvent::Acquire(1))
        );
    }
    let cancelled_id = if streaming { 1 } else { 2 };
    let mut bridge = start_bridge(provider_url, key).await;
    let cancelled = request(bridge.base_url()).await;
    assert_eq!(
        bounded("cancelled request Acquire", coordination.recv()).await,
        Some(CoordinationEvent::Acquire(cancelled_id))
    );
    if streaming {
        assert_eq!(
            bounded("provider stream start", provider_events.recv()).await,
            Some("streaming")
        );
    }
    let mut successor = request(bridge.base_url()).await;
    assert_eq!(
        bounded("successor Acquire", coordination.recv()).await,
        Some(CoordinationEvent::Acquire(cancelled_id + 1))
    );
    drop(cancelled);
    assert_eq!(
        bounded("cancelled IPC connection close", coordination.recv()).await,
        Some(CoordinationEvent::Closed(cancelled_id))
    );
    if streaming {
        assert_eq!(
            bounded("provider stream drop", provider_events.recv()).await,
            Some("dropped")
        );
    } else {
        assert_eq!(state.requests.load(std::sync::atomic::Ordering::SeqCst), 0);
        drop(second);
    }
    let mut response = String::new();
    bounded(
        "successor completion",
        successor.read_to_string(&mut response),
    )
    .await
    .expect("successor response");
    assert!(
        response.contains("response.completed"),
        "successor must complete"
    );
    assert_eq!(
        state.requests.load(std::sync::atomic::Ordering::SeqCst),
        if streaming { 2 } else { 1 }
    );
    drop(held);
    bridge.shutdown();
    bounded("bridge shutdown", bridge.wait())
        .await
        .expect("bridge shutdown");
    upstream.abort();
    relay.abort();
    daemon.abort();
}
