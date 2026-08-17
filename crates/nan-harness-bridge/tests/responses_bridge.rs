use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use nan_harness_bridge::{ResponsesBridgeConfig, RunningBridge};
use nan_harness_core::SecretValue;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;

#[derive(Clone, Default)]
struct FakeNanState {
    chat_requests: Arc<Mutex<Vec<Value>>>,
    search_requests: Arc<Mutex<Vec<Value>>>,
}

struct TestServers {
    bridge: RunningBridge,
    upstream_task: tokio::task::JoinHandle<()>,
    state: FakeNanState,
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
    assert!(body.contains("Working"));
    assert!(body.contains(r#""namespace":"web""#));
    assert!(body.contains(r#""name":"run""#));
    assert!(body.contains(r#""type":"custom_tool_call""#));
    assert!(body.contains("*** Begin Patch"));
    assert!(body.contains("response.completed"));

    {
        let requests = servers
            .state
            .chat_requests
            .lock()
            .expect("chat request lock");
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0]["model"], "qwen3.6");
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
        "store": false,
        "stream": true,
        "include": []
    })
}

async fn start_servers() -> TestServers {
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
            provider_base_url: format!("http://{upstream_address}/v1"),
            provider_model: "qwen3.6".to_owned(),
            provider_api_key: Arc::new(SecretValue::new("provider-key").expect("valid key")),
            session_token: Arc::new(SecretValue::new("local-session-token").expect("valid token")),
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
    state
        .chat_requests
        .lock()
        .expect("chat request lock")
        .push(body);
    let patch_arguments = json!({"input": "*** Begin Patch"}).to_string();
    let chunks = [
        json!({"id":"chatcmpl_test","choices":[{"delta":{"content":"Working"}}]}).to_string(),
        json!({"id":"chatcmpl_test","choices":[{"delta":{"tool_calls":[
            {"index":0,"id":"call_web","function":{"name":"web__run","arguments":"{}"}},
            {"index":1,"id":"call_patch","function":{"name":"apply_patch","arguments":patch_arguments}}
        ]}}]}).to_string(),
        json!({"id":"chatcmpl_test","choices":[],"usage":{"prompt_tokens":10,"completion_tokens":5}}).to_string(),
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
