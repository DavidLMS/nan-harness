use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use nan_harness_bridge::{FxGatewayConfig, FxModelCatalog, RunningBridge};
use nan_harness_core::SecretValue;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;

#[derive(Clone, Default)]
struct FakeNanState {
    requests: Arc<Mutex<Vec<Value>>>,
}

struct TestServers {
    bridge: RunningBridge,
    upstream_task: tokio::task::JoinHandle<()>,
    state: FakeNanState,
}

#[tokio::test]
async fn fx_gateway_translates_catalog_reasoning_tools_and_streaming() {
    let servers = start_servers().await;
    let client = reqwest::Client::new();
    let models = client
        .get(format!(
            "{}/coding-agent/v1/models",
            servers.bridge.base_url()
        ))
        .bearer_auth("local-session-token")
        .send()
        .await
        .expect("catalog request should complete");
    assert_eq!(models.status(), StatusCode::OK);
    let models: Value = models.json().await.expect("catalog should be JSON");
    assert_eq!(models["data"][0]["id"], "qwen3.6");
    assert_eq!(
        models["data"][0]["reasoning_options"][0]["values"][1],
        "high"
    );

    let response = client
        .post(format!(
            "{}/v3/ai/language-model",
            servers.bridge.base_url()
        ))
        .bearer_auth("local-session-token")
        .header("ai-language-model-id", "qwen3.6")
        .json(&json!({
            "prompt": [
                {"role":"system","content":"You are an agent."},
                {"role":"user","content":[{"type":"text","text":"Inspect this."}]}
            ],
            "tools": [{
                "type":"function",
                "name":"read_file",
                "description":"Read a file",
                "inputSchema":{"type":"object","properties":{"path":{"type":"string"}}}
            }],
            "toolChoice":{"type":"auto"},
            "maxOutputTokens":1024,
            "reasoning":"high"
        }))
        .send()
        .await
        .expect("chat request should complete");
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.text().await.expect("stream should be readable");
    assert!(body.contains("response-metadata"));
    assert!(body.contains("reasoning-delta"));
    assert!(body.contains("text-delta"));
    assert!(body.contains("tool-call"));
    assert!(body.contains("finish"));

    {
        let requests = servers.state.requests.lock().expect("request lock");
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0]["model"], "qwen3.6");
        assert_eq!(requests[0]["chat_template_kwargs"]["enable_thinking"], true);
        assert_eq!(requests[0]["tools"][0]["function"]["name"], "read_file");
        assert_eq!(requests[0]["max_tokens"], 1024);
    }
    servers.shutdown().await;
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
        .with_state(state.clone());
    let upstream_task = tokio::spawn(async move {
        axum::serve(upstream_listener, app)
            .await
            .expect("upstream should serve");
    });

    let bridge_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bridge should bind");
    let bridge = nan_harness_bridge::spawn_fx_gateway(
        bridge_listener,
        FxGatewayConfig {
            provider_base_url: format!("http://{upstream_address}/v1"),
            models: FxModelCatalog::from_provider_ids(["qwen3.6".to_owned()])
                .expect("model catalog should build"),
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
    state.requests.lock().expect("request lock").push(body);
    let chunks = [
        json!({"id":"chatcmpl_fx","choices":[{"delta":{"reasoning_content":"Inspect first"}}]}),
        json!({"id":"chatcmpl_fx","choices":[{"delta":{"content":"Done"}}]}),
        json!({"id":"chatcmpl_fx","choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"read_file","arguments":"{\"path\":\"README.md\"}"}}]}}]}),
        json!({"id":"chatcmpl_fx","choices":[],"usage":{"prompt_tokens":12,"completion_tokens":8}}),
    ];
    let body = chunks
        .into_iter()
        .map(|chunk| format!("data: {chunk}\n\n"))
        .chain(std::iter::once("data: [DONE]\n\n".to_owned()))
        .collect::<String>();
    ([(header::CONTENT_TYPE, "text/event-stream")], body).into_response()
}
