use super::support::start_servers;
use axum::http::{StatusCode, header};
use nan_harness_bridge::ProviderUsageSnapshot;
use serde_json::{Value, json};

#[tokio::test]
async fn chat_bridge_authenticates_and_preserves_models_and_error_responses() {
    let servers = start_servers().await;
    let client = reqwest::Client::new();

    let unauthorized = client
        .get(format!("{}/v1/models", servers.bridge.base_url()))
        .send()
        .await
        .expect("models request should complete");
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

    let models = client
        .get(format!(
            "{}/v1/models?include=owned_by",
            servers.bridge.base_url()
        ))
        .bearer_auth("local-session-token")
        .send()
        .await
        .expect("models request should complete");
    assert_eq!(models.status(), StatusCode::OK);
    assert_eq!(models.headers()["x-upstream-marker"], "models");
    assert_eq!(
        models.json::<Value>().await.expect("models JSON"),
        json!({
            "object": "list",
            "data": [{"id": "qwen3.6"}]
        })
    );

    let unknown = client
        .get(format!("{}/v1/unknown", servers.bridge.base_url()))
        .bearer_auth("local-session-token")
        .send()
        .await
        .expect("unknown path should complete");
    assert_eq!(unknown.status(), StatusCode::NOT_FOUND);

    let error = client
        .post(format!(
            "{}/v1/chat/completions?trace=one",
            servers.bridge.base_url()
        ))
        .bearer_auth("local-session-token")
        .header("x-client-marker", "preserved")
        .json(&json!({"model":"error","messages":[]}))
        .send()
        .await
        .expect("error response should complete");
    assert_eq!(error.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(error.headers()["x-upstream-marker"], "error");
    assert_eq!(
        error.text().await.expect("error body"),
        r#"{"error":"rate limited"}"#
    );

    {
        let requests = servers.state.requests.lock().expect("request lock");
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].0[header::AUTHORIZATION],
            "Bearer provider-secret"
        );
        assert_eq!(requests[0].0["x-client-marker"], "preserved");
        assert_eq!(requests[0].1["model"], "error");
    }
    assert_eq!(servers.bridge.usage(), ProviderUsageSnapshot::default());
    servers.shutdown().await;
}
