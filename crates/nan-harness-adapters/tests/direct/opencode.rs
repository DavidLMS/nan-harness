use super::support::{assert_direct_secret, context, plan, without_search_block};
use nan_harness_adapters::OpenCodeAdapter;
use nan_harness_core::HarnessKind;
use nan_harness_core::launch_plan::{
    BRIDGE_BASE_URL_PLACEHOLDER, OPENCODE_MODEL_CATALOG_PLACEHOLDER,
};

#[test]
fn opencode_uses_an_inline_provider_overlay_without_hiding_user_plugins() {
    let plan = plan(
        &OpenCodeAdapter,
        &context(HarnessKind::OpenCode, Vec::new()),
    );
    let template = plan
        .environment
        .public
        .get("OPENCODE_CONFIG_CONTENT")
        .expect("OpenCode overlay should exist");
    let config: serde_json::Value = serde_json::from_str(&without_search_block(template))
        .expect("OpenCode overlay without its conditional block should be JSON");

    assert_eq!(plan.process.arguments, ["--model", "nan/qwen3.6"]);
    assert_eq!(config["enabled_providers"], serde_json::json!(["nan"]));
    assert_eq!(
        config["provider"]["nan"]["options"]["apiKey"],
        "{env:NAN_API_KEY}"
    );
    assert!(template.contains("\"nan-search\""));
    assert!(template.contains("\"__search-mcp\""));
    assert!(template.contains(BRIDGE_BASE_URL_PLACEHOLDER));
    assert_eq!(
        config["provider"]["nan"]["models"],
        OPENCODE_MODEL_CATALOG_PLACEHOLDER
    );
    assert!(plan.temporary_artifacts.is_empty());
    assert_direct_secret(&plan, "NAN_API_KEY");
}
