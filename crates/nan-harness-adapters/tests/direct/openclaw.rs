use super::support::{
    assert_direct_secret, context, plan, with_search_block, without_search_block,
};
use nan_harness_adapters::OpenClawAdapter;
use nan_harness_core::HarnessKind;
use nan_harness_core::launch_plan::{
    BRIDGE_BASE_URL_PLACEHOLDER, OPENCLAW_MODEL_ALIASES_PLACEHOLDER,
    OPENCLAW_MODEL_CATALOG_PLACEHOLDER,
};

#[test]
fn openclaw_merges_user_configuration_without_persisting_the_nan_secret() {
    let plan = plan(
        &OpenClawAdapter,
        &context(HarnessKind::OpenClaw, Vec::new()),
    );
    let overlay = plan
        .configuration_overlays
        .first()
        .expect("OpenClaw overlay should exist");
    let config_file = overlay
        .files
        .iter()
        .find(|file| file.path == "nan-harness.json")
        .expect("nan-harness configuration should exist");
    let config: serde_json::Value =
        serde_json::from_str(&without_search_block(&config_file.content_template))
            .expect("OpenClaw configuration without search should be JSON");
    let search_config: serde_json::Value =
        serde_json::from_str(&with_search_block(&config_file.content_template))
            .expect("OpenClaw configuration with search should be JSON");
    let search_plugin = overlay
        .files
        .iter()
        .find(|file| file.path == "plugins/nan-harness-search/index.js")
        .expect("OpenClaw search plugin should exist");

    assert_eq!(plan.process.arguments, ["chat"]);
    assert_eq!(overlay.source_path, "{runtime:user_home}/.openclaw");
    assert_eq!(config["$include"], "./openclaw.json");
    assert_eq!(
        config["models"]["providers"]["nan"]["apiKey"],
        serde_json::json!({
            "id": "NAN_API_KEY",
            "provider": "default",
            "source": "env"
        })
    );
    assert_eq!(
        config["agents"]["defaults"]["models"],
        OPENCLAW_MODEL_ALIASES_PLACEHOLDER
    );
    assert_eq!(
        config["models"]["providers"]["nan"]["models"],
        OPENCLAW_MODEL_CATALOG_PLACEHOLDER
    );
    assert_eq!(
        search_config["tools"]["web"]["search"]["provider"],
        "nan-harness"
    );
    assert_eq!(
        search_config["plugins"]["load"]["paths"][0],
        "{artifact:openclaw-config}/plugins/nan-harness-search"
    );
    assert!(
        search_plugin
            .content_template
            .contains("registerWebSearchProvider")
    );
    assert!(
        search_plugin
            .content_template
            .contains(BRIDGE_BASE_URL_PLACEHOLDER)
    );
    assert!(
        !overlay
            .files
            .iter()
            .any(|file| file.content_template.contains("nan-secret-value"))
    );
    assert_direct_secret(&plan, "NAN_API_KEY");
}
