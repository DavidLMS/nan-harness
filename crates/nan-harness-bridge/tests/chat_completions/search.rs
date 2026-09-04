// Web search exposure: the local `/v1/search` endpoint and the native
// Anthropic `web_search` tool path, both gated on the same feature flag and
// on the local session token.

use super::support::{start_servers, start_servers_with_search};
use axum::http::StatusCode;
use serde_json::{Value, json};

#[tokio::test]
async fn chat_bridge_exposes_search_only_when_enabled_and_authenticated() {
    let servers = start_servers().await;
    let client = reqwest::Client::new();
    let endpoint = format!("{}/v1/search", servers.bridge.base_url());

    let unauthorized = client
        .post(&endpoint)
        .json(&json!({"query":"rust async"}))
        .send()
        .await
        .expect("unauthorized search should complete");
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

    let response = client
        .post(&endpoint)
        .bearer_auth("local-session-token")
        .json(&json!({"query":"rust async","maxResults":1}))
        .send()
        .await
        .expect("search should complete");
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.json::<Value>().await.expect("search JSON");
    assert_eq!(body["results"][0]["title"], "Tokio");
    assert!(
        body["summary"]
            .as_str()
            .expect("summary")
            .contains("tokio.rs")
    );
    servers.shutdown().await;

    let disabled = start_servers_with_search(false).await;
    let response = client
        .post(format!("{}/v1/search", disabled.bridge.base_url()))
        .bearer_auth("local-session-token")
        .json(&json!({"query":"rust async"}))
        .send()
        .await
        .expect("disabled search should complete");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    disabled.shutdown().await;
}

#[tokio::test]
async fn chat_bridge_serves_anthropic_search_for_native_search_providers() {
    let servers = start_servers().await;
    let response = reqwest::Client::new()
        .post(format!("{}/v1/messages", servers.bridge.base_url()))
        .bearer_auth("local-session-token")
        .json(&json!({
            "model": "qwen3.6",
            "max_tokens": 4096,
            "messages": [{
                "role": "user",
                "content": [{
                    "type": "text",
                    "text": "Perform a web search for the query: rust async"
                }]
            }],
            "tools": [{
                "type": "web_search_20250305",
                "name": "web_search",
                "max_uses": 1
            }]
        }))
        .send()
        .await
        .expect("native search request should complete");

    assert_eq!(response.status(), StatusCode::OK);
    let body = response
        .json::<Value>()
        .await
        .expect("Anthropic search JSON");
    assert_eq!(body["content"][1]["type"], "web_search_tool_result");
    assert_eq!(body["content"][1]["content"][0]["url"], "https://tokio.rs");
    servers.shutdown().await;

    let disabled = start_servers_with_search(false).await;
    let response = reqwest::Client::new()
        .post(format!("{}/v1/messages", disabled.bridge.base_url()))
        .bearer_auth("local-session-token")
        .json(&json!({
            "model": "qwen3.6",
            "max_tokens": 4096,
            "messages": [{"role": "user", "content": "Perform a web search for the query: rust async"}],
            "tools": [{"type": "web_search_20250305", "name": "web_search"}]
        }))
        .send()
        .await
        .expect("disabled native search request should complete");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    disabled.shutdown().await;
}
