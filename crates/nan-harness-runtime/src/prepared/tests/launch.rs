use crate::prepared::{BridgePreparation, PreparedLaunch, requires_model_catalog};
use nan_harness_core::SecretRef;
use nan_harness_core::launch_plan::{
    ArtifactLifecycle, ConfigurationOverlay, LaunchPlan, OPENCODE_MODEL_CATALOG_PLACEHOLDER,
    OverlayFile, OverlayFilePolicy, TemporaryArtifactMode,
};
use std::fs;
use std::sync::Arc;

use super::support::model;

#[test]
fn prepared_search_overlay_resolves_to_valid_enabled_or_disabled_json() {
    for enabled in [false, true] {
        let source_home = tempfile::tempdir().expect("empty search source");
        let source =
            include_str!("../../../../nan-harness-core/tests/fixtures/launch-plan.direct.json");
        let mut plan: LaunchPlan = serde_json::from_str(source).expect("fixture should parse");
        plan.configuration_overlays.push(ConfigurationOverlay {
            id: "search-home".to_owned(),
            path_hint: "search-home".to_owned(),
            source_path: source_home.path().to_string_lossy().into_owned(),
            files: vec![OverlayFile {
                path: "mcp.json".to_owned(),
                mode: TemporaryArtifactMode::OwnerFile,
                content_template: concat!(
                    "{{runtime:nan_search:begin}\"mcpServers\":{\"nan-search\":{",
                    "\"command\":\"nan-harness\",\"args\":[\"__search-mcp\",",
                    "\"--endpoint\",\"{runtime:bridge_base_url}/v1/search\",",
                    "\"--token-env\",\"NAN_API_KEY\"]}}{runtime:nan_search:end}}"
                )
                .to_owned(),
                policy: OverlayFilePolicy::MergeJson,
            }],
            lifecycle: ArtifactLifecycle::Launch,
        });
        let token_ref = SecretRef::new("nan_api_key").expect("secret reference");
        let prepared = PreparedLaunch::prepare(
            &plan,
            "https://api.nan.builders/v1",
            Some(BridgePreparation {
                base_url: "http://127.0.0.1:3210".to_owned(),
                client_base_url: Some("http://127.0.0.1:3210/v1".to_owned()),
                chat_url: None,
                session_token_ref: token_ref,
                session_token: Arc::new(
                    nan_harness_core::SecretValue::new("local-session-token")
                        .expect("session token"),
                ),
                claude_available_models: Vec::new(),
                codex_model_catalog: None,
                web_search_enabled: enabled,
            }),
            None,
        )
        .expect("search overlay should prepare");
        let path = prepared
            .artifact_path("search-home")
            .expect("search overlay path")
            .join("mcp.json");
        let value: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(path).expect("rendered search overlay should be readable"),
        )
        .expect("rendered search overlay should be JSON");
        if enabled {
            let server = &value["mcpServers"]["nan-search"];
            assert_eq!(server["command"], "nan-harness");
            assert_eq!(server["args"][2], "http://127.0.0.1:3210/v1/search");
        } else {
            assert!(value["mcpServers"].get("nan-search").is_none());
        }
    }
}

#[test]
fn catalog_placeholders_in_arguments_trigger_live_discovery() {
    let source =
        include_str!("../../../../nan-harness-core/tests/fixtures/launch-plan.direct.json");
    let mut plan: LaunchPlan = serde_json::from_str(source).expect("fixture should parse");
    plan.process.arguments = vec![OPENCODE_MODEL_CATALOG_PLACEHOLDER.to_owned()];

    assert!(requires_model_catalog(&plan));
}

#[test]
fn model_catalog_placeholders_in_arguments_are_rendered() {
    let source =
        include_str!("../../../../nan-harness-core/tests/fixtures/launch-plan.direct.json");
    let mut plan: LaunchPlan = serde_json::from_str(source).expect("fixture should parse");
    plan.process.arguments = vec![OPENCODE_MODEL_CATALOG_PLACEHOLDER.to_owned()];
    let models = [model("qwen3.6")];

    let prepared =
        PreparedLaunch::prepare(&plan, "https://api.nan.builders/v1", None, Some(&models))
            .expect("argument catalog should render");

    assert!(prepared.arguments()[0].contains("qwen3.6"));
    assert!(!prepared.arguments()[0].contains(OPENCODE_MODEL_CATALOG_PLACEHOLDER));
}
