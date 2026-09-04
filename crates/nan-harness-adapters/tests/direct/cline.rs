use super::support::{assert_direct_secret, assert_search_mcp, context, plan};
use nan_harness_adapters::ClineAdapter;
use nan_harness_core::HarnessKind;
use nan_harness_core::launch_plan::{
    CLINE_MODEL_CATALOG_PLACEHOLDER, PROVIDER_BASE_URL_PLACEHOLDER,
};

#[test]
fn cline_merges_provider_routing_and_models_into_linked_user_settings() {
    let plan = plan(&ClineAdapter, &context(HarnessKind::Cline, Vec::new()));
    let overlay = plan
        .configuration_overlays
        .first()
        .expect("Cline overlay should exist");
    let provider_file = overlay
        .files
        .iter()
        .find(|file| file.path == "data/settings/providers.json")
        .expect("Cline provider settings should exist");
    let models_file = overlay
        .files
        .iter()
        .find(|file| file.path == "data/settings/models.json")
        .expect("Cline model catalog should exist");
    let search_file = overlay
        .files
        .iter()
        .find(|file| file.path == "data/settings/mcp_settings.json")
        .expect("Cline search MCP settings should exist");
    let settings: serde_json::Value = serde_json::from_str(&provider_file.content_template)
        .expect("Cline settings should be JSON");

    assert_eq!(overlay.source_path, "{runtime:user_home}/.cline");
    assert_eq!(
        plan.process.arguments,
        [
            "--config",
            "{artifact:cline-config}",
            "--provider",
            "openai-compatible",
            "--model",
            "qwen3.6"
        ]
    );
    assert_eq!(
        settings["providers"]["openai-compatible"]["settings"]["baseUrl"],
        PROVIDER_BASE_URL_PLACEHOLDER
    );
    assert!(
        settings["providers"]["openai-compatible"]["settings"]
            .get("apiKey")
            .is_none()
    );
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&models_file.content_template)
            .expect("Cline model catalog should be JSON")["providers"]["openai-compatible"]["models"],
        CLINE_MODEL_CATALOG_PLACEHOLDER
    );
    assert_search_mcp(&search_file.content_template, "OPENAI_API_KEY");
    assert_direct_secret(&plan, "OPENAI_API_KEY");
}
