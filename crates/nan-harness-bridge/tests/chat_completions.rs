use axum::Json;
use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use futures_util::StreamExt;
use nan_harness_bridge::{
    ChatCompletionsBridgeConfig, ChatUsageSnapshot, RunningBridge, spawn_chat_completions,
};
use nan_harness_core::SecretValue;
use serde_json::{Value, json};
use std::convert::Infallible;
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;
use tokio::sync::Notify;

#[derive(Clone, Default)]
struct FakeState {
    requests: Arc<Mutex<Vec<(HeaderMap, Value)>>>,
    release_stream: Arc<Notify>,
}

struct TestServers {
    bridge: RunningBridge,
    upstream_task: tokio::task::JoinHandle<()>,
    state: FakeState,
}

impl TestServers {
    async fn shutdown(mut self) {
        self.bridge.shutdown();
        self.bridge
            .wait()
            .await
            .expect("bridge should stop cleanly");
        self.upstream_task.abort();
    }
}

#[tokio::test]
async fn chat_bridge_authenticates_and_preserves_models_and_error_responses() {
    let servers = start_servers().await;
    let client = reqwest::Client::new();

    let unauthorized = client
        .get(format!("{}/v1/models", servers.bridge.base_url()))
        .send()
        .await
        .expect("models request should complete");
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

    let models = client
        .get(format!(
            "{}/v1/models?include=owned_by",
            servers.bridge.base_url()
        ))
        .bearer_auth("local-session-token")
        .send()
        .await
        .expect("models request should complete");
    assert_eq!(models.status(), StatusCode::OK);
    assert_eq!(models.headers()["x-upstream-marker"], "models");
    assert_eq!(
        models.json::<Value>().await.expect("models JSON"),
        json!({
            "object": "list",
            "data": [{"id": "qwen3.6"}]
        })
    );

    let unknown = client
        .get(format!("{}/v1/unknown", servers.bridge.base_url()))
        .bearer_auth("local-session-token")
        .send()
        .await
        .expect("unknown path should complete");
    assert_eq!(unknown.status(), StatusCode::NOT_FOUND);

    let error = client
        .post(format!(
            "{}/v1/chat/completions?trace=one",
            servers.bridge.base_url()
        ))
        .bearer_auth("local-session-token")
        .header("x-client-marker", "preserved")
        .json(&json!({"model":"error","messages":[]}))
        .send()
        .await
        .expect("error response should complete");
    assert_eq!(error.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(error.headers()["x-upstream-marker"], "error");
    assert_eq!(
        error.text().await.expect("error body"),
        r#"{"error":"rate limited"}"#
    );

    {
        let requests = servers.state.requests.lock().expect("request lock");
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].0[header::AUTHORIZATION],
            "Bearer provider-secret"
        );
        assert_eq!(requests[0].0["x-client-marker"], "preserved");
        assert_eq!(requests[0].1["model"], "error");
    }
    assert_eq!(
        servers.bridge.chat_usage(),
        Some(ChatUsageSnapshot::default())
    );
    servers.shutdown().await;
}

#[tokio::test]
async fn chat_bridge_forwards_stream_chunks_before_upstream_completion_and_observes_usage() {
    let servers = start_servers().await;
    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/v1/chat/completions", servers.bridge.base_url()))
        .bearer_auth("local-session-token")
        .json(&json!({
            "model":"qwen3.6",
            "messages":[{"role":"user","content":"hello"}],
            "stream":true,
            "tools":[{"type":"function","function":{"name":"run","parameters":{"type":"object"}}}],
            "reasoning_effort":"high"
        }))
        .send()
        .await
        .expect("stream request should complete");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "text/event-stream"
    );

    let mut body = response.bytes_stream();
    let first = tokio::time::timeout(std::time::Duration::from_secs(1), body.next())
        .await
        .expect("first chunk should arrive before upstream completes")
        .expect("stream should contain a first chunk")
        .expect("first chunk should be readable");
    assert!(
        first.starts_with(b"data: {\"id\":\"first\",\"choices\""),
        "{first:?}"
    );
    assert_eq!(
        servers.bridge.chat_usage(),
        Some(ChatUsageSnapshot::default())
    );

    servers.state.release_stream.notify_one();
    let mut rest = Vec::new();
    while let Some(chunk) = body.next().await {
        rest.extend_from_slice(&chunk.expect("stream chunk should be readable"));
    }
    assert!(
        rest.windows(b"usage".len())
            .any(|window| window == b"usage")
    );
    assert!(rest.ends_with(b"data: [DONE]\n\n"));
    assert_eq!(
        servers.state.requests.lock().expect("request lock").len(),
        1
    );
    let request = servers.state.requests.lock().expect("request lock")[0]
        .1
        .clone();
    assert_eq!(request["stream_options"]["include_usage"], true);
    assert_eq!(request["tools"][0]["function"]["name"], "run");
    assert_eq!(request["reasoning_effort"], "high");

    assert_eq!(
        servers.bridge.chat_usage(),
        Some(ChatUsageSnapshot {
            completed_requests: 1,
            responses_with_usage: 1,
            responses_without_usage: 0,
            incomplete_responses: 0,
            prompt_tokens: 17,
            completion_tokens: 9,
            reasoning_tokens: 4,
        })
    );
    servers.shutdown().await;
}

#[tokio::test]
async fn chat_bridge_preserves_non_streaming_fields_and_usage() {
    let servers = start_servers().await;
    let response = reqwest::Client::new()
        .post(format!("{}/v1/chat/completions", servers.bridge.base_url()))
        .bearer_auth("local-session-token")
        .json(&json!({"model":"qwen3.6","messages":[],"stream":false}))
        .send()
        .await
        .expect("non-stream request should complete");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.json::<Value>().await.expect("response JSON")["choices"][0]["message"]["content"],
        "hello"
    );
    assert_eq!(
        servers.bridge.chat_usage(),
        Some(ChatUsageSnapshot {
            completed_requests: 1,
            responses_with_usage: 1,
            responses_without_usage: 0,
            incomplete_responses: 0,
            prompt_tokens: 3,
            completion_tokens: 2,
            reasoning_tokens: 0,
        })
    );
    servers.shutdown().await;
}

#[tokio::test]
async fn chat_bridge_passes_malformed_streams_and_rejects_oversized_requests() {
    let servers = start_servers().await;
    let client = reqwest::Client::new();
    let malformed = client
        .post(format!("{}/v1/chat/completions", servers.bridge.base_url()))
        .bearer_auth("local-session-token")
        .json(&json!({"model":"malformed","stream":true}))
        .send()
        .await
        .expect("malformed stream request should complete");
    assert_eq!(malformed.status(), StatusCode::OK);
    assert_eq!(
        malformed.text().await.expect("malformed stream body"),
        "data: {not-json}\n\n"
    );
    assert_eq!(
        servers.bridge.chat_usage(),
        Some(ChatUsageSnapshot {
            incomplete_responses: 1,
            ..ChatUsageSnapshot::default()
        })
    );

    let oversized = vec![b'x'; 32 * 1024 * 1024 + 1];
    let response = client
        .post(format!("{}/v1/chat/completions", servers.bridge.base_url()))
        .bearer_auth("local-session-token")
        .body(oversized)
        .send()
        .await
        .expect("oversized request should complete");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        servers.bridge.chat_usage(),
        Some(ChatUsageSnapshot {
            incomplete_responses: 1,
            ..ChatUsageSnapshot::default()
        })
    );
    servers.shutdown().await;
}

#[tokio::test]
async fn chat_bridge_requires_done_before_committing_stream_usage() {
    let servers = start_servers().await;
    let client = reqwest::Client::new();

    let truncated = client
        .post(format!("{}/v1/chat/completions", servers.bridge.base_url()))
        .bearer_auth("local-session-token")
        .json(&json!({"model":"usage-before-truncated","stream":true}))
        .send()
        .await
        .expect("truncated usage stream should complete headers");
    assert_eq!(truncated.status(), StatusCode::OK);
    assert!(
        truncated
            .text()
            .await
            .expect("truncated usage body")
            .contains("usage")
    );
    assert_eq!(
        servers.bridge.chat_usage(),
        Some(ChatUsageSnapshot {
            incomplete_responses: 1,
            ..ChatUsageSnapshot::default()
        })
    );

    let split = client
        .post(format!("{}/v1/chat/completions", servers.bridge.base_url()))
        .bearer_auth("local-session-token")
        .json(&json!({"model":"split-usage","stream":true}))
        .send()
        .await
        .expect("split usage stream should complete headers");
    assert_eq!(split.status(), StatusCode::OK);
    assert!(
        split
            .text()
            .await
            .expect("split usage body")
            .ends_with("[DONE]\n\n")
    );
    assert_eq!(
        servers.bridge.chat_usage(),
        Some(ChatUsageSnapshot {
            completed_requests: 1,
            responses_with_usage: 1,
            incomplete_responses: 1,
            prompt_tokens: 5,
            completion_tokens: 7,
            reasoning_tokens: 2,
            ..ChatUsageSnapshot::default()
        })
    );
    servers.shutdown().await;
}

#[tokio::test]
async fn chat_bridge_bounds_observation_without_changing_large_response_bodies() {
    let servers = start_servers().await;
    let response = reqwest::Client::new()
        .post(format!("{}/v1/chat/completions", servers.bridge.base_url()))
        .bearer_auth("local-session-token")
        .json(&json!({"model":"oversized","stream":false}))
        .send()
        .await
        .expect("oversized response should complete");
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.bytes().await.expect("oversized response body");
    let expected = oversized_response_body();
    assert_eq!(body.as_ref(), expected.as_slice());
    assert!(body.len() > 1024 * 1024);
    assert_eq!(
        servers.bridge.chat_usage(),
        Some(ChatUsageSnapshot {
            completed_requests: 1,
            responses_without_usage: 1,
            ..ChatUsageSnapshot::default()
        })
    );
    servers.shutdown().await;
}

#[tokio::test]
async fn chat_bridge_does_not_commit_usage_after_a_body_error() {
    let servers = start_servers().await;
    let response = reqwest::Client::new()
        .post(format!("{}/v1/chat/completions", servers.bridge.base_url()))
        .bearer_auth("local-session-token")
        .json(&json!({"model":"body-error","stream":true}))
        .send()
        .await
        .expect("body error stream should complete headers");
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.bytes().await.is_err());
    assert_eq!(
        servers.bridge.chat_usage(),
        Some(ChatUsageSnapshot::default())
    );
    servers.shutdown().await;
}

async fn start_servers() -> TestServers {
    let upstream_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("upstream should bind");
    let upstream_address = upstream_listener
        .local_addr()
        .expect("upstream address should be available");
    let state = FakeState::default();
    let router = Router::new()
        .route("/v1/models", get(fake_models))
        .route("/v1/chat/completions", post(fake_chat))
        .with_state(state.clone());
    let upstream_task = tokio::spawn(async move {
        axum::serve(upstream_listener, router)
            .await
            .expect("upstream should serve");
    });

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bridge should bind");
    let bridge = spawn_chat_completions(
        listener,
        ChatCompletionsBridgeConfig {
            provider_base_url: format!("http://{upstream_address}/v1"),
            provider_api_key: Arc::new(SecretValue::new("provider-secret").expect("provider key")),
            session_token: Arc::new(
                SecretValue::new("local-session-token").expect("session token"),
            ),
        },
    )
    .expect("bridge should start");
    TestServers {
        bridge,
        upstream_task,
        state,
    }
}

async fn fake_models() -> Response {
    (
        [("x-upstream-marker", "models")],
        Json(json!({"object":"list","data":[{"id":"qwen3.6"}]})),
    )
        .into_response()
}

async fn fake_chat(State(state): State<FakeState>, headers: HeaderMap, body: Bytes) -> Response {
    let value: Value = serde_json::from_slice(&body).expect("chat body should be JSON");
    state
        .requests
        .lock()
        .expect("request lock")
        .push((headers, value.clone()));
    assert_eq!(
        state
            .requests
            .lock()
            .expect("request lock")
            .last()
            .expect("request")
            .0[header::AUTHORIZATION],
        "Bearer provider-secret"
    );
    if value["model"] == "error" {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            [("x-upstream-marker", "error")],
            Body::from(r#"{"error":"rate limited"}"#),
        )
            .into_response();
    }
    if value["model"] == "oversized" {
        return Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(oversized_response_body()))
            .expect("oversized response");
    }
    if value["stream"] == true {
        let release = state.release_stream.clone();
        if value["model"] == "usage-before-truncated" {
            return truncated_usage_response();
        }
        if value["model"] == "split-usage" {
            return split_usage_response();
        }
        if value["model"] == "body-error" {
            return body_error_response();
        }
        return normal_stream_response(value, release);
    }
    Json(json!({
        "id":"response-1",
        "choices":[{"message":{"content":"hello"}}],
        "usage":{"prompt_tokens":3,"completion_tokens":2}
    }))
    .into_response()
}

fn truncated_usage_response() -> Response {
    let body = async_stream::stream! {
        yield Ok::<Bytes, Infallible>(Bytes::from_static(
            b"data: {\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":7}}\n\n",
        ));
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .body(Body::from_stream(body))
        .expect("truncated usage response")
}

fn split_usage_response() -> Response {
    let body = async_stream::stream! {
        yield Ok::<Bytes, Infallible>(Bytes::from_static(
            b"data: {\"usage\":{\"prompt_tokens\":",
        ));
        yield Ok::<Bytes, Infallible>(Bytes::from_static(
            b"5,\"completion_tokens\":7,\"completion_tokens_details\":{\"reasoning_tokens\":2}}}\n\n",
        ));
        yield Ok::<Bytes, Infallible>(Bytes::from_static(b"data: [DONE]\n\n"));
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .body(Body::from_stream(body))
        .expect("split usage response")
}

fn body_error_response() -> Response {
    let body = async_stream::stream! {
        yield Ok::<Bytes, std::io::Error>(Bytes::from_static(
            b"data: {\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":7}}\n\n",
        ));
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        yield Err(std::io::Error::other("synthetic body failure"));
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .body(Body::from_stream(body))
        .expect("body error response")
}

fn normal_stream_response(value: Value, release: Arc<Notify>) -> Response {
    let stream_body = match value["model"].as_str() {
        Some("malformed") => Bytes::from_static(b"data: {not-json}\n\n"),
        _ => Bytes::from_static(
            b"data: {\"id\":\"first\",\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\n",
        ),
    };
    let body = async_stream::stream! {
        yield Ok::<Bytes, Infallible>(stream_body);
        if value["model"] == "malformed" {
            return;
        }
        release.notified().await;
        yield Ok::<Bytes, Infallible>(Bytes::from_static(
            b"data: {\"id\":\"last\",\"choices\":[],\"usage\":{\"prompt_tokens\":17,\"completion_tokens\":9,\"completion_tokens_details\":{\"reasoning_tokens\":4}}}\n\n",
        ));
        yield Ok::<Bytes, Infallible>(Bytes::from_static(b"data: [DONE]\n\n"));
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .body(Body::from_stream(body))
        .expect("stream response")
}

fn oversized_response_body() -> Vec<u8> {
    let mut body = Vec::with_capacity(1024 * 1024 + 128);
    body.extend_from_slice(b"{\"choices\":[{\"message\":{\"content\":\"");
    body.extend(std::iter::repeat_n(b'x', 1024 * 1024 + 1));
    body.extend_from_slice(b"\"}}],\"usage\":{\"prompt_tokens\":101,\"completion_tokens\":202}}");
    body
}
