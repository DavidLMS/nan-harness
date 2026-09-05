use crate::common::{responses_request, start_servers};
use axum::http::StatusCode;
use nan_harness_bridge::{
    BridgeDiagnosticReason, BridgeModelPolicy, BridgeReasoningRequest, ModelUsageSnapshot,
    ProviderUsageSnapshot,
};
use serde_json::json;

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
    assert!(body.contains("response.in_progress"));
    assert!(
        body.find("response.created") < body.find("response.in_progress"),
        "creation must precede progress: {body}"
    );
    assert!(
        body.find("response.in_progress") < body.find("response.output_item.added"),
        "progress must precede visible output: {body}"
    );
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
