// Shared private harness for the Chat Completions suite: a fake upstream
// provider that scripts every response the concern modules need, the local
// bridge launcher each test calls, and the usage-snapshot builders the
// assertions compare against.
//
// The upstream addresses are loopback-only and the credentials below are
// fixed synthetic values that exist so tests can assert what the bridge
// forwards; they are never real provider keys, prompts, or model output.

use axum::Json;
use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use nan_harness_bridge::{
    ChatCompletionsBridgeConfig, ModelUsageSnapshot, ProviderUsageSnapshot, RunningBridge,
    spawn_chat_completions,
};
use nan_harness_core::SecretValue;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::convert::Infallible;
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;
use tokio::sync::Notify;

/// What the fake upstream saw and the gate that releases a held-open stream.
#[derive(Clone, Default)]
pub(super) struct FakeState {
    pub(super) requests: Arc<Mutex<Vec<(HeaderMap, Value)>>>,
    pub(super) release_stream: Arc<Notify>,
}

/// A bridge plus the fake provider it points at, owned by one test.
pub(super) struct TestServers {
    pub(super) bridge: RunningBridge,
    pub(super) upstream_task: tokio::task::JoinHandle<()>,
    pub(super) state: FakeState,
}

impl TestServers {
    pub(super) async fn shutdown(mut self) {
        self.bridge.shutdown();
        self.bridge
            .wait()
            .await
            .expect("bridge should stop cleanly");
        self.upstream_task.abort();
    }
}

pub(super) fn usage_for(model: ModelUsageSnapshot) -> ProviderUsageSnapshot {
    usage_for_model("qwen3.6", model)
}

pub(super) fn usage_for_model(model_id: &str, model: ModelUsageSnapshot) -> ProviderUsageSnapshot {
    ProviderUsageSnapshot {
        models: BTreeMap::from([(model_id.to_owned(), model)]),
    }
}

pub(super) fn usage_for_models(
    models: impl IntoIterator<Item = (&'static str, ModelUsageSnapshot)>,
) -> ProviderUsageSnapshot {
    ProviderUsageSnapshot {
        models: models
            .into_iter()
            .map(|(model_id, usage)| (model_id.to_owned(), usage))
            .collect(),
    }
}

pub(super) async fn start_servers() -> TestServers {
    start_servers_with_search(true).await
}

pub(super) async fn start_servers_with_search(web_search_enabled: bool) -> TestServers {
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
        .route("/v1/search", post(fake_search))
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
            model_id: "qwen3.6".to_owned(),
            provider_api_key: Arc::new(SecretValue::new("provider-secret").expect("provider key")),
            session_token: Arc::new(
                SecretValue::new("local-session-token").expect("session token"),
            ),
            web_search_enabled,
        },
    )
    .expect("bridge should start");
    TestServers {
        bridge,
        upstream_task,
        state,
    }
}

async fn fake_search(headers: HeaderMap, Json(body): Json<Value>) -> Json<Value> {
    assert_eq!(headers[header::AUTHORIZATION], "Bearer provider-secret");
    assert_eq!(body["query"], "rust async");
    assert_eq!(body["count"], 1);
    Json(json!({
        "results": [{
            "title": "Tokio",
            "url": "https://tokio.rs",
            "snippet": "An asynchronous runtime for Rust."
        }]
    }))
}

async fn fake_models(headers: HeaderMap, body: Bytes) -> Response {
    assert!(!headers.contains_key(header::TRANSFER_ENCODING));
    assert!(body.is_empty());
    (
        [("x-upstream-marker", "models")],
        Json(json!({
            "object":"list",
            "data":[
                {"id":"qwen3.6"},
                {"id":"whisper"},
                {"id":"minimax-h3"}
            ]
        })),
    )
        .into_response()
}

/// The upstream script: each synthetic model name selects one response shape.
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
        if value["model"] == "done-without-usage" {
            return done_without_usage_response();
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

fn done_without_usage_response() -> Response {
    let body = async_stream::stream! {
        yield Ok::<Bytes, Infallible>(Bytes::from_static(
            b"data: {\"id\":\"done\",\"choices\":[]}\n\n",
        ));
        yield Ok::<Bytes, Infallible>(Bytes::from_static(b"data: [DONE]\n\n"));
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .body(Body::from_stream(body))
        .expect("done without usage response")
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

/// A JSON body past the bridge's observation bound, used by both the fake
/// upstream and the assertion that the delivered bytes stay unchanged.
pub(super) fn oversized_response_body() -> Vec<u8> {
    let mut body = Vec::with_capacity(1024 * 1024 + 128);
    body.extend_from_slice(b"{\"choices\":[{\"message\":{\"content\":\"");
    body.extend(std::iter::repeat_n(b'x', 1024 * 1024 + 1));
    body.extend_from_slice(b"\"}}],\"usage\":{\"prompt_tokens\":101,\"completion_tokens\":202}}");
    body
}
