use axum::{Router, body::Body, extract::Request, response::Response, routing::post};
use nan_harness_bridge::{CodexModelCatalog, ResponsesBridgeConfig};
use nan_harness_core::SecretValue;
use serde_json::{Value, json};
use std::{ffi::OsStr, future::Future, path::Path, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::{TcpListener, TcpStream},
    sync::mpsc,
};

const STEP_TIMEOUT: Duration = Duration::from_secs(10);
const CHILD_SCENARIO: &str = "NAN_TEST_CONTROL_ACK_SCENARIO";
const PROFILE_FILE: &str = "LLVM_PROFILE_FILE";

#[tokio::test]
async fn responses_terminal_data_survives_a_control_acknowledgement_stall() {
    if std::env::var(CHILD_SCENARIO).is_ok_and(|scenario| scenario == "terminal-stall") {
        exercise().await;
        return;
    }
    run_isolated().await;
}

async fn run_isolated() {
    let directory = tempfile::tempdir().expect("private test directory");
    let profile_file = std::env::var_os(PROFILE_FILE);
    let mut child = isolated_child_command(directory.path(), profile_file.as_deref())
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

fn isolated_child_command(
    directory: &Path,
    profile_file: Option<&OsStr>,
) -> tokio::process::Command {
    let mut command = tokio::process::Command::new(std::env::current_exe().expect("test binary"));
    command.args([
        "--exact",
        "responses_terminal_data_survives_a_control_acknowledgement_stall",
        "--nocapture",
    ]);
    command
        .env_clear()
        .env(CHILD_SCENARIO, "terminal-stall")
        .env("NAN_HARNESS_CONFIG_DIR", directory)
        .env("NAN_HARNESS_INTERNAL_MANAGED_PROCESS", "1")
        .kill_on_drop(true);
    if let Some(profile_file) = profile_file {
        command.env(PROFILE_FILE, profile_file);
    }
    command
}

#[test]
fn isolated_child_command_forwards_profile_file_conditionally() {
    let directory = tempfile::tempdir().expect("private test directory");
    let command =
        isolated_child_command(directory.path(), Some(OsStr::new("coverage/%p-%m.profraw")));
    let profile_file = command
        .as_std()
        .get_envs()
        .find(|(name, _)| *name == OsStr::new(PROFILE_FILE))
        .and_then(|(_, value)| value);
    assert_eq!(profile_file, Some(OsStr::new("coverage/%p-%m.profraw")));

    let command = isolated_child_command(directory.path(), None);
    let profile_file = command
        .as_std()
        .get_envs()
        .find(|(name, _)| *name == OsStr::new(PROFILE_FILE))
        .and_then(|(_, value)| value);
    assert_eq!(profile_file, None);
}

async fn start_fake_coordinator() -> mpsc::UnboundedReceiver<&'static str> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("coordinator listener");
    let address = listener.local_addr().expect("coordinator address");
    let receipt = json!({
        "protocol_version": 3,
        "port": address.port(),
        "token": "synthetic-coordinator-token",
        "generation": "control-ack-test",
        "pid": std::process::id(),
    });
    let directory = nan_harness_coordinator::config_directory()
        .expect("configuration")
        .join("coordinator/v1");
    std::fs::create_dir_all(&directory).expect("coordinator directory");
    std::fs::write(
        directory.join("receipt.json"),
        serde_json::to_vec(&receipt).expect("receipt JSON"),
    )
    .expect("receipt");
    let (events, receiver) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let events = events.clone();
            tokio::spawn(async move {
                let message = read_json(&mut stream).await;
                assert_eq!(message["type"], "acquire");
                write_json(
                    &mut stream,
                    json!({"type": "granted", "lease_id": 1, "queued_ms": 0}),
                )
                .await;
                loop {
                    let message = read_json(&mut stream).await;
                    match message["type"].as_str() {
                        Some("progress") => {
                            let _ = events.send("progress");
                        }
                        Some("observe") => {
                            let _ = events.send("observe");
                            let error = stream.read_u8().await.expect_err("lease must close");
                            assert_eq!(error.kind(), std::io::ErrorKind::UnexpectedEof);
                            let _ = events.send("closed");
                            return;
                        }
                        _ => panic!("unexpected coordinator message"),
                    }
                }
            });
        }
    });
    receiver
}

async fn read_json(stream: &mut TcpStream) -> Value {
    let length = stream.read_u32().await.expect("coordinator frame length");
    let mut payload = vec![0; usize::try_from(length).expect("frame length")];
    stream
        .read_exact(&mut payload)
        .await
        .expect("coordinator frame");
    serde_json::from_slice(&payload).expect("coordinator JSON")
}

async fn write_json(stream: &mut TcpStream, value: Value) {
    let payload = serde_json::to_vec(&value).expect("coordinator JSON");
    stream
        .write_u32(u32::try_from(payload.len()).expect("frame length"))
        .await
        .expect("coordinator frame length");
    stream.write_all(&payload).await.expect("coordinator frame");
}

async fn exercise() {
    let mut coordination = start_fake_coordinator().await;
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("provider listener");
    let provider_url = format!(
        "http://{}/v1",
        listener.local_addr().expect("provider address")
    );
    let provider = tokio::spawn(async move {
        let app = Router::new().route("/v1/chat/completions", post(provider_response));
        axum::serve(listener, app).await.expect("provider server");
    });
    let key = Arc::new(SecretValue::new("synthetic-provider").expect("key"));
    let mut bridge = nan_harness_bridge::spawn_responses(
        TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bridge listener"),
        ResponsesBridgeConfig {
            launch_id: "control-ack-test".to_owned(),
            provider_base_url: provider_url,
            models: CodexModelCatalog::from_provider_ids(["qwen3.6".to_owned()], "qwen3.6")
                .expect("catalog"),
            provider_api_key: key,
            session_token: Arc::new(SecretValue::new("synthetic-session").expect("session")),
            web_search_enabled: false,
            search_config: None,
            session_max_tokens: None,
        },
    )
    .expect("bridge");
    let mut client = request(bridge.base_url()).await;
    assert_eq!(
        bounded("progress acknowledgement", coordination.recv()).await,
        Some("progress")
    );
    assert_eq!(
        bounded("observe acknowledgement", coordination.recv()).await,
        Some("observe")
    );
    assert_eq!(
        bounded("lease release", coordination.recv()).await,
        Some("closed")
    );
    let mut response = String::new();
    bounded("terminal response", client.read_to_string(&mut response))
        .await
        .expect("terminal response");
    assert!(response.contains("response.completed"), "{response}");
    bridge.shutdown();
    bounded("bridge shutdown", bridge.wait())
        .await
        .expect("bridge shutdown");
    provider.abort();
}

async fn provider_response(_: Request) -> Response {
    Response::builder()
        .header("content-type", "text/event-stream")
        .body(Body::from(
            "data: {\"id\":\"resp_1\",\"choices\":[{\"delta\":{\"content\":\"done\"},\"finish_reason\":null}]}\n\ndata: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n",
        ))
        .expect("provider response")
}

async fn bounded<T>(condition: &str, future: impl Future<Output = T>) -> T {
    tokio::time::timeout(STEP_TIMEOUT, future)
        .await
        .unwrap_or_else(|_| panic!("{condition} exceeded ten seconds"))
}

async fn request(base_url: &str) -> TcpStream {
    let mut socket = TcpStream::connect(base_url.trim_start_matches("http://"))
        .await
        .expect("bridge client");
    let body = json!({"model":"qwen3.6", "input":[{"type":"message", "role":"user", "content":[{"type":"input_text", "text":"Synthetic control acknowledgement test"}]}], "stream":true})
        .to_string();
    socket
        .write_all(
            format!("POST /v1/responses HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer synthetic-session\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes(),
        )
        .await
        .expect("Responses request");
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
