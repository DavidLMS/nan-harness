use super::support::{assert_direct_secret, context, plan};
use nan_harness_adapters::HermesAdapter;
use nan_harness_core::HarnessKind;
use nan_harness_core::launch_plan::{
    BRIDGE_BASE_URL_PLACEHOLDER, HERMES_MODEL_CATALOG_PLACEHOLDER, NAN_SEARCH_BLOCK_BEGIN,
    OverlayFilePolicy, PROVIDER_BASE_URL_PLACEHOLDER,
};

#[test]
fn hermes_loads_a_launch_scoped_nan_provider_without_hiding_user_state() {
    let plan = plan(
        &HermesAdapter,
        &context(HarnessKind::Hermes, vec!["--tui".to_owned()]),
    );

    assert_eq!(
        plan.process.arguments,
        ["--provider", "nan", "--model", "qwen3.6", "--tui"]
    );
    let overlay = plan
        .configuration_overlays
        .first()
        .expect("Hermes home overlay should exist");
    let plugin = overlay
        .files
        .iter()
        .find(|file| file.path.ends_with("__init__.py"))
        .expect("NaN provider plugin should exist");
    let search_provider = overlay
        .files
        .iter()
        .find(|file| file.path.ends_with("web/nan_harness/provider.py"))
        .expect("NaN search provider should exist");
    let search_config = overlay
        .files
        .iter()
        .find(|file| file.path == "config.yaml")
        .expect("Hermes search config should exist");
    assert_eq!(overlay.source_path, "{runtime:user_home}/.hermes");
    assert_eq!(
        plan.environment.public.get("HERMES_HOME"),
        Some(&"{artifact:hermes-home}".to_owned())
    );
    assert!(
        plugin
            .content_template
            .contains(PROVIDER_BASE_URL_PLACEHOLDER)
    );
    assert!(
        search_provider
            .content_template
            .contains(BRIDGE_BASE_URL_PLACEHOLDER)
    );
    assert!(search_provider.content_template.contains("maxResults"));
    assert!(
        search_config
            .content_template
            .contains(NAN_SEARCH_BLOCK_BEGIN)
    );
    assert_eq!(search_config.policy, OverlayFilePolicy::MergeYaml);
    assert!(
        plugin
            .content_template
            .contains(HERMES_MODEL_CATALOG_PLACEHOLDER)
    );
    assert!(plan.environment.remove.contains("OPENAI_BASE_URL"));
    assert_direct_secret(&plan, "NAN_API_KEY");
}
