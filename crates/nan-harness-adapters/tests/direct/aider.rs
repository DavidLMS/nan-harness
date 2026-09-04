use super::support::{assert_direct_secret, context, plan};
use nan_harness_adapters::AiderAdapter;
use nan_harness_core::HarnessKind;
use nan_harness_core::launch_plan::{
    AIDER_MODEL_METADATA_PLACEHOLDER, AIDER_MODEL_SETTINGS_PLACEHOLDER,
    PROVIDER_BASE_URL_PLACEHOLDER,
};

#[test]
fn aider_pins_every_internal_model_without_replacing_user_state() {
    let plan = plan(
        &AiderAdapter,
        &context(
            HarnessKind::Aider,
            vec!["--message".to_owned(), "inspect the project".to_owned()],
        ),
    );

    assert_eq!(
        plan.process.arguments,
        [
            "--model",
            "openai/qwen3.6",
            "--weak-model",
            "openai/qwen3.6",
            "--editor-model",
            "openai/qwen3.6",
            "--model-settings-file",
            "{artifact:aider-model-settings}",
            "--model-metadata-file",
            "{artifact:aider-model-metadata}",
            "--message",
            "inspect the project"
        ]
    );
    assert_eq!(
        plan.environment.public.get("AIDER_OPENAI_API_BASE"),
        Some(&PROVIDER_BASE_URL_PLACEHOLDER.to_owned())
    );
    assert_eq!(plan.temporary_artifacts.len(), 2);
    assert_eq!(
        plan.temporary_artifacts[0].content_template.as_deref(),
        Some(AIDER_MODEL_SETTINGS_PLACEHOLDER)
    );
    assert_eq!(
        plan.temporary_artifacts[1].content_template.as_deref(),
        Some(AIDER_MODEL_METADATA_PLACEHOLDER)
    );
    assert_direct_secret(&plan, "AIDER_OPENAI_API_KEY");
}
