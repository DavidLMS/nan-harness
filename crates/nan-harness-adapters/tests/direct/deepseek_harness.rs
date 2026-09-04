use super::support::{assert_direct_secret, context, plan};
use nan_harness_adapters::DeepSeekHarnessAdapter;
use nan_harness_core::HarnessKind;
use nan_harness_core::launch_plan::{
    BRIDGE_BASE_URL_PLACEHOLDER, DEEPSEEK_MODEL_CATALOG_PLACEHOLDER, NAN_SEARCH_BLOCK_BEGIN,
};

#[test]
fn deepseek_harness_uses_a_highest_precedence_patch_and_routes_conditional_search() {
    let plan = plan(
        &DeepSeekHarnessAdapter,
        &context(HarnessKind::DeepSeekHarness, Vec::new()),
    );
    let patch = plan.temporary_artifacts[0]
        .content_template
        .as_deref()
        .expect("DeepSeek Harness patch should have content");

    assert_eq!(
        plan.process.arguments,
        ["web", "--patch", "{artifact:deepseek-harness-patch}"]
    );
    assert_eq!(
        plan.environment.public.get("DSH_TELEMETRY_DISABLED"),
        Some(&"1".to_owned())
    );
    assert!(patch.contains("provider: nan-harness"));
    assert!(patch.contains("api: openai-completions"));
    assert!(patch.contains("baseURL: !!js process.env.NAN_HARNESS_PROVIDER_BASE_URL"));
    assert!(patch.contains(DEEPSEEK_MODEL_CATALOG_PLACEHOLDER));
    assert!(patch.contains("- id: web-search-deepseek\n  disabled: false"));
    assert!(patch.contains(&format!("baseURL: {BRIDGE_BASE_URL_PLACEHOLDER}/v1")));
    assert!(patch.contains(NAN_SEARCH_BLOCK_BEGIN));
    assert_direct_secret(&plan, "NAN_API_KEY");
}

#[test]
fn deepseek_harness_preserves_an_explicit_headless_profile() {
    let plan = plan(
        &DeepSeekHarnessAdapter,
        &context(
            HarnessKind::DeepSeekHarness,
            vec![
                "--profile".to_owned(),
                "headless".to_owned(),
                "inspect the project".to_owned(),
            ],
        ),
    );

    assert_eq!(
        plan.process.arguments,
        [
            "--profile",
            "headless",
            "--patch",
            "{artifact:deepseek-harness-patch}",
            "inspect the project"
        ]
    );
}
