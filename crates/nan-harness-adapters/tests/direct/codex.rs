use super::support::{context, plan};
use nan_harness_adapters::CodexAdapter;
use nan_harness_core::HarnessKind;
use nan_harness_core::launch_plan::{
    BRIDGE_BASE_URL_PLACEHOLDER, CODEX_HOME_ARTIFACT_PLACEHOLDER, CODEX_HOME_OVERLAY_ID,
    CODEX_PROFILE_ARTIFACT_ID, OverlayFilePolicy, Protocol,
    SELECTED_MODEL_REASONING_EFFORT_PLACEHOLDER, Transport,
};

#[test]
fn codex_uses_a_launch_scoped_profile_without_replacing_user_state() {
    let plan = plan(&CodexAdapter, &context(HarnessKind::Codex, Vec::new()));

    assert!(matches!(
        &plan.transport,
        Transport::ResponsesBridge {
            client_protocol: Protocol::OpenAiResponses,
            upstream_protocol: Protocol::ChatCompletions,
            ..
        }
    ));
    assert!(
        plan.process
            .arguments
            .iter()
            .any(|argument| argument.contains(&format!("{BRIDGE_BASE_URL_PLACEHOLDER}/v1")))
    );
    assert_eq!(
        &plan.process.arguments[..2],
        ["--profile", "nan-harness-launch_01directadapter"]
    );
    assert!(
        plan.process
            .arguments
            .iter()
            .any(|argument| argument == "model=\"qwen3.6\"")
    );
    assert!(plan.process.arguments.iter().any(|argument| {
        argument
            == &format!("model_reasoning_effort=\"{SELECTED_MODEL_REASONING_EFFORT_PLACEHOLDER}\"")
    }));
    assert!(
        plan.process
            .arguments
            .contains(&"features.standalone_web_search=true".to_owned())
    );
    assert!(
        plan.process
            .arguments
            .windows(2)
            .any(|arguments| arguments == ["--disable", "apps"])
    );
    assert_eq!(plan.temporary_artifacts.len(), 1);
    assert!(!plan.environment.public.contains_key("CODEX_HOME"));
    assert!(plan.configuration_overlays.is_empty());
    assert_eq!(plan.launch_scoped_files.len(), 1);
    assert_eq!(plan.launch_scoped_files[0].id, CODEX_PROFILE_ARTIFACT_ID);
    assert_eq!(
        plan.launch_scoped_files[0].directory,
        "{runtime:codex_home}"
    );
    assert_eq!(
        plan.launch_scoped_files[0].file_name,
        "nan-harness-launch_01directadapter.config.toml"
    );
    assert!(
        plan.launch_scoped_files[0]
            .content_template
            .contains(SELECTED_MODEL_REASONING_EFFORT_PLACEHOLDER)
    );
    assert_eq!(
        plan.temporary_artifacts[0].content_template.as_deref(),
        Some("{runtime:codex_model_catalog}")
    );
    assert!(
        plan.process.arguments.iter().any(|argument| {
            argument == "model_catalog_json=\"{artifact:codex-model-catalog}\""
        })
    );
    assert_eq!(
        plan.environment
            .secrets
            .get("NAN_HARNESS_SESSION_TOKEN")
            .expect("session token should be injected")
            .as_str(),
        "bridge_session_token"
    );
}

#[test]
fn codex_without_profile_support_uses_the_legacy_isolated_home() {
    let mut context = context(HarnessKind::Codex, Vec::new());
    context.harness.capabilities.clear();
    let plan = plan(&CodexAdapter, &context);

    assert_eq!(
        plan.environment
            .public
            .get("CODEX_HOME")
            .map(String::as_str),
        Some(CODEX_HOME_ARTIFACT_PLACEHOLDER)
    );
    assert!(plan.launch_scoped_files.is_empty());
    assert_eq!(plan.configuration_overlays.len(), 1);
    assert_eq!(plan.configuration_overlays[0].id, CODEX_HOME_OVERLAY_ID);
    assert_eq!(
        plan.configuration_overlays[0].files[0].policy,
        OverlayFilePolicy::MergeToml
    );
}
