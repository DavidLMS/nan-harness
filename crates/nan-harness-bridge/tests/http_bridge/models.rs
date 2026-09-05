use axum::http::StatusCode;
use serde_json::{Value, json};

use crate::support::{SESSION_TOKEN, post_messages, start_servers};

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
        .bearer_auth(SESSION_TOKEN)
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
    let response = post_messages(
        &servers,
        "/v1/messages",
        &json!({
            "model": "anthropic/nan/mimo-v2.5",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "hello"}]
        }),
    )
    .await;
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
    let response = post_messages(
        &servers,
        "/v1/messages",
        &json!({
            "model": "anthropic/nan/deepseek-v4-flash-0731",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "hello"}]
        }),
    )
    .await;
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
