use super::support::{assert_direct_secret, context, plan};
use nan_harness_adapters::{PiAdapter, PrimeAgentAdapter};
use nan_harness_core::launch_plan::{
    NAN_SEARCH_BLOCK_BEGIN, PI_MODEL_CATALOG_PLACEHOLDER, PROVIDER_BASE_URL_PLACEHOLDER,
};
use nan_harness_core::{HarnessAdapter, HarnessKind, WebSearchPolicy};

#[test]
fn pi_and_prime_agent_load_the_same_ephemeral_provider_extension() {
    for (adapter, kind) in [
        (&PiAdapter as &dyn HarnessAdapter, HarnessKind::Pi),
        (
            &PrimeAgentAdapter as &dyn HarnessAdapter,
            HarnessKind::PrimeAgent,
        ),
    ] {
        let plan = plan(adapter, &context(kind, vec!["--continue".to_owned()]));
        let extension = plan.temporary_artifacts[0]
            .content_template
            .as_deref()
            .expect("provider extension should have content");

        assert_eq!(
            plan.process.arguments,
            [
                "--extension",
                "{artifact:pi-provider-extension}",
                "--provider",
                "nan",
                "--model",
                "qwen3.6",
                "--models",
                "nan/*",
                "--continue"
            ]
        );
        assert!(extension.contains("pi.registerProvider(\"nan\""));
        assert!(extension.contains(PROVIDER_BASE_URL_PLACEHOLDER));
        assert!(extension.contains("const apiKey = process.env.NAN_API_KEY"));
        assert!(extension.contains(PI_MODEL_CATALOG_PLACEHOLDER));
        assert!(extension.contains("profile.reasoningPolicy.kind"));
        assert!(extension.contains("thinkingLevelMap"));
        assert!(extension.contains("pi.on(\"resources_discover\""));
        assert!(extension.contains("pi.getAllTools()"));
        assert!(extension.contains("const forceNanSearch = false"));
        assert!(extension.contains("pi.registerTool({"));
        assert!(extension.contains("/v1/search"));
        assert!(extension.contains(NAN_SEARCH_BLOCK_BEGIN));
        assert!(!extension.contains("reasoning: false"));
        assert!(!extension.contains("fetch(`${baseUrl}/models`"));
        assert_direct_secret(&plan, "NAN_API_KEY");
    }
}

#[test]
fn pi_force_search_registers_a_precedence_override_at_runtime() {
    let mut force_context = context(HarnessKind::Pi, Vec::new());
    force_context.web_search_policy = WebSearchPolicy::Force;
    let plan = plan(&PiAdapter, &force_context);
    let extension = plan.temporary_artifacts[0]
        .content_template
        .as_deref()
        .expect("provider extension should have content");

    assert!(extension.contains("const forceNanSearch = true"));
    assert!(extension.contains("pi.getAllTools()"));
}

#[test]
fn prime_rust_uses_native_configuration_and_preserves_user_arguments() {
    use nan_harness_core::launch_plan::{
        PRIME_HOME_PLACEHOLDER, PRIME_PROVIDER_CATALOG_PLACEHOLDER,
    };
    for version in ["0.10.0", "0.11.0", "1.0.0"] {
        let mut context = context(
            HarnessKind::PrimeAgent,
            vec!["--thinking".into(), "high".into()],
        );
        context.harness.detected_version = version.into();
        let plan = plan(&PrimeAgentAdapter, &context);
        assert!(!plan.process.arguments.contains(&"--extension".into()));
        assert!(
            plan.process
                .arguments
                .ends_with(&["--thinking".into(), "high".into()])
        );
        assert!(plan.temporary_artifacts.is_empty());
        assert_direct_secret(&plan, "NAN_API_KEY");
        let overlay = &plan.configuration_overlays[0];
        assert_eq!(overlay.source_path, PRIME_HOME_PLACEHOLDER);
        assert!(
            overlay
                .files
                .iter()
                .find(|file| file.path == "models.json")
                .unwrap()
                .content_template
                .contains(PRIME_PROVIDER_CATALOG_PLACEHOLDER)
        );
        let settings = overlay
            .files
            .iter()
            .find(|file| file.path == "settings.json")
            .unwrap()
            .content_template
            .replace(NAN_SEARCH_BLOCK_BEGIN, "")
            .replace(nan_harness_core::launch_plan::NAN_SEARCH_BLOCK_END, "");
        let settings: serde_json::Value = serde_json::from_str(&settings).unwrap();
        assert_eq!(settings["mcpServers"]["nan-search"]["type"], "stdio");
        assert_eq!(settings["mcpServers"]["nan-search"]["args"][1], "--config");
        assert!(settings["mcpServers"]["nan-search"].get("env").is_none());
    }
}

#[test]
fn prime_legacy_keeps_its_provider_extension() {
    let mut context = context(HarnessKind::PrimeAgent, Vec::new());
    context.harness.detected_version = "0.9.3".into();
    assert!(
        plan(&PrimeAgentAdapter, &context)
            .process
            .arguments
            .contains(&"--extension".into())
    );
}

#[test]
fn prime_native_selects_the_toggle_provider_for_mimo() {
    let mut context = context(HarnessKind::PrimeAgent, Vec::new());
    context.harness.detected_version = "0.10.0".into();
    context.model.resolved_id = "mimo-v2.6-flash".into();
    let plan = plan(&PrimeAgentAdapter, &context);
    assert_eq!(plan.process.arguments[1], "nan-thinking");
    assert!(
        plan.process
            .arguments
            .contains(&"nan/*,nan-thinking/*".into())
    );
}
