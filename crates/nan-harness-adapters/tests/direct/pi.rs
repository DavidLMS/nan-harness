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
