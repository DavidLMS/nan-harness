use super::support::{assert_direct_secret, context, plan, with_search_block};
use nan_harness_adapters::GooseAdapter;
use nan_harness_core::HarnessKind;
use nan_harness_core::launch_plan::{
    GOOSE_MODEL_CATALOG_PLACEHOLDER, PROVIDER_BASE_URL_PLACEHOLDER,
};

#[test]
fn goose_routes_with_environment_without_hiding_user_extensions() {
    let plan = plan(
        &GooseAdapter,
        &context(
            HarnessKind::Goose,
            vec!["run".to_owned(), "--text".to_owned(), "inspect".to_owned()],
        ),
    );

    assert_eq!(plan.process.arguments, ["run", "--text", "inspect"]);
    assert_eq!(
        plan.environment.public.get("OPENAI_BASE_URL"),
        Some(&PROVIDER_BASE_URL_PLACEHOLDER.to_owned())
    );
    assert_eq!(
        plan.environment.public.get("GOOSE_PROVIDER"),
        Some(&"openai".to_owned())
    );
    assert_eq!(
        plan.environment.public.get("GOOSE_MODEL"),
        Some(&"qwen3.6".to_owned())
    );
    assert_eq!(
        plan.environment.public.get("GOOSE_PREDEFINED_MODELS"),
        Some(&GOOSE_MODEL_CATALOG_PLACEHOLDER.to_owned())
    );
    assert_eq!(
        plan.environment.public.get("GOOSE_ADDITIONAL_CONFIG_FILES"),
        Some(&"{runtime:goose_additional_config_files}{artifact:goose-nan-search}".to_owned())
    );
    let search_config = plan
        .temporary_artifacts
        .first()
        .and_then(|artifact| artifact.content_template.as_deref())
        .expect("Goose search config should exist");
    let search_config: serde_json::Value = serde_json::from_str(&with_search_block(search_config))
        .expect("Goose search config should be valid YAML-compatible JSON");
    assert_eq!(
        search_config["extensions"]["nan-search"]["cmd"],
        "nan-harness"
    );
    assert_eq!(
        search_config["extensions"]["nan-search"]["args"][0],
        "__search-mcp"
    );
    assert!(plan.configuration_overlays.is_empty());
    assert_direct_secret(&plan, "OPENAI_API_KEY");
}

#[test]
fn goose_defaults_to_an_interactive_session() {
    let plan = plan(&GooseAdapter, &context(HarnessKind::Goose, Vec::new()));

    assert_eq!(plan.process.arguments, ["session"]);
}
