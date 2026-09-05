use crate::common::{start_servers, start_servers_with_search};
use axum::http::StatusCode;
use nan_harness_bridge::CodexModelCatalog;
use nan_harness_core::known_coding_model;
use serde_json::{Value, json};

#[test]
fn codex_catalog_reports_exact_reasoning_picker_contracts_in_stable_order() {
    let catalog = CodexModelCatalog::from_provider_ids(
        [
            "glm5.2",
            "gemma4",
            "mimo-v2.5",
            "deepseek-v4-flash",
            "qwen3.6",
        ]
        .into_iter()
        .map(str::to_owned),
        "qwen3.6",
    )
    .expect("catalog should build");
    let response = catalog.api_response();
    let models = response["models"].as_array().expect("models list");
    let values = |index: usize| {
        (
            models[index]["slug"].clone(),
            models[index]["default_reasoning_level"].clone(),
            models[index]["supported_reasoning_levels"]
                .as_array()
                .expect("reasoning levels")
                .iter()
                .map(|level| level["effort"].clone())
                .collect::<Vec<_>>(),
        )
    };
    assert_eq!(
        values(0),
        (
            json!("qwen3.6"),
            json!("high"),
            vec![json!("none"), json!("high")]
        )
    );
    assert_eq!(
        values(1),
        (
            json!("deepseek-v4-flash"),
            json!("medium"),
            vec![json!("low"), json!("medium"), json!("high")]
        )
    );
    assert_eq!(
        values(2),
        (json!("mimo-v2.5"), json!("high"), vec![json!("high")])
    );
    assert_eq!(
        values(3),
        (
            json!("gemma4"),
            json!("none"),
            vec![json!("none"), json!("high")]
        )
    );
    assert_eq!(
        values(4),
        (
            json!("glm5.2"),
            json!("medium"),
            vec![json!("low"), json!("medium"), json!("high")]
        )
    );
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
    assert_eq!(models["models"][1]["slug"], "mimo-v2.5");
    for model in models["models"]
        .as_array()
        .expect("models should be a list")
    {
        let id = model["slug"].as_str().expect("slug should be text");
        assert_eq!(
            model["description"],
            known_coding_model(id)
                .expect("catalog models need shared metadata")
                .description
        );
    }
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

#[tokio::test]
async fn responses_bridge_keeps_models_available_when_search_is_disabled() {
    let servers = start_servers_with_search(false).await;
    let client = reqwest::Client::new();
    let models = client
        .get(format!("{}/v1/models", servers.bridge.base_url()))
        .bearer_auth("local-session-token")
        .send()
        .await
        .expect("models request should complete");
    assert_eq!(models.status(), StatusCode::OK);

    for path in ["/v1/alpha/search", "/v1/search"] {
        let response = client
            .post(format!("{}{path}", servers.bridge.base_url()))
            .bearer_auth("local-session-token")
            .json(&json!({}))
            .send()
            .await
            .expect("disabled search request should complete");
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
    }
    assert!(
        servers
            .state
            .search_requests
            .lock()
            .expect("search request lock")
            .is_empty()
    );
    servers.shutdown().await;
}
