use super::support::{assert_direct_secret, assert_search_mcp, context, plan};
use nan_harness_adapters::KimiCodeAdapter;
use nan_harness_core::HarnessKind;
use nan_harness_core::launch_plan::{
    KIMI_CODE_MODEL_CATALOG_PLACEHOLDER, OverlayFilePolicy, PROVIDER_BASE_URL_PLACEHOLDER,
    SELECTED_MODEL_CAPABILITIES_PLACEHOLDER, SELECTED_MODEL_CONTEXT_WINDOW_PLACEHOLDER,
    SELECTED_MODEL_DISPLAY_NAME_PLACEHOLDER, SELECTED_MODEL_MAX_OUTPUT_TOKENS_PLACEHOLDER,
};

#[test]
fn kimi_code_exposes_a_launch_scoped_model_catalog() {
    let plan = plan(
        &KimiCodeAdapter,
        &context(
            HarnessKind::KimiCode,
            vec!["--prompt".to_owned(), "inspect the project".to_owned()],
        ),
    );

    assert_eq!(plan.process.arguments, ["--prompt", "inspect the project"]);
    assert_eq!(
        plan.environment.public.get("KIMI_MODEL_PROVIDER_TYPE"),
        Some(&"openai".to_owned())
    );
    assert_eq!(
        plan.environment.public.get("KIMI_MODEL_BASE_URL"),
        Some(&PROVIDER_BASE_URL_PLACEHOLDER.to_owned())
    );
    assert_eq!(
        plan.environment.public.get("KIMI_MODEL_DISPLAY_NAME"),
        Some(&SELECTED_MODEL_DISPLAY_NAME_PLACEHOLDER.to_owned())
    );
    assert_eq!(
        plan.environment.public.get("KIMI_MODEL_MAX_CONTEXT_SIZE"),
        Some(&SELECTED_MODEL_CONTEXT_WINDOW_PLACEHOLDER.to_owned())
    );
    assert_eq!(
        plan.environment.public.get("KIMI_MODEL_MAX_OUTPUT_SIZE"),
        Some(&SELECTED_MODEL_MAX_OUTPUT_TOKENS_PLACEHOLDER.to_owned())
    );
    assert_eq!(
        plan.environment.public.get("KIMI_MODEL_CAPABILITIES"),
        Some(&SELECTED_MODEL_CAPABILITIES_PLACEHOLDER.to_owned())
    );
    assert_eq!(
        plan.environment.public.get("KIMI_CODE_HOME"),
        Some(&"{artifact:kimi-code-home}".to_owned())
    );
    assert!(plan.temporary_artifacts.is_empty());
    let overlay = plan
        .configuration_overlays
        .first()
        .expect("Kimi Code home overlay should exist");
    assert_eq!(overlay.source_path, "{runtime:user_home}/.kimi-code");
    let config = overlay
        .files
        .first()
        .expect("Kimi Code config overlay should exist");
    assert_eq!(config.path, "config.toml");
    assert_eq!(config.content_template, KIMI_CODE_MODEL_CATALOG_PLACEHOLDER);
    assert_eq!(config.policy, OverlayFilePolicy::MergeToml);
    let search_file = overlay
        .files
        .iter()
        .find(|file| file.path == "mcp.json")
        .expect("Kimi Code search MCP settings should exist");
    assert_search_mcp(&search_file.content_template, "KIMI_MODEL_API_KEY");
    assert_direct_secret(&plan, "KIMI_MODEL_API_KEY");
}
