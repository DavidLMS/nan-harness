use axum::http::StatusCode;
use nan_harness_bridge::ModelUsageSnapshot;
use serde_json::{Value, json};

use crate::support::{post_messages, start_servers, usage_for};

#[tokio::test]
async fn bridge_translates_anthropic_thinking_controls_without_changing_defaults() {
    let servers = start_servers().await;
    for (model, thinking, output_config, expected_key, expected_value) in [
        (
            "anthropic/nan/qwen3.6",
            json!({"type":"disabled"}),
            Value::Null,
            "reasoning_effort",
            json!("none"),
        ),
        (
            "anthropic/nan/qwen3.6",
            json!({"type":"enabled","budget_tokens":1024}),
            Value::Null,
            "reasoning_effort",
            json!("high"),
        ),
        (
            "anthropic/nan/glm5.2",
            json!({"type":"adaptive"}),
            json!({"effort":"high"}),
            "reasoning_effort",
            json!("high"),
        ),
        (
            "anthropic/nan/qwen3.6",
            json!({"type":"adaptive"}),
            json!({"effort":"high"}),
            "reasoning_effort",
            json!("high"),
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
        let response = post_messages(&servers, "/v1/messages", &request).await;
        assert_eq!(response.status(), StatusCode::OK);
        let response: Value = response.json().await.expect("response JSON");
        assert_eq!(response["content"][0]["type"], "thinking");
        let requests = servers.state.requests.lock().expect("request lock");
        assert_eq!(
            requests.last().expect("upstream request")[expected_key],
            expected_value
        );
    }

    let response = post_messages(
        &servers,
        "/v1/messages",
        &json!({
            "model":"anthropic/nan/qwen3.6", "max_tokens":128,
            "messages":[{"role":"user","content":"default"}]
        }),
    )
    .await;
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
                "glm5.2",
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
    let response = post_messages(
        &servers,
        "/v1/messages",
        &json!({
            "model":"anthropic/nan/deepseek-v4-flash", "max_tokens":128,
            "thinking":{"type":"disabled"},
            "messages":[{"role":"user","content":"hello"}]
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    servers.shutdown().await;
}

#[tokio::test]
async fn bridge_preserves_graded_effort_off_and_automatic_defaults() {
    let servers = start_servers().await;
    for model in ["qwen3.6", "gemma4", "glm5.3-flash"] {
        for (thinking, output, expected) in [
            (Value::Null, Value::Null, None),
            (
                json!({"type":"adaptive"}),
                json!({"effort":"low"}),
                Some("low"),
            ),
            (
                json!({"type":"adaptive"}),
                json!({"effort":"medium"}),
                Some("medium"),
            ),
            (
                json!({"type":"adaptive"}),
                json!({"effort":"high"}),
                Some("high"),
            ),
            (
                json!({"type":"adaptive"}),
                json!({"effort":"max"}),
                Some("max"),
            ),
        ] {
            let mut request = json!({
                "model": format!("anthropic/nan/{model}"), "max_tokens": 2048,
                "messages": [{"role":"user","content":"think"}]
            });
            if !thinking.is_null() {
                request["thinking"] = thinking;
            }
            if !output.is_null() {
                request["output_config"] = output;
            }
            let response = post_messages(&servers, "/v1/messages", &request).await;
            assert_eq!(response.status(), StatusCode::OK, "{model}: {expected:?}");
            let requests = servers.state.requests.lock().unwrap();
            let body = requests.last().unwrap();
            assert_eq!(
                body.get("reasoning_effort").and_then(Value::as_str),
                expected
            );
            assert!(body.get("chat_template_kwargs").is_none());
        }
    }
    servers.shutdown().await;
}
