use axum::{Router, body::Body, response::Response, routing::post};
use futures_util::StreamExt as _;
use nan_harness_bridge::ChatCompletionsBridgeConfig;
use nan_harness_core::SecretValue;
use serde_json::{Value, json};
use std::{future::Future, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::{TcpListener, TcpStream},
    sync::{Notify, mpsc},
};

const TEST_NAME: &str = "terminal_outcomes::chat_completions_settles_terminal_outcomes_once";
const CHILD_ENV: &str = "NAN_TEST_CHAT_TERMINAL_OUTCOMES";

#[tokio::test]
async fn chat_completions_settles_terminal_outcomes_once() {
    if std::env::var_os(CHILD_ENV).is_some() {
        exercise().await;
        return;
    }
    // Isolate coordinator discovery without modifying the test runner environment.
    let directory = tempfile::tempdir().expect("private test directory");
    let mut command = tokio::process::Command::new(std::env::current_exe().expect("test binary"));
    command
        .args(["--exact", TEST_NAME, "--nocapture"])
        .env_clear()
        .env(CHILD_ENV, "1")
        .env("NAN_HARNESS_CONFIG_DIR", directory.path())
        .env("NAN_HARNESS_INTERNAL_MANAGED_PROCESS", "1")
        .kill_on_drop(true);
    if let Some(profile) = std::env::var_os("LLVM_PROFILE_FILE") {
        command.env("LLVM_PROFILE_FILE", profile);
    }
    let mut child = command.spawn().expect("isolated test child");
    let result = tokio::time::timeout(Duration::from_mins(1), child.wait()).await;
    if result.is_err() {
        child.kill().await.expect("terminate test child");
        child.wait().await.expect("reap test child");
    }
    assert!(
        result
            .expect("test deadline")
            .expect("child status")
            .success()
    );
}

async fn exercise() {
    let (mut events, coordinator) = start_coordinator().await;
    let usage = b"data: {\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":7}}\n\n";
    for model in [None, Some("synthetic-model")] {
        for payload in [b"data: {}\n\n".as_slice(), usage.as_slice(), b""] {
            run_case(
                &mut events,
                model,
                true,
                200,
                payload.to_vec(),
                "invalid_response",
            )
            .await;
        }
        for payload in [
            b"data: [DONE]\n\n".to_vec(),
            b"data: [DONE]\r\r".to_vec(),
            b"\xef\xbb\xbfdata: [DONE]\r\r".to_vec(),
            [usage.as_slice(), b"data: [DONE]\r\n\r\n"].concat(),
        ] {
            run_case(&mut events, model, true, 200, payload, "success").await;
        }
    }
    run_case(&mut events, None, false, 200, b"{}".to_vec(), "success").await;
    run_case(
        &mut events,
        None,
        true,
        400,
        b"synthetic error".to_vec(),
        "terminal",
    )
    .await;
    let mut oversized = vec![b'x'; 1024 * 1024 + 1];
    run_case(&mut events, None, true, 200, oversized.clone(), "success").await;
    oversized.extend_from_slice(b"\ndata: [DONE]\n\n");
    run_case(
        &mut events,
        Some("synthetic-model"),
        true,
        200,
        oversized,
        "success",
    )
    .await;
    for ending in ["transport", "cancelled"] {
        for payload in [
            usage.to_vec(),
            [usage.as_slice(), b"data: [DONE]\n\n"].concat(),
        ] {
            run_case(
                &mut events,
                Some("synthetic-model"),
                true,
                200,
                payload,
                ending,
            )
            .await;
        }
    }
    coordinator.abort();
}

async fn run_case(
    events: &mut mpsc::UnboundedReceiver<Value>,
    model: Option<&str>,
    streaming: bool,
    status: u16,
    payload: Vec<u8>,
    expected: &'static str,
) {
    let release = Arc::new(Notify::new());
    let (provider_url, provider) =
        start_provider(payload.clone(), status, expected, release.clone()).await;
    let mut bridge = nan_harness_bridge::spawn_chat_completions(
        TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bridge listener"),
        ChatCompletionsBridgeConfig {
            launch_id: "terminal-outcomes".to_owned(),
            provider_base_url: provider_url,
            model_id: "synthetic-model".to_owned(),
            provider_api_key: Arc::new(SecretValue::new("synthetic-provider").expect("key")),
            session_token: Arc::new(SecretValue::new("synthetic-session").expect("token")),
            web_search_enabled: false,
        },
    )
    .expect("bridge");
    let response = bounded(
        reqwest::Client::new()
            .post(format!("{}/v1/chat/completions", bridge.base_url()))
            .bearer_auth("synthetic-session")
            .json(&json!({"model": model, "stream": streaming, "messages": []}))
            .send(),
    )
    .await
    .expect("response headers");
    assert_eq!(response.status().as_u16(), status);
    assert_eq!(
        bounded(events.recv()).await.expect("acquire")["type"],
        "acquire"
    );
    assert_eq!(
        bounded(events.recv()).await.expect("progress")["type"],
        "progress"
    );
    consume_response(response, &payload, expected, &release).await;
    if expected != "cancelled" {
        let observation = bounded(events.recv()).await.expect("observation");
        assert_eq!(observation["type"], "observe");
        assert_eq!(observation["outcome"], expected);
    }
    // EOF without Observe is the existing coordinator cancellation contract.
    // Any duplicate Observe or replayed Acquire fails this exact sequence.
    assert_eq!(
        bounded(events.recv()).await.expect("lease release")["type"],
        "closed"
    );
    if model.is_some() && streaming && status == 200 {
        let snapshot = bridge.usage();
        if matches!(expected, "invalid_response" | "transport" | "cancelled") {
            assert_eq!(snapshot.incomplete_responses(), 1);
        } else {
            assert_eq!(
                snapshot.responses_with_usage() + snapshot.responses_without_usage(),
                1
            );
        }
    }
    bridge.shutdown();
    bounded(bridge.wait()).await.expect("bridge shutdown");
    provider.abort();
    assert!(events.try_recv().is_err(), "no duplicate outcome or replay");
}

async fn start_provider(
    payload: Vec<u8>,
    status: u16,
    expected: &'static str,
    release: Arc<Notify>,
) -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("provider listener");
    let provider_url = format!(
        "http://{}/v1",
        listener.local_addr().expect("provider address")
    );
    let provider = tokio::spawn(async move {
        let app = Router::new().route(
            "/v1/chat/completions",
            post(move || {
                let payload = payload.clone();
                let release = release.clone();
                async move {
                    let body = async_stream::stream! {
                        yield Ok::<_, std::io::Error>(payload);
                        if matches!(expected, "transport" | "cancelled") {
                            release.notified().await;
                            yield Err(std::io::Error::other("synthetic transport failure"));
                        }
                    };
                    Response::builder()
                        .status(status)
                        .header("content-type", "text/event-stream")
                        .body(Body::from_stream(body))
                        .expect("provider response")
                }
            }),
        );
        axum::serve(listener, app).await.expect("provider server");
    });
    (provider_url, provider)
}

async fn consume_response(
    response: reqwest::Response,
    payload: &[u8],
    ending: &str,
    release: &Notify,
) {
    if !matches!(ending, "transport" | "cancelled") {
        assert_eq!(
            bounded(response.bytes())
                .await
                .expect("response bytes")
                .as_ref(),
            payload
        );
        return;
    }
    let mut body = response.bytes_stream();
    let mut received = Vec::with_capacity(payload.len());
    while received.len() < payload.len() {
        let chunk = bounded(body.next())
            .await
            .expect("payload chunk")
            .expect("payload bytes");
        received.extend_from_slice(&chunk);
    }
    assert_eq!(received, payload);
    if ending == "transport" {
        release.notify_one();
        assert!(bounded(body.next()).await.expect("body error").is_err());
    }
    drop(body);
}

async fn start_coordinator() -> (mpsc::UnboundedReceiver<Value>, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("coordinator listener");
    let directory = nan_harness_coordinator::config_directory()
        .expect("configuration")
        .join("coordinator/v1");
    std::fs::create_dir_all(&directory).expect("coordinator directory");
    let receipt = json!({"protocol_version": 2, "port": listener.local_addr().expect("address").port(),
        "token": "synthetic-coordinator", "generation": "chat-terminal", "pid": std::process::id()});
    std::fs::write(
        directory.join("receipt.json"),
        serde_json::to_vec(&receipt).expect("receipt JSON"),
    )
    .expect("receipt");
    let (events, receiver) = mpsc::unbounded_channel();
    let task = tokio::spawn(async move {
        loop {
            let (mut stream, _) = listener.accept().await.expect("lease connection");
            let acquire = read_json(&mut stream).await.expect("acquire");
            events.send(acquire).expect("acquire event");
            write_json(
                &mut stream,
                json!({"type": "granted", "lease_id": 1, "queued_ms": 0}),
            )
            .await;
            while let Some(message) = read_json(&mut stream).await {
                let observe = message["type"] == "observe";
                events.send(message).expect("lease event");
                if observe {
                    write_json(&mut stream, json!({"type": "complete"})).await;
                }
            }
            events
                .send(json!({"type": "closed"}))
                .expect("closed event");
        }
    });
    (receiver, task)
}

async fn read_json(stream: &mut TcpStream) -> Option<Value> {
    let length = match stream.read_u32().await {
        Ok(length) => length,
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => return None,
        Err(error) => panic!("coordinator frame: {error}"),
    };
    assert!(length <= 64 * 1024);
    let mut payload = vec![0; usize::try_from(length).expect("frame size")];
    stream
        .read_exact(&mut payload)
        .await
        .expect("frame payload");
    Some(serde_json::from_slice(&payload).expect("frame JSON"))
}

async fn write_json(stream: &mut TcpStream, value: Value) {
    let payload = serde_json::to_vec(&value).expect("frame JSON");
    stream
        .write_u32(u32::try_from(payload.len()).expect("frame size"))
        .await
        .expect("frame length");
    stream.write_all(&payload).await.expect("frame payload");
}

async fn bounded<T>(future: impl Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(10), future)
        .await
        .expect("test step deadline")
}
