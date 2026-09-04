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

#[derive(Clone, Default)]
struct FakeNanState {
    requests: Arc<Mutex<Vec<Value>>>,
}

struct TestServers {
    bridge: RunningBridge,
    upstream_task: JoinHandle<()>,
    state: FakeNanState,
}

fn usage_for(
    models: impl IntoIterator<Item = (&'static str, ModelUsageSnapshot)>,
) -> ProviderUsageSnapshot {
    ProviderUsageSnapshot {
        models: models
            .into_iter()
            .map(|(model, usage)| (model.to_owned(), usage))
            .collect::<BTreeMap<_, _>>(),
    }
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
async fn bridge_authenticates_locally_and_translates_non_streaming_messages() {
    let servers = start_servers().await;
    let client = reqwest::Client::new();
    let endpoint = format!("{}/v1/messages?beta=true", servers.bridge.base_url());
    let request = json!({
        "model": "anthropic/nan/qwen3.6",
        "max_tokens": 100_000,
        "messages": [{"role": "user", "content": "hello"}]
    });

    let unauthorized = client
        .post(&endpoint)
        .json(&request)
        .send()
        .await
        .expect("local request should complete");
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
    let error: Value = unauthorized.json().await.expect("error should be JSON");
    assert_eq!(error["error"]["type"], "authentication_error");

    let response = client
        .post(endpoint)
        .bearer_auth("local-session-token")
        .json(&request)
        .send()
        .await
        .expect("authenticated request should complete");
    assert_eq!(response.status(), StatusCode::OK);
    let response: Value = response.json().await.expect("response should be JSON");
    assert_eq!(response["type"], "message");
    assert_eq!(response["content"][0]["text"], "hello from NaN");

    {
        let requests = servers.state.requests.lock().expect("request lock");
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0]["model"], "qwen3.6");
        assert_eq!(requests[0]["max_tokens"], 65_536);
    }
    assert_eq!(
        servers.bridge.usage(),
        usage_for([(
            "qwen3.6",
            ModelUsageSnapshot {
                responses_with_usage: 1,
                input_tokens: 5,
                output_tokens: 4,
                reasoning_tokens: 2,
                ..ModelUsageSnapshot::default()
            },
        )])
    );
    servers.shutdown().await;
}

#[tokio::test]
async fn bridge_preserves_images_inside_tool_results() {
    let servers = start_servers().await;
    let response = reqwest::Client::new()
        .post(format!("{}/v1/messages", servers.bridge.base_url()))
        .bearer_auth("local-session-token")
        .json(&json!({
            "model": "anthropic/nan/qwen3.6",
            "max_tokens": 1_024,
            "tools": [{
                "name": "screenshot",
                "description": "Capture the current screen",
                "input_schema": {"type": "object", "properties": {}}
            }],
            "messages": [
                {"role": "user", "content": "Inspect the screen"},
                {"role": "assistant", "content": [{
                    "type": "tool_use",
                    "id": "tool_screenshot_1",
                    "name": "screenshot",
                    "input": {}
                }]},
                {"role": "user", "content": [{
                    "type": "tool_result",
                    "tool_use_id": "tool_screenshot_1",
                    "content": [
                        {"type": "text", "text": "Screenshot captured"},
                        {"type": "image", "source": {
                            "type": "base64",
                            "media_type": "image/png",
                            "data": "AA=="
                        }}
                    ]
                }]}
            ]
        }))
        .send()
        .await
        .expect("image tool result request should complete");

    assert_eq!(response.status(), StatusCode::OK);
    {
        let requests = servers.state.requests.lock().expect("request lock");
        let tool_result = &requests[0]["messages"][2];
        assert_eq!(tool_result["role"], "tool");
        assert_eq!(tool_result["tool_call_id"], "tool_screenshot_1");
        assert_eq!(tool_result["content"][0]["type"], "text");
        assert_eq!(tool_result["content"][0]["text"], "Screenshot captured");
        assert_eq!(tool_result["content"][1]["type"], "image_url");
        assert_eq!(
            tool_result["content"][1]["image_url"]["url"],
            "data:image/png;base64,AA=="
        );
    }
    servers.shutdown().await;
}

#[tokio::test]
async fn bridge_translates_anthropic_thinking_controls_without_changing_defaults() {
    let servers = start_servers().await;
    let client = reqwest::Client::new();
    let endpoint = format!("{}/v1/messages", servers.bridge.base_url());
    for (model, thinking, output_config, expected_key, expected_value) in [
        (
            "anthropic/nan/qwen3.6",
            json!({"type":"disabled"}),
            Value::Null,
            "chat_template_kwargs",
            json!({"enable_thinking":false}),
        ),
        (
            "anthropic/nan/qwen3.6",
            json!({"type":"enabled","budget_tokens":1024}),
            Value::Null,
            "chat_template_kwargs",
            json!({"enable_thinking":true}),
        ),
        (
            "anthropic/nan/deepseek-v4-flash",
            json!({"type":"adaptive"}),
            json!({"effort":"high"}),
            "reasoning_effort",
            json!("high"),
        ),
        (
            "anthropic/nan/qwen3.6",
            json!({"type":"adaptive"}),
            json!({"effort":"high"}),
            "chat_template_kwargs",
            json!({"enable_thinking":true}),
        ),
    ] {
        let mut request = json!({
            "model": model, "max_tokens": 2048,
            "messages": [{"role":"user","content":"think"}],
            "thinking": thinking
        });
        if !output_config.is_null() {
            request["output_config"] = output_config;
        }
        let response = client
            .post(&endpoint)
            .bearer_auth("local-session-token")
            .json(&request)
            .send()
            .await
            .expect("thinking request should complete");
        assert_eq!(response.status(), StatusCode::OK);
        let response: Value = response.json().await.expect("response JSON");
        assert_eq!(response["content"][0]["type"], "thinking");
        let requests = servers.state.requests.lock().expect("request lock");
        assert_eq!(
            requests.last().expect("upstream request")[expected_key],
            expected_value
        );
    }

    let response = client
        .post(&endpoint)
        .bearer_auth("local-session-token")
        .json(&json!({
            "model":"anthropic/nan/qwen3.6", "max_tokens":128,
            "messages":[{"role":"user","content":"default"}]
        }))
        .send()
        .await
        .expect("default request");
    assert_eq!(response.status(), StatusCode::OK);
    {
        let requests = servers.state.requests.lock().expect("request lock");
        let default_request = requests.last().expect("default upstream request");
        assert!(default_request.get("chat_template_kwargs").is_none());
        assert!(default_request.get("reasoning_effort").is_none());
    }
    assert_eq!(
        servers.bridge.usage(),
        usage_for([
            (
                "qwen3.6",
                ModelUsageSnapshot {
                    responses_with_usage: 4,
                    input_tokens: 20,
                    output_tokens: 16,
                    reasoning_tokens: 8,
                    ..ModelUsageSnapshot::default()
                },
            ),
            (
                "deepseek-v4-flash",
                ModelUsageSnapshot {
                    responses_with_usage: 1,
                    input_tokens: 5,
                    output_tokens: 4,
                    reasoning_tokens: 2,
                    ..ModelUsageSnapshot::default()
                },
            ),
        ])
    );
    servers.shutdown().await;
}

#[tokio::test]
async fn bridge_rejects_impossible_thinking_controls() {
    let servers = start_servers().await;
    let response = reqwest::Client::new()
        .post(format!("{}/v1/messages", servers.bridge.base_url()))
        .bearer_auth("local-session-token")
        .json(&json!({
            "model":"anthropic/nan/deepseek-v4-flash", "max_tokens":128,
            "thinking":{"type":"disabled"},
            "messages":[{"role":"user","content":"hello"}]
        }))
        .send()
        .await
        .expect("rejection response");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    servers.shutdown().await;
}

#[tokio::test]
async fn bridge_tunes_both_native_auto_classifier_stages_for_qwen() {
    let servers = start_servers().await;
    let client = reqwest::Client::new();
    let endpoint = format!("{}/v1/messages?beta=true", servers.bridge.base_url());

    for model in ["opus", "anthropic/nan/qwen3.6"] {
        for (requested_tokens, stage_marker, expected_tokens) in [
            (
                64,
                "Stage 1 does NOT apply user intent or ALLOW exceptions",
                256,
            ),
            (
                8_192,
                "Review the classification process and follow it carefully",
                8_192,
            ),
        ] {
            let response = client
                .post(&endpoint)
                .bearer_auth("local-session-token")
                .json(&json!({
                    "model": model,
                    "max_tokens": requested_tokens,
                    "temperature": 1,
                    "thinking": {"type": "enabled", "budget_tokens": 1024},
                    "system": [{
                        "type": "text",
                        "text": concat!(
                            "You are a security monitor for autonomous AI coding agents.\n",
                            "## Classification Process\n",
                            "## Output Format"
                        )
                    }],
                    "messages": [{
                        "role": "user",
                        "content": [{"type": "text", "text": stage_marker}]
                    }]
                }))
                .send()
                .await
                .expect("classifier request should complete");
            assert_eq!(response.status(), StatusCode::OK);
            let response: Value = response.json().await.expect("response should be JSON");
            assert_eq!(response["model"], "anthropic/nan/qwen3.6");

            let requests = servers.state.requests.lock().expect("request lock");
            let upstream = requests.last().expect("NaN request should be recorded");
            assert_eq!(upstream["model"], "qwen3.6");
            assert_eq!(upstream["max_tokens"], expected_tokens);
            assert_eq!(upstream["temperature"], 0);
            assert_eq!(upstream["chat_template_kwargs"]["enable_thinking"], false);
        }
    }
    servers.shutdown().await;
}

#[tokio::test]
async fn bridge_fails_closed_for_unknown_auto_classifier_prompts() {
    let servers = start_servers().await;
    let response = reqwest::Client::new()
        .post(format!("{}/v1/messages", servers.bridge.base_url()))
        .bearer_auth("local-session-token")
        .json(&json!({
            "model": "opus",
            "max_tokens": 64,
            "system": "An unknown classifier policy",
            "messages": [{
                "role": "user",
                "content": "Stage 1 does NOT apply user intent or ALLOW exceptions"
            }]
        }))
        .send()
        .await
        .expect("rejected classifier request should complete");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let response: Value = response.json().await.expect("error should be JSON");
    assert!(
        response["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("blocked for safety"))
    );
    assert!(
        servers
            .state
            .requests
            .lock()
            .expect("request lock")
            .is_empty()
    );
    servers.shutdown().await;
}

#[tokio::test]
async fn bridge_keeps_regular_compatibility_alias_requests_untuned() {
    let servers = start_servers().await;
    let response = reqwest::Client::new()
        .post(format!("{}/v1/messages", servers.bridge.base_url()))
        .bearer_auth("local-session-token")
        .json(&json!({
            "model": "opus",
            "max_tokens": 1_024,
            "temperature": 0.7,
            "messages": [{"role": "user", "content": "hello"}]
        }))
        .send()
        .await
        .expect("regular request should complete");

    assert_eq!(response.status(), StatusCode::OK);
    let response: Value = response.json().await.expect("response should be JSON");
    assert_eq!(response["model"], "anthropic/nan/qwen3.6");
    {
        let requests = servers.state.requests.lock().expect("request lock");
        assert_eq!(requests[0]["max_tokens"], 1_024);
        assert_eq!(requests[0]["temperature"], 0.7);
        assert!(requests[0].get("chat_template_kwargs").is_none());
    }
    servers.shutdown().await;
}

#[tokio::test]
async fn count_tokens_needs_no_generation_limit_or_upstream_request() {
    let servers = start_servers().await;
    let client = reqwest::Client::new();
    let hello = client
        .head(format!("{}/api/hello", servers.bridge.base_url()))
        .send()
        .await
        .expect("hello request should complete");
    assert_eq!(hello.status(), StatusCode::NO_CONTENT);

    let response = client
        .post(format!(
            "{}/v1/messages/count_tokens",
            servers.bridge.base_url()
        ))
        .bearer_auth("local-session-token")
        .json(&json!({
            "model": "anthropic/nan/qwen3.6",
            "messages": [{"role": "user", "content": "count this prompt"}]
        }))
        .send()
        .await
        .expect("count request should complete");
    assert_eq!(response.status(), StatusCode::OK);
    let response: Value = response.json().await.expect("count should be JSON");
    assert!(
        response["input_tokens"]
            .as_u64()
            .is_some_and(|count| count > 0)
    );
    assert!(
        servers
            .state
            .requests
            .lock()
            .expect("request lock")
            .is_empty()
    );
    servers.shutdown().await;
}

#[tokio::test]
async fn bridge_streams_text_and_tool_deltas_in_anthropic_order() {
    let servers = start_servers().await;
    let response = reqwest::Client::new()
        .post(format!("{}/v1/messages", servers.bridge.base_url()))
        .bearer_auth("local-session-token")
        .json(&json!({
            "model": "anthropic/nan/qwen3.6",
            "max_tokens": 1024,
            "stream": true,
            "thinking": {"type": "enabled", "budget_tokens": 1024},
            "tools": [{
                "name": "Read",
                "description": "Read a file",
                "input_schema": {"type": "object", "properties": {"file_path": {"type": "string"}}}
            }],
            "messages": [{"role": "user", "content": "Read README.md"}]
        }))
        .send()
        .await
        .expect("streaming request should complete");
    assert_eq!(response.status(), StatusCode::OK);
    let stream = response.text().await.expect("stream should be readable");

    let message_start = stream.find("message_start").expect("message start event");
    let thinking_delta = stream.find("thinking_delta").expect("thinking delta event");
    let text_delta = stream.find("text_delta").expect("text delta event");
    let tool_start = stream.find("tool_use").expect("tool start event");
    let tool_delta = stream.find("input_json_delta").expect("tool delta event");
    let message_stop = stream.rfind("message_stop").expect("message stop event");
    assert!(message_start < thinking_delta);
    assert!(thinking_delta < text_delta);
    assert!(text_delta < tool_start);
    assert!(tool_start < tool_delta);
    assert!(tool_delta < message_stop);
    assert!(stream.contains("README.md"));
    servers.shutdown().await;
}

#[tokio::test]
async fn bridge_executes_claude_code_web_search_through_nan() {
    let servers = start_servers().await;
    let response = reqwest::Client::new()
        .post(format!("{}/v1/messages", servers.bridge.base_url()))
        .bearer_auth("local-session-token")
        .json(&json!({
            "model": "anthropic/nan/qwen3.6",
            "max_tokens": 32_000,
            "stream": true,
            "tools": [{
                "type": "web_search_20250305",
                "name": "web_search",
                "max_uses": 8
            }],
            "tool_choice": {"type": "tool", "name": "web_search"},
            "messages": [{
                "role": "user",
                "content": "Perform a web search for the query: best Rust async runtime 2025"
            }]
        }))
        .send()
        .await
        .expect("web search request should complete");
    assert_eq!(response.status(), StatusCode::OK);
    let stream = response.text().await.expect("stream should be readable");

    assert!(stream.contains("server_tool_use"), "{stream}");
    assert!(stream.contains("web_search_tool_result"), "{stream}");
    assert!(stream.contains("Tokio project"), "{stream}");
    assert!(stream.contains("https://tokio.rs"), "{stream}");
    assert!(stream.contains("Async runtime for Rust"), "{stream}");
    assert!(stream.contains("message_stop"), "{stream}");

    {
        let requests = servers.state.requests.lock().expect("request lock");
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0]["query"], "best Rust async runtime 2025");
        assert_eq!(requests[0]["count"], 8);
        assert_eq!(requests[0]["fetch_content"], false);
    }
    servers.shutdown().await;
}

#[tokio::test]
async fn bridge_lists_only_the_configured_claude_code_models() {
    let servers = start_servers().await;
    let endpoint = format!("{}/v1/models", servers.bridge.base_url());
    let unauthorized = reqwest::Client::new()
        .get(&endpoint)
        .send()
        .await
        .expect("model request should complete");
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

    let response = reqwest::Client::new()
        .get(endpoint)
        .bearer_auth("local-session-token")
        .send()
        .await
        .expect("authorized model request should complete");
    assert_eq!(response.status(), StatusCode::OK);
    let response: Value = response.json().await.expect("model list should be JSON");
    assert_eq!(response["has_more"], false);
    assert_eq!(response["data"].as_array().map(Vec::len), Some(4));
    assert_eq!(response["data"][0]["id"], "anthropic/nan/qwen3.6");
    assert_eq!(response["data"][0]["display_name"], "NaN · Qwen 3.6");
    assert_eq!(response["data"][1]["id"], "anthropic/nan/deepseek-v4-flash");
    assert_eq!(response["data"][2]["id"], "anthropic/nan/mimo-v2.5");
    assert_eq!(response["data"][3]["id"], "anthropic/nan/gemma4");
    servers.shutdown().await;
}

#[tokio::test]
async fn bridge_routes_each_gateway_model_to_its_nan_model() {
    let servers = start_servers().await;
    let response = reqwest::Client::new()
        .post(format!("{}/v1/messages", servers.bridge.base_url()))
        .bearer_auth("local-session-token")
        .json(&json!({
            "model": "anthropic/nan/mimo-v2.5",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "hello"}]
        }))
        .send()
        .await
        .expect("model-routed request should complete");
    assert_eq!(response.status(), StatusCode::OK);
    let response: Value = response.json().await.expect("response should be JSON");
    assert_eq!(response["model"], "anthropic/nan/mimo-v2.5");
    assert_eq!(
        servers.state.requests.lock().expect("request lock")[0]["model"],
        "mimo-v2.5"
    );
    servers.shutdown().await;
}

#[tokio::test]
async fn bridge_rejects_models_outside_the_discovered_catalog() {
    let servers = start_servers().await;
    let response = reqwest::Client::new()
        .post(format!("{}/v1/messages", servers.bridge.base_url()))
        .bearer_auth("local-session-token")
        .json(&json!({
            "model": "anthropic/nan/deepseek-v4-flash-0731",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "hello"}]
        }))
        .send()
        .await
        .expect("rejected model request should complete");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let response: Value = response.json().await.expect("error should be JSON");
    assert_eq!(response["error"]["type"], "invalid_request_error");
    assert!(
        response["error"]["message"]
            .as_str()
            .is_some_and(|message| {
                message.contains("not available through this bridge")
                    && message.contains("NH-BRIDGE-102")
            })
    );
    assert!(
        servers
            .state
            .requests
            .lock()
            .expect("request lock")
            .is_empty()
    );
    servers.shutdown().await;
}

async fn start_servers() -> TestServers {
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
            provider_api_key: Arc::new(SecretValue::new("nan-test-key").expect("provider key")),
            session_token: Arc::new(
                SecretValue::new("local-session-token").expect("session token"),
            ),
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
        return ([(header::CONTENT_TYPE, "text/event-stream")], stream).into_response();
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
