use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use nan_harness_bridge::{
    FxGatewayConfig, FxModelCatalog, ModelUsageSnapshot, ProviderUsageSnapshot, RunningBridge,
};
use nan_harness_core::SecretValue;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;

#[derive(Clone, Default)]
struct FakeNanState {
    requests: Arc<Mutex<Vec<Value>>>,
    search_requests: Arc<Mutex<Vec<Value>>>,
}

struct TestServers {
    bridge: RunningBridge,
    upstream_task: tokio::task::JoinHandle<()>,
    state: FakeNanState,
}

#[tokio::test]
async fn fx_gateway_translates_catalog_reasoning_tools_and_streaming() {
    let servers = start_servers().await.expect("test servers should start");
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

#[tokio::test]
async fn fx_gateway_executes_provider_search_with_correlated_result() {
    let servers = start_servers().await.expect("test servers should start");
    let response = reqwest::Client::new()
        .post(format!(
            "{}/v3/ai/language-model",
            servers.bridge.base_url()
        ))
        .bearer_auth("local-session-token")
        .header("ai-language-model-id", "qwen3.6")
        .json(&json!({
            "prompt": [{"role":"user","content":[{"type":"text","text":"Find the latest Rust release."}]}],
            "tools": [{
                "type":"provider",
                "id":"gateway.perplexity_search",
                "name":"perplexity_search",
                "args":{"maxResults":5}
            }],
            "toolChoice":{"type":"required"}
        }))
        .send()
        .await
        .expect("provider search request should complete");
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.text().await.expect("stream should be readable");
    assert!(body.contains("\"providerExecuted\":true"), "{body}");
    assert!(body.contains("tool-result"), "{body}");
    assert!(body.contains("\"unified\":\"stop\""), "{body}");

    {
        let searches = servers
            .state
            .search_requests
            .lock()
            .expect("search request lock");
        assert_eq!(searches.len(), 1);
        assert_eq!(searches[0]["query"], "Find the latest Rust release.");
        assert_eq!(searches[0]["count"], 5);
        assert!(searches[0].get("allowed_domains").is_none());
        assert!(searches[0].get("blocked_domains").is_none());
    }
    servers.shutdown().await;
}

#[tokio::test]
async fn fx_gateway_routes_auto_review_to_the_selected_nan_model() {
    let servers = start_servers().await.expect("test servers should start");
    let response = reqwest::Client::new()
        .post(format!(
            "{}/v3/ai/language-model",
            servers.bridge.base_url()
        ))
        .bearer_auth("local-session-token")
        .header("ai-language-model-id", "zai/glm-5.2")
        .json(&json!({
            "prompt": [{"role":"user","content":"Review this action."}],
            "tools": [{
                "type":"function",
                "name":"permission_decision",
                "inputSchema":{"type":"object"}
            }],
            "toolChoice":{"type":"required"},
            "maxOutputTokens":2048
        }))
        .send()
        .await
        .expect("auto review request should complete");
    assert_eq!(response.status(), StatusCode::OK);
    let body = response
        .text()
        .await
        .expect("review stream should be readable");
    assert!(body.contains("permission_decision"), "{body}");
    assert_eq!(
        servers.bridge.usage(),
        ProviderUsageSnapshot {
            models: std::collections::BTreeMap::from([(
                "qwen3.6".to_owned(),
                ModelUsageSnapshot {
                    responses_with_usage: 1,
                    input_tokens: 12,
                    output_tokens: 8,
                    reasoning_tokens: 3,
                    ..ModelUsageSnapshot::default()
                },
            )]),
        }
    );

    {
        let requests = servers.state.requests.lock().expect("request lock");
        let review = requests.last().expect("review request should be captured");
        assert_eq!(review["model"], "qwen3.6");
        assert_eq!(review["tool_choice"], "required");
        assert_eq!(
            review["tools"][0]["function"]["name"],
            "permission_decision"
        );
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

impl Drop for TestServers {
    fn drop(&mut self) {
        self.bridge.shutdown();
        self.upstream_task.abort();
    }
}

async fn bind_loopback(label: &str) -> Result<TcpListener, String> {
    TcpListener::bind("127.0.0.1:0").await.map_err(|error| {
        format!(
            "{label} listener could not bind 127.0.0.1:0: {error} (kind: {:?}); the test runner must allow loopback sockets",
            error.kind()
        )
    })
}

async fn start_servers() -> Result<TestServers, String> {
    let upstream_listener = bind_loopback("upstream").await?;
    let upstream_address = upstream_listener
        .local_addr()
        .map_err(|error| format!("upstream address should be available: {error}"))?;
    let bridge_listener = bind_loopback("bridge").await?;
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

    let bridge = nan_harness_bridge::spawn_fx_gateway(
        bridge_listener,
        FxGatewayConfig {
            launch_id: "fx_test".to_owned(),
            provider_base_url: format!("http://{upstream_address}/v1"),
            models: FxModelCatalog::from_provider_ids(["qwen3.6".to_owned()])
                .expect("model catalog should build"),
            selected_model_id: "qwen3.6".to_owned(),
            provider_api_key: Arc::new(SecretValue::new("provider-key").expect("valid key")),
            session_token: Arc::new(SecretValue::new("local-session-token").expect("valid token")),
            web_search_enabled: true,
        },
    )
    .map_err(|error| {
        upstream_task.abort();
        format!("bridge should start: {error}")
    })?;
    Ok(TestServers {
        bridge,
        upstream_task,
        state,
    })
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
        .requests
        .lock()
        .expect("request lock")
        .push(body.clone());
    let tool_name = body
        .pointer("/tools/0/function/name")
        .and_then(Value::as_str)
        .unwrap_or("read_file");
    let arguments = match tool_name {
        "permission_decision" => {
            "{\"risk\":\"low\",\"authorization\":\"high\",\"decision\":\"allow\",\"rationale\":\"Routine local action.\"}"
        }
        "perplexity_search" => "{\"query\":\"Find the latest Rust release.\"}",
        _ => "{\"path\":\"README.md\"}",
    };
    let mut chunks = vec![json!({
        "id":"chatcmpl_fx",
        "choices":[{"delta":{"reasoning_content":"Inspect first"}}]
    })];
    if tool_name != "permission_decision" {
        chunks.push(json!({"id":"chatcmpl_fx","choices":[{"delta":{"content":"Done"}}]}));
    }
    chunks.push(json!({
        "id":"chatcmpl_fx",
        "choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":tool_name,"arguments":arguments}}]},"finish_reason":"tool_calls"}]
    }));
    chunks.push(
        json!({"id":"chatcmpl_fx","choices":[],"usage":{"prompt_tokens":12,"completion_tokens":8,"completion_tokens_details":{"reasoning_tokens":3}}}),
    );
    let body = chunks
        .into_iter()
        .map(|chunk| format!("data: {chunk}\n\n"))
        .chain(std::iter::once("data: [DONE]\n\n".to_owned()))
        .collect::<String>();
    ([(header::CONTENT_TYPE, "text/event-stream")], body).into_response()
}

async fn search(
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
        .search_requests
        .lock()
        .expect("search request lock")
        .push(body);
    Json(json!({
        "results": [{
            "title": "Rust release",
            "url": "https://www.rust-lang.org/",
            "snippet": "The latest Rust release."
        }]
    }))
    .into_response()
}
