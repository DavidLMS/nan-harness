use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::post,
};
use nan_harness_bridge::{CodexModelCatalog, ResponsesBridgeConfig, RunningBridge};
use nan_harness_core::SecretValue;
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU8, Ordering},
};
use tokio::net::TcpListener;

#[derive(Clone, Default)]
pub(crate) struct FakeNanState {
    pub(crate) chat_requests: Arc<Mutex<Vec<Value>>>,
    pub(crate) chat_headers: Arc<Mutex<Vec<HeaderMap>>>,
    pub(crate) search_requests: Arc<Mutex<Vec<Value>>>,
    /// Total upstream chat attempts, including transient failures.
    pub(crate) chat_attempts: Arc<AtomicU8>,
    /// Number of remaining transient 503 failures to inject before success.
    pub(crate) transient_faults: Arc<AtomicU8>,
    /// Exact send indices on which to inject a transient 503.
    pub(crate) transient_fault_sends: Arc<Mutex<Vec<u8>>>,
    /// Number of reasoning-only successful streams to inject.
    pub(crate) empty_completions: Arc<AtomicU8>,
    /// Zero repeats an ID, one varies it, and two omits it.
    pub(crate) empty_id_mode: Arc<AtomicU8>,
    /// Replays one empty completion until the provider request body changes.
    pub(crate) body_keyed_empty_replay: Arc<AtomicBool>,
    pub(crate) body_keyed_empty_request: Arc<Mutex<Option<Value>>>,
    /// Number of streams to end before their terminal marker.
    pub(crate) truncated_completions: Arc<AtomicU8>,
    pub(crate) truncated_text_completions: Arc<AtomicU8>,
    /// Number of incomplete apply-patch calls to inject before success.
    pub(crate) malformed_patch_completions: Arc<AtomicU8>,
}

pub(crate) struct TestServers {
    pub(crate) bridge: RunningBridge,
    pub(crate) upstream_task: tokio::task::JoinHandle<()>,
    pub(crate) state: FakeNanState,
}

impl TestServers {
    pub(crate) async fn shutdown(mut self) {
        self.bridge.shutdown();
        self.bridge
            .wait()
            .await
            .expect("bridge should stop cleanly");
        self.upstream_task.abort();
    }
}

pub(crate) fn responses_request() -> Value {
    json!({
        "model": "qwen3.6",
        "instructions": "Use tools when needed.",
        "input": [{
            "type": "message",
            "role": "user",
            "content": [{"type": "input_text", "text": "Inspect and edit"}]
        }],
        "tools": [
            {
                "type": "namespace",
                "name": "web",
                "description": "Web tools",
                "tools": [{
                    "type": "function",
                    "name": "run",
                    "description": "Search the web",
                    "strict": false,
                    "parameters": {"type": "object", "properties": {}}
                }]
            },
            {
                "type": "custom",
                "name": "apply_patch",
                "description": "Edit files",
                "format": {"type": "grammar", "syntax": "lark", "definition": "start: patch"}
            }
        ],
        "tool_choice": "auto",
        "parallel_tool_calls": true,
        "reasoning": {"effort": "high"},
        "store": false,
        "stream": true,
        "include": []
    })
}

pub(crate) async fn start_servers() -> TestServers {
    start_servers_with_search(true).await
}

pub(crate) async fn start_servers_with_search(web_search_enabled: bool) -> TestServers {
    let upstream_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("upstream should bind");
    let upstream_address = upstream_listener
        .local_addr()
        .expect("upstream address should be available");
    let state = FakeNanState::default();
    let app = Router::new()
        .route("/v1/chat/completions", post(chat_completions))
        .route("/v1/search", post(search))
        .with_state(state.clone());
    let upstream_task = tokio::spawn(async move {
        axum::serve(upstream_listener, app)
            .await
            .expect("upstream should serve");
    });

    let bridge_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bridge should bind");
    let bridge = nan_harness_bridge::spawn_responses(
        bridge_listener,
        ResponsesBridgeConfig {
            launch_id: "responses_test".to_owned(),
            provider_base_url: format!("http://{upstream_address}/v1"),
            models: CodexModelCatalog::from_provider_ids(
                ["qwen3.6".to_owned(), "mimo-v2.5".to_owned()],
                "qwen3.6",
            )
            .expect("model catalog should build"),
            provider_api_key: Arc::new(SecretValue::new("provider-key").expect("valid key")),
            session_token: Arc::new(SecretValue::new("local-session-token").expect("valid token")),
            web_search_enabled,
            session_max_tokens: None,
        },
    )
    .expect("bridge should start");
    TestServers {
        bridge,
        upstream_task,
        state,
    }
}

async fn chat_completions(
    State(state): State<FakeNanState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    if headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        != Some("Bearer provider-key")
    {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let attempt = state.chat_attempts.fetch_add(1, Ordering::Relaxed) + 1;
    if inject_transient_fault(&state, attempt) {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    state
        .chat_requests
        .lock()
        .expect("chat request lock")
        .push(body.clone());
    state
        .chat_headers
        .lock()
        .expect("chat header lock")
        .push(headers);
    let body_keyed_replay = replays_cached_empty(&state, &body);
    if body_keyed_replay || state.empty_completions.load(Ordering::Relaxed) > 0 {
        if !body_keyed_replay {
            state.empty_completions.fetch_sub(1, Ordering::Relaxed);
        }
        let mut chunk = json!({
            "choices": [{
                "delta": {"reasoning_content": "unfinished"},
                "finish_reason": "stop"
            }]
        });
        match state.empty_id_mode.load(Ordering::Relaxed) {
            0 => chunk["id"] = Value::from("chatcmpl_empty"),
            1 => chunk["id"] = Value::from(format!("chatcmpl_empty_{attempt}")),
            _ => {}
        }
        return (
            [(header::CONTENT_TYPE, "text/event-stream")],
            format!("data: {chunk}\n\ndata: [DONE]\n\n"),
        )
            .into_response();
    }
    if state.truncated_completions.load(Ordering::Relaxed) > 0 {
        state.truncated_completions.fetch_sub(1, Ordering::Relaxed);
        return (
            [(header::CONTENT_TYPE, "text/event-stream")],
            "data: {\"id\":\"chatcmpl_truncated\",\"choices\":[{\"delta\":{\"reasoning_content\":\"unfinished\"}}]}\n\n",
        )
            .into_response();
    }
    if state.truncated_text_completions.load(Ordering::Relaxed) > 0 {
        state
            .truncated_text_completions
            .fetch_sub(1, Ordering::Relaxed);
        return (
            [(header::CONTENT_TYPE, "text/event-stream")],
            "data: {\"id\":\"chatcmpl_truncated_text\",\"choices\":[{\"delta\":{\"content\":\"Discard this partial answer\"}}]}\n\n",
        )
            .into_response();
    }
    if state.malformed_patch_completions.load(Ordering::Relaxed) > 0 {
        state
            .malformed_patch_completions
            .fetch_sub(1, Ordering::Relaxed);
        let chunks = [
            json!({"id":format!("chatcmpl_malformed_{attempt}"),"choices":[{"delta":{"content":"Writing a report that must not leak from a discarded attempt."}}]}).to_string(),
            json!({"id":format!("chatcmpl_malformed_{attempt}"),"choices":[{"delta":{"tool_calls":[
                {"index":0,"id":format!("call_malformed_{attempt}"),"function":{"name":"apply_patch","arguments":"{"}}
            ]},"finish_reason":"tool_calls"}]}).to_string(),
        ];
        let stream = chunks
            .into_iter()
            .map(|chunk| format!("data: {chunk}\n\n"))
            .chain(std::iter::once("data: [DONE]\n\n".to_owned()))
            .collect::<String>();
        return ([(header::CONTENT_TYPE, "text/event-stream")], stream).into_response();
    }
    let patch_arguments = json!({"input": "*** Begin Patch\n*** End Patch"}).to_string();
    let chunks = [
        json!({"id":"chatcmpl_test","choices":[{"delta":{"reasoning_content":"Inspect before editing"}}]}).to_string(),
        json!({"id":"chatcmpl_test","choices":[{"delta":{"content":"Working"}}]}).to_string(),
        json!({"id":"chatcmpl_test","choices":[{"delta":{"tool_calls":[
            {"index":0,"id":"call_web","function":{"name":"web__run","arguments":"{}"}},
            {"index":1,"id":"call_patch","function":{"name":"apply_patch","arguments":patch_arguments}}
        ]}}]}).to_string(),
        json!({"id":"chatcmpl_test","choices":[],"usage":{"prompt_tokens":10,"completion_tokens":5,"completion_tokens_details":{"reasoning_tokens":4}}}).to_string(),
    ];
    let stream = chunks
        .into_iter()
        .map(|chunk| format!("data: {chunk}\n\n"))
        .chain(std::iter::once("data: [DONE]\n\n".to_owned()))
        .collect::<String>();
    ([(header::CONTENT_TYPE, "text/event-stream")], stream).into_response()
}

async fn search(State(state): State<FakeNanState>, Json(body): Json<Value>) -> Json<Value> {
    let query = body["query"].as_str().unwrap_or_default().to_owned();
    state
        .search_requests
        .lock()
        .expect("search request lock")
        .push(body);
    Json(json!({
        "results": [{
            "title": query,
            "url": "https://example.test/rust-async",
            "snippet": "A deterministic search result."
        }]
    }))
}

fn replays_cached_empty(state: &FakeNanState, body: &Value) -> bool {
    if !state.body_keyed_empty_replay.load(Ordering::Relaxed) {
        return false;
    }
    let mut cached = state
        .body_keyed_empty_request
        .lock()
        .expect("body-keyed request lock");
    if let Some(original) = cached.as_ref() {
        original == body
    } else {
        *cached = Some(body.clone());
        true
    }
}

fn inject_transient_fault(state: &FakeNanState, attempt: u8) -> bool {
    if state
        .transient_fault_sends
        .lock()
        .expect("transient fault schedule lock")
        .contains(&attempt)
    {
        return true;
    }
    state
        .transient_faults
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |remaining| {
            remaining.checked_sub(1)
        })
        .is_ok()
}

pub(crate) async fn send_budget_request(servers: &TestServers) -> String {
    reqwest::Client::new()
        .post(format!("{}/v1/responses", servers.bridge.base_url()))
        .bearer_auth("local-session-token")
        .json(&responses_request())
        .send()
        .await
        .expect("request should be accepted")
        .text()
        .await
        .expect("stream should be readable")
}
