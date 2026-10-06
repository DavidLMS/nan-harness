use super::support::{assert_direct_secret, context, plan};
use nan_harness_adapters::ZCodeAdapter;
use nan_harness_core::{HarnessAdapter, HarnessCapability, HarnessKind, WebSearchPolicy};

#[test]
fn zcode_uses_private_launch_files_and_keeps_native_arguments() {
    let mut context = context(
        HarnessKind::ZCode,
        vec![
            "--prompt".into(),
            "synthetic prompt".into(),
            "--resume".into(),
            "session-id".into(),
        ],
    );
    context
        .harness
        .capabilities
        .insert(HarnessCapability::ZCodeConfigOverride);
    let plan = plan(&ZCodeAdapter, &context);
    assert_direct_secret(&plan, "ZCODE_NAN_API_KEY");
    assert_eq!(plan.process.arguments, context.user_arguments);
    assert_eq!(plan.temporary_artifacts.len(), 3);
    assert!(plan.configuration_overlays.is_empty());
    assert!(!plan.environment.public.contains_key("HOME"));
    assert_eq!(
        plan.environment.public["ZCODE_PERSONAL_PROVIDER_CONFIG_FILE"],
        "{artifact:zcode-provider}"
    );
    assert!(plan.environment.remove.contains("NAN_API_KEY"));
    assert!(
        plan.environment
            .remove
            .contains("ZCODE_BUILTIN_PROVIDER_BUNDLED_CONFIG_FILE")
    );
    assert!(
        !plan
            .environment
            .public
            .contains_key("ZCODE_BUILTIN_PROVIDER_BUNDLED_CONFIG_FILE")
    );
    let builtin = plan
        .temporary_artifacts
        .iter()
        .find(|artifact| artifact.id == "zcode-builtin")
        .unwrap();
    let config: serde_json::Value =
        serde_json::from_str(builtin.content_template.as_ref().unwrap()).unwrap();
    assert_eq!(config["revision"], 0);
    assert_eq!(
        config["config"]["providerConfigRules"]["providerRules"],
        serde_json::json!([])
    );
}

#[test]
fn zcode_rejects_routing_override_and_requires_private_binding_for_search() {
    let mut context = context(HarnessKind::ZCode, vec![]);
    assert!(ZCodeAdapter.plan(&context).is_err());
    context.web_search_policy = WebSearchPolicy::Disabled;
    assert!(ZCodeAdapter.plan(&context).is_ok());
    for flag in [
        "--model=other",
        "-mother",
        "--provider",
        "--config",
        "--nanh-source-info",
    ] {
        context.user_arguments = vec![flag.into()];
        assert!(ZCodeAdapter.plan(&context).is_err(), "{flag}");
    }
}
