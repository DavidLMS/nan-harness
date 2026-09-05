use axum::http::StatusCode;
use serde_json::{Value, json};

use crate::support::{post_messages, start_servers};

#[tokio::test]
async fn bridge_tunes_both_native_auto_classifier_stages_for_qwen() {
    let servers = start_servers().await;
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
            let response = post_messages(
                &servers,
                "/v1/messages?beta=true",
                &json!({
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
                }),
            )
            .await;
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
    let response = post_messages(
        &servers,
        "/v1/messages",
        &json!({
            "model": "opus",
            "max_tokens": 64,
            "system": "An unknown classifier policy",
            "messages": [{
                "role": "user",
                "content": "Stage 1 does NOT apply user intent or ALLOW exceptions"
            }]
        }),
    )
    .await;

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
