use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use nan_harness_bridge::{
    BridgeDiagnosticReason, BridgeModelPolicy, BridgeReasoningRequest, CodexModelCatalog,
    ModelUsageSnapshot, ProviderUsageSnapshot, ResponsesBridgeConfig, RunningBridge,
};
use nan_harness_core::{SecretValue, known_coding_model};
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU8, Ordering},
};
use tokio::net::TcpListener;

#[derive(Clone, Default)]
struct FakeNanState {
    chat_requests: Arc<Mutex<Vec<Value>>>,
    search_requests: Arc<Mutex<Vec<Value>>>,
    /// Total upstream chat attempts, including transient failures.
    chat_attempts: Arc<AtomicU8>,
    /// Number of remaining transient 503 failures to inject before success.
    transient_faults: Arc<AtomicU8>,
}

struct TestServers {
    bridge: RunningBridge,
    upstream_task: tokio::task::JoinHandle<()>,
    state: FakeNanState,
}

#[test]
fn codex_catalog_reports_exact_reasoning_picker_contracts_in_stable_order() {
    let catalog = CodexModelCatalog::from_provider_ids(
        [
            "glm5.2",
            "gemma4",
            "mimo-v2.5",
            "deepseek-v4-flash",
            "qwen3.6",
        ]
        .into_iter()
        .map(str::to_owned),
        "qwen3.6",
    )
    .expect("catalog should build");
    let response = catalog.api_response();
    let models = response["models"].as_array().expect("models list");
    let values = |index: usize| {
        (
            models[index]["slug"].clone(),
            models[index]["default_reasoning_level"].clone(),
            models[index]["supported_reasoning_levels"]
                .as_array()
                .expect("reasoning levels")
                .iter()
                .map(|level| level["effort"].clone())
                .collect::<Vec<_>>(),
        )
    };
    assert_eq!(
        values(0),
        (
            json!("qwen3.6"),
            json!("high"),
            vec![json!("none"), json!("high")]
        )
    );
    assert_eq!(
        values(1),
        (
            json!("deepseek-v4-flash"),
            json!("medium"),
            vec![json!("low"), json!("medium"), json!("high")]
        )
    );
    assert_eq!(
        values(2),
        (json!("mimo-v2.5"), json!("high"), vec![json!("high")])
    );
    assert_eq!(
        values(3),
        (
            json!("gemma4"),
            json!("none"),
            vec![json!("none"), json!("high")]
        )
    );
    assert_eq!(
        values(4),
        (
            json!("glm5.2"),
            json!("medium"),
            vec![json!("low"), json!("medium"), json!("high")]
        )
    );
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
async fn responses_bridge_translates_namespaced_and_freeform_tools() {
    let servers = start_servers().await;
    let client = reqwest::Client::new();
    let endpoint = format!("{}/v1/responses", servers.bridge.base_url());
    let request = responses_request();

    let unauthorized = client
        .post(&endpoint)
        .json(&request)
        .send()
        .await
        .expect("unauthorized request should complete");
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

    let response = client
        .post(endpoint)
        .bearer_auth("local-session-token")
        .json(&request)
        .send()
        .await
        .expect("authenticated request should complete");
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.text().await.expect("stream should be readable");
    assert!(body.contains("response.created"));
    assert!(body.contains("response.output_item.added"));
    assert!(body.contains("response.content_part.added"));
    assert!(body.contains("Working"));
    assert!(body.contains("response.reasoning_summary_text.delta"));
    assert!(body.contains("Inspect before editing"));
    assert!(body.contains("response.output_text.done"));
    assert!(body.contains("response.content_part.done"));
    assert!(body.contains(r#""namespace":"web""#));
    assert!(body.contains(r#""name":"run""#));
    assert!(body.contains(r#""type":"custom_tool_call""#));
    assert!(body.contains("*** Begin Patch"));
    assert!(body.contains("response.completed"));
    assert_eq!(
        servers.bridge.usage(),
        ProviderUsageSnapshot {
            models: std::collections::BTreeMap::from([(
                "qwen3.6".to_owned(),
                ModelUsageSnapshot {
                    responses_with_usage: 1,
                    input_tokens: 10,
                    output_tokens: 5,
                    reasoning_tokens: 4,
                    ..ModelUsageSnapshot::default()
                },
            )]),
        }
    );

    {
        let requests = servers
            .state
            .chat_requests
            .lock()
            .expect("chat request lock");
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0]["model"], "qwen3.6");
        assert_eq!(requests[0]["chat_template_kwargs"]["enable_thinking"], true);
        assert_eq!(requests[0]["tools"][0]["function"]["name"], "web__run");
        assert_eq!(requests[0]["tools"][1]["function"]["name"], "apply_patch");
    }
    servers.shutdown().await;
}

#[tokio::test]
async fn responses_bridge_serves_codex_metadata_and_standalone_search() {
    let servers = start_servers().await;
    let client = reqwest::Client::new();
    let models = client
        .get(format!("{}/v1/models", servers.bridge.base_url()))
        .bearer_auth("local-session-token")
        .send()
        .await
        .expect("models request should complete");
    assert_eq!(models.status(), StatusCode::OK);
    let models: Value = models.json().await.expect("models should be JSON");
    assert_eq!(models["models"][0]["slug"], "qwen3.6");
    assert_eq!(models["models"][1]["slug"], "mimo-v2.5");
    for model in models["models"]
        .as_array()
        .expect("models should be a list")
    {
        let id = model["slug"].as_str().expect("slug should be text");
        assert_eq!(
            model["description"],
            known_coding_model(id)
                .expect("catalog models need shared metadata")
                .description
        );
    }
    assert_eq!(models["models"][0]["shell_type"], "shell_command");
    assert_eq!(models["models"][0]["multi_agent_version"], "v1");

    let response = client
        .post(format!("{}/v1/alpha/search", servers.bridge.base_url()))
        .bearer_auth("local-session-token")
        .json(&json!({
            "id": "session-1",
            "model": "qwen3.6",
            "commands": {"search_query": [{"q": "Rust async runtime"}]},
            "settings": {"filters": {"allowed_domains": ["example.test"]}}
        }))
        .send()
        .await
        .expect("search request should complete");
    assert_eq!(response.status(), StatusCode::OK);
    let response: Value = response.json().await.expect("search should be JSON");
    assert_eq!(response["results"][0]["ref_id"], "turn0search0");
    assert!(
        response["output"]
            .as_str()
            .is_some_and(|output| output.contains("Rust async runtime"))
    );
    assert_eq!(
        servers
            .state
            .search_requests
            .lock()
            .expect("search request lock")[0]["query"],
        "Rust async runtime"
    );
    servers.shutdown().await;
}

#[tokio::test]
async fn responses_bridge_keeps_models_available_when_search_is_disabled() {
    let servers = start_servers_with_search(false).await;
    let client = reqwest::Client::new();
    let models = client
        .get(format!("{}/v1/models", servers.bridge.base_url()))
        .bearer_auth("local-session-token")
        .send()
        .await
        .expect("models request should complete");
    assert_eq!(models.status(), StatusCode::OK);

    for path in ["/v1/alpha/search", "/v1/search"] {
        let response = client
            .post(format!("{}{path}", servers.bridge.base_url()))
            .bearer_auth("local-session-token")
            .json(&json!({}))
            .send()
            .await
            .expect("disabled search request should complete");
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
    }
    assert!(
        servers
            .state
            .search_requests
            .lock()
            .expect("search request lock")
            .is_empty()
    );
    servers.shutdown().await;
}

#[tokio::test]
async fn responses_bridge_routes_each_selected_catalog_model() {
    let servers = start_servers().await;
    let client = reqwest::Client::new();
    let endpoint = format!("{}/v1/responses", servers.bridge.base_url());
    let mut request = responses_request();
    request["model"] = json!("mimo-v2.5");

    let response = client
        .post(endpoint)
        .bearer_auth("local-session-token")
        .json(&request)
        .send()
        .await
        .expect("request should complete");
    assert_eq!(response.status(), StatusCode::OK);
    let _body = response.text().await.expect("stream should be readable");

    assert_eq!(
        servers
            .state
            .chat_requests
            .lock()
            .expect("chat request lock")[0]["model"],
        "mimo-v2.5"
    );
    servers.shutdown().await;
}

#[tokio::test]
async fn responses_bridge_accepts_codex_plan_reasoning_for_always_on_models() {
    let servers = start_servers().await;
    let client = reqwest::Client::new();
    let mut request = responses_request();
    request["model"] = json!("mimo-v2.5");
    request["reasoning"]["effort"] = json!("medium");
    let response = client
        .post(format!("{}/v1/responses", servers.bridge.base_url()))
        .bearer_auth("local-session-token")
        .json(&request)
        .send()
        .await
        .expect("request should complete");
    assert_eq!(response.status(), StatusCode::OK);
    let _body = response.text().await.expect("stream should be readable");

    {
        let requests = servers.state.chat_requests.lock().expect("request lock");
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0]["model"], "mimo-v2.5");
        assert!(requests[0].get("reasoning_effort").is_none());
        assert!(requests[0].get("chat_template_kwargs").is_none());
    }
    servers.shutdown().await;
}

#[tokio::test]
async fn responses_bridge_rejects_disabling_always_on_reasoning_before_upstream() {
    let mut servers = start_servers().await;
    let mut diagnostics = servers.bridge.take_diagnostics();
    let client = reqwest::Client::new();
    let mut request = responses_request();
    request["model"] = json!("mimo-v2.5");
    request["reasoning"]["effort"] = json!("none");
    let response = client
        .post(format!("{}/v1/responses", servers.bridge.base_url()))
        .bearer_auth("local-session-token")
        .json(&request)
        .send()
        .await
        .expect("request should complete");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = response.text().await.expect("error body");
    assert!(body.contains("incompatible with model policy"));
    let diagnostic = diagnostics
        .recv()
        .await
        .expect("diagnostic should be emitted");
    assert_eq!(
        diagnostic.reason,
        BridgeDiagnosticReason::ReasoningPolicyMismatch
    );
    assert_eq!(diagnostic.model_id.as_deref(), Some("mimo-v2.5"));
    assert_eq!(
        diagnostic.requested_reasoning,
        Some(BridgeReasoningRequest::None)
    );
    assert_eq!(diagnostic.model_policy, Some(BridgeModelPolicy::AlwaysOn));
    assert!(
        servers
            .state
            .chat_requests
            .lock()
            .expect("request lock")
            .is_empty()
    );
    servers.shutdown().await;
}

fn responses_request() -> Value {
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

async fn start_servers() -> TestServers {
    start_servers_with_search(true).await
}

async fn start_servers_with_search(web_search_enabled: bool) -> TestServers {
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
    state.chat_attempts.fetch_add(1, Ordering::Relaxed);
    if state.transient_faults.load(Ordering::Relaxed) > 0 {
        state.transient_faults.fetch_sub(1, Ordering::Relaxed);
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    state
        .chat_requests
        .lock()
        .expect("chat request lock")
        .push(body);
    let patch_arguments = json!({"input": "*** Begin Patch"}).to_string();
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

#[tokio::test]
async fn responses_bridge_retries_transient_upstream_gateway_errors() {
    let servers = start_servers().await;
    servers.state.transient_faults.store(2, Ordering::Relaxed);

    let response = reqwest::Client::new()
        .post(format!("{}/v1/responses", servers.bridge.base_url()))
        .bearer_auth("local-session-token")
        .json(&responses_request())
        .send()
        .await
        .expect("request should succeed after the bridge retries transient 503s");
    assert_eq!(response.status(), StatusCode::OK);
    let _body = response.text().await.expect("stream should be readable");

    assert_eq!(
        servers.state.chat_attempts.load(Ordering::Relaxed),
        3,
        "the two injected 503s plus one success should be attempted"
    );
    assert_eq!(
        servers.state.transient_faults.load(Ordering::Relaxed),
        0,
        "all injected faults should be consumed"
    );
    assert_eq!(
        servers
            .state
            .chat_requests
            .lock()
            .expect("chat request lock")
            .len(),
        1,
        "only a successful (non-fault) upstream request should be recorded"
    );
    servers.shutdown().await;
}

#[tokio::test]
async fn responses_bridge_exposes_upstream_failures_as_diagnostics() {
    let mut servers = start_servers().await;
    // Keep the upstream failing so the bridge exhausts its retries and
    // surfaces the gateway error to the harness call site.
    servers
        .state
        .transient_faults
        .store(u8::MAX, Ordering::Relaxed);
    let mut diagnostics_rx = servers.bridge.take_diagnostics();

    let response = reqwest::Client::new()
        .post(format!("{}/v1/responses", servers.bridge.base_url()))
        .bearer_auth("local-session-token")
        .json(&responses_request())
        .send()
        .await
        .expect("request should complete with a gateway error");
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);

    let diagnostic = diagnostics_rx
        .recv()
        .await
        .expect("bridge should publish a diagnostic");
    assert_eq!(diagnostic.code, "NH-BRIDGE-104");
    assert_eq!(diagnostic.http_status, Some(503));
    assert_eq!(
        diagnostic.endpoint,
        nan_harness_bridge::BridgeEndpoint::Responses
    );
    servers.shutdown().await;
}

#[tokio::test]
async fn responses_bridge_queues_multiple_diagnostics_without_overwriting_them() {
    let mut servers = start_servers().await;
    let mut diagnostics_rx = servers.bridge.take_diagnostics();
    let endpoint = format!("{}/v1/responses", servers.bridge.base_url());
    let client = reqwest::Client::new();

    let unauthorized = client
        .post(&endpoint)
        .json(&responses_request())
        .send()
        .await
        .expect("unauthorized request should complete");
    let invalid = client
        .post(&endpoint)
        .bearer_auth("local-session-token")
        .body("{")
        .send()
        .await
        .expect("invalid request should complete");

    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
    let first = diagnostics_rx
        .recv()
        .await
        .expect("first diagnostic should be queued");
    let second = diagnostics_rx
        .recv()
        .await
        .expect("second diagnostic should be queued");
    assert_eq!(
        [first.code, second.code],
        ["NH-BRIDGE-101", "NH-BRIDGE-102"]
    );
    assert_eq!([first.http_status, second.http_status], [None, None]);
    servers.shutdown().await;
}
