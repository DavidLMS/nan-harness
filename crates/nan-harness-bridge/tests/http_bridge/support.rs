use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use nan_harness_bridge::{
    BridgeConfig, ClaudeModelCatalog, ModelUsageSnapshot, ProviderUsageSnapshot, RunningBridge,
};
use nan_harness_core::SecretValue;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

pub(super) const SESSION_TOKEN: &str = "local-session-token";
const PROVIDER_API_KEY: &str = "nan-test-key";

#[derive(Clone, Default)]
pub(super) struct FakeNanState {
    pub(super) requests: Arc<Mutex<Vec<Value>>>,
}

pub(super) struct TestServers {
    pub(super) bridge: RunningBridge,
    upstream_task: JoinHandle<()>,
    pub(super) state: FakeNanState,
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

pub(super) fn usage_for(
    models: impl IntoIterator<Item = (&'static str, ModelUsageSnapshot)>,
) -> ProviderUsageSnapshot {
    ProviderUsageSnapshot {
        models: models
            .into_iter()
            .map(|(model, usage)| (model.to_owned(), usage))
            .collect::<BTreeMap<_, _>>(),
    }
}

pub(super) async fn post_messages(
    servers: &TestServers,
    path: &str,
    body: &Value,
) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!("{}{path}", servers.bridge.base_url()))
        .bearer_auth(SESSION_TOKEN)
        .json(body)
        .send()
        .await
        .expect("message request should complete")
}

pub(super) async fn start_servers() -> TestServers {
    let state = FakeNanState::default();
    let upstream_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("upstream should bind");
    let upstream_address = upstream_listener
        .local_addr()
        .expect("upstream address should exist");
    let upstream = Router::new()
        .route("/v1/chat/completions", post(fake_chat_completions))
        .route("/v1/search", post(fake_web_search))
        .with_state(state.clone());
    let upstream_task = tokio::spawn(async move {
        axum::serve(upstream_listener, upstream)
            .await
            .expect("fake upstream should serve");
    });

    let bridge_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bridge should bind");
    let bridge = nan_harness_bridge::spawn(
        bridge_listener,
        BridgeConfig {
            launch_id: "anthropic_test".to_owned(),
            provider_base_url: format!("http://{upstream_address}/v1"),
            models: ClaudeModelCatalog::from_provider_ids(
                [
                    "qwen3.6".to_owned(),
                    "deepseek-v4-flash".to_owned(),
                    "mimo-v2.5".to_owned(),
                    "gemma4".to_owned(),
                ],
                "qwen3.6",
            )
            .expect("model catalog should build"),
            provider_api_key: Arc::new(SecretValue::new(PROVIDER_API_KEY).expect("provider key")),
            session_token: Arc::new(SecretValue::new(SESSION_TOKEN).expect("session token")),
            web_search_enabled: true,
            auto_mode_traces: false,
        },
    )
    .expect("bridge should start");

    TestServers {
        bridge,
        upstream_task,
        state,
    }
}

async fn fake_chat_completions(
    State(state): State<FakeNanState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    if headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        != Some("Bearer nan-test-key")
    {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    state
        .requests
        .lock()
        .expect("request lock")
        .push(body.clone());
    if body["stream"] == true {
        return streaming_chat_response(&body);
    }

    let mut message = json!({"role": "assistant", "content": "hello from NaN"});
    if body.get("chat_template_kwargs").is_some() || body.get("reasoning_effort").is_some() {
        message["reasoning_content"] = json!("I should answer carefully.");
    }
    Json(json!({
        "id": "chat_response",
        "model": "qwen3.6",
        "choices": [{
            "message": message,
            "finish_reason": "stop"
        }],
        "usage": {"prompt_tokens": 5, "completion_tokens": 4, "completion_tokens_details":{"reasoning_tokens":2}}
    }))
    .into_response()
}

fn streaming_chat_response(body: &Value) -> Response {
    let reasoning = if body.get("chat_template_kwargs").is_some()
        || body.get("reasoning_effort").is_some()
    {
        "data: {\"id\":\"chat_stream\",\"choices\":[{\"index\":0,\"delta\":{\"reasoning_content\":\"I should inspect the file.\"}}]}\n\n"
    } else {
        ""
    };
    let stream = format!(
        "{reasoning}{}",
        concat!(
            "data: {\"id\":\"chat_stream\",\"model\":\"qwen3.6\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"I will read it.\"}}]}\n\n",
            "data: {\"id\":\"chat_stream\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"function\":{\"name\":\"Read\",\"arguments\":\"{\\\"file_path\\\":\"}}]}}]}\n\n",
            "data: {\"id\":\"chat_stream\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"README.md\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: {\"id\":\"chat_stream\",\"choices\":[],\"usage\":{\"prompt_tokens\":30,\"completion_tokens\":12,\"completion_tokens_details\":{\"reasoning_tokens\":6}}}\n\n",
            "data: [DONE]\n\n"
        )
    );
    ([(header::CONTENT_TYPE, "text/event-stream")], stream).into_response()
}

async fn fake_web_search(
    State(state): State<FakeNanState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    if headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        != Some("Bearer nan-test-key")
    {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    state.requests.lock().expect("request lock").push(body);
    Json(json!({
        "cached": false,
        "results": [{
            "title": "Tokio project",
            "url": "https://tokio.rs",
            "snippet": "Async runtime for Rust",
            "source": "primary"
        }]
    }))
    .into_response()
}
