use super::support::{BRIDGE_PLAN, assert_removed, start_model_provider, test_config_with_url};
use nan_harness_core::launch_plan::{
    ArtifactLifecycle, CLAUDE_MODEL_PICKER_PLACEHOLDER, CLAUDE_MODEL_PRESENTATIONS_PLACEHOLDER,
    CODEX_MODEL_CATALOG_PLACEHOLDER, ListenAddress, Protocol, TemporaryArtifact,
    TemporaryArtifactKind, TemporaryArtifactMode, TerminalMode, Transport,
};
use nan_harness_core::{HarnessKind, LaunchPlan, SecretRef};
use nan_harness_runtime::{CancellationToken, ExecutionOutcome, ProviderUsageSnapshot, Supervisor};

#[tokio::test]
async fn supervisor_prepares_and_cleans_an_anthropic_bridge_launch() {
    let (provider_base_url, provider_task) = start_model_provider().await;
    let working_directory = tempfile::tempdir().expect("working directory should exist");
    let mut plan: LaunchPlan = serde_json::from_str(BRIDGE_PLAN).expect("valid bridge fixture");
    "/bin/sh".clone_into(&mut plan.harness.executable);
    let settings_artifact = plan
        .temporary_artifacts
        .iter_mut()
        .find(|artifact| artifact.id == "claude-settings")
        .expect("Claude settings artifact");
    let mut settings: serde_json::Value = serde_json::from_str(
        settings_artifact
            .content_template
            .as_deref()
            .expect("Claude settings template"),
    )
    .expect("Claude settings template should be JSON");
    settings["modelPicker"] = serde_json::json!(CLAUDE_MODEL_PICKER_PLACEHOLDER);
    settings["env"][CLAUDE_MODEL_PRESENTATIONS_PLACEHOLDER] = serde_json::json!("");
    settings_artifact.content_template = Some(settings.to_string());
    plan.process.arguments = vec![
        "-c".to_owned(),
        concat!(
            "test -f \"$1\" && ",
            "test -n \"$ANTHROPIC_AUTH_TOKEN\" && ",
            "test \"${#ANTHROPIC_AUTH_TOKEN}\" -eq 64 && ",
            "test \"$ANTHROPIC_AUTH_TOKEN\" != \"test-key\" && ",
            "case \"$ANTHROPIC_AUTH_TOKEN\" in *[!0-9a-f]*) exit 9;; esac && ",
            "test -z \"$NAN_API_KEY\" && ",
            "test -z \"$CLAUDE_CODE_SUBPROCESS_ENV_SCRUB\" && ",
            "test \"$ANTHROPIC_MODEL\" = \"anthropic/nan/qwen3.6\" && ",
            "test \"$CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY\" = \"1\" && ",
            "grep -Fq '\"availableModels\":[\"anthropic/nan/qwen3.6\",\"anthropic/nan/mimo-v2.5\",\"anthropic/nan/gemma4\",\"anthropic/nan/deepseek-v4-flash-0731\"]' \"$1\" && ",
            "grep -Fq '\"replaceBuiltInOptions\":true' \"$1\" && ",
            "grep -Fq '\"model\":\"opus\"' \"$1\" && ",
            "grep -Fq '\"model\":\"anthropic/nan/mimo-v2.5[1m]\"' \"$1\" && ",
            "! grep -Fq '\"model\":\"anthropic/nan/gemma4[1m]\"' \"$1\" && ",
            "! grep -Fq '\"model\":\"anthropic/nan/deepseek-v4-flash-0731[1m]\"' \"$1\" && ",
            "grep -Fq '\"ANTHROPIC_DEFAULT_OPUS_MODEL\":\"anthropic/nan/qwen3.6\"' \"$1\" && ",
            "grep -Fq '\"disableAutoMode\":\"disable\"' \"$1\" && ",
            "grep -Fq '\"useAutoModeDuringPlan\":false' \"$1\" && ",
            "! grep -Fq 'CLAUDE_CODE_SUBPROCESS_ENV_SCRUB' \"$1\" && ",
            "case \"$ANTHROPIC_BASE_URL\" in http://127.0.0.1:*) exit 0;; *) exit 8;; esac"
        )
        .to_owned(),
        "nan-harness-test".to_owned(),
        "{artifact:claude-settings}".to_owned(),
    ];
    plan.process.working_directory = working_directory.path().to_string_lossy().into_owned();
    plan.process.terminal = TerminalMode::Captured;

    let report = Supervisor::new()
        .execute(
            &plan,
            &test_config_with_url(provider_base_url),
            &CancellationToken::new(),
        )
        .await
        .expect("bridge launch should complete");
    provider_task.abort();

    assert_eq!(report.outcome, ExecutionOutcome::Succeeded);
    assert_eq!(
        report.provider_usage,
        Some(ProviderUsageSnapshot::default())
    );
    assert_removed(report.temporary_root);
}

#[tokio::test]
async fn supervisor_materializes_a_codex_catalog_for_the_responses_bridge() {
    let (provider_base_url, provider_task) = start_model_provider().await;
    let working_directory = tempfile::tempdir().expect("working directory should exist");
    let mut plan: LaunchPlan = serde_json::from_str(BRIDGE_PLAN).expect("valid bridge fixture");
    plan.harness.kind = HarnessKind::Codex;
    "/bin/sh".clone_into(&mut plan.harness.executable);
    let provider_credential_ref = SecretRef::new("nan_api_key").expect("valid secret reference");
    let session_token_ref =
        SecretRef::new("bridge_session_token").expect("valid session token reference");
    plan.transport = Transport::ResponsesBridge {
        client_protocol: Protocol::OpenAiResponses,
        upstream_protocol: Protocol::ChatCompletions,
        listen: ListenAddress {
            host: "127.0.0.1".to_owned(),
            port: 0,
        },
        provider_credential_ref,
        session_token_ref,
    };
    plan.temporary_artifacts = vec![
        TemporaryArtifact {
            id: "codex-model-catalog".to_owned(),
            kind: TemporaryArtifactKind::File,
            path_hint: "catalog.json".to_owned(),
            mode: TemporaryArtifactMode::OwnerFile,
            content_template: Some(CODEX_MODEL_CATALOG_PLACEHOLDER.to_owned()),
            lifecycle: ArtifactLifecycle::Launch,
        },
        TemporaryArtifact {
            id: "codex-home".to_owned(),
            kind: TemporaryArtifactKind::Directory,
            path_hint: "codex-home".to_owned(),
            mode: TemporaryArtifactMode::OwnerDirectory,
            content_template: None,
            lifecycle: ArtifactLifecycle::Launch,
        },
    ];
    plan.process.arguments = vec![
        "-c".to_owned(),
        concat!(
            "catalog=${1#--catalog=} && ",
            "test -f \"$catalog\" && ",
            "grep -Fq '\"slug\":\"qwen3.6\"' \"$catalog\" && ",
            "grep -Fq '\"slug\":\"mimo-v2.5\"' \"$catalog\" && ",
            "grep -Fq '\"slug\":\"gemma4\"' \"$catalog\" && ",
            "grep -Fq '\"slug\":\"deepseek-v4-flash-0731\"' \"$catalog\" && ",
            "! grep -Fq '\"slug\":\"qwen3-embedding\"' \"$catalog\" && ",
            "grep -Fq '\"apply_patch_tool_type\":\"freeform\"' \"$catalog\" && ",
            "printf '%s\\n' 'model = \"mimo-v2.5\"' > \"$2/config.toml\""
        )
        .to_owned(),
        "nan-harness-test".to_owned(),
        "--catalog={artifact:codex-model-catalog}".to_owned(),
        "{artifact:codex-home}".to_owned(),
    ];
    plan.process.working_directory = working_directory.path().to_string_lossy().into_owned();
    plan.process.terminal = TerminalMode::Captured;

    let report = Supervisor::new()
        .execute(
            &plan,
            &test_config_with_url(provider_base_url.clone()),
            &CancellationToken::new(),
        )
        .await
        .expect("responses bridge launch should complete");
    assert_eq!(report.outcome, ExecutionOutcome::Succeeded);
    assert_eq!(report.selected_model.as_deref(), Some("mimo-v2.5"));
    assert_removed(report.temporary_root);

    plan.process.arguments[1] =
        "printf '%s\\n' 'model = \"qwen3.6\"' > \"$2/config.toml\"; exit 7".to_owned();
    let failed = Supervisor::new()
        .execute(
            &plan,
            &test_config_with_url(provider_base_url.clone()),
            &CancellationToken::new(),
        )
        .await
        .expect("failed Codex launch should still report completion");
    assert_eq!(failed.outcome, ExecutionOutcome::Failed);
    assert_eq!(failed.selected_model, None);
    assert_removed(failed.temporary_root);

    plan.process.arguments[1] =
        "printf '%s\\n' 'model = \"retired-model\"' > \"$2/config.toml\"".to_owned();
    let unavailable = Supervisor::new()
        .execute(
            &plan,
            &test_config_with_url(provider_base_url),
            &CancellationToken::new(),
        )
        .await
        .expect("Codex launch should complete");
    provider_task.abort();
    assert_eq!(unavailable.outcome, ExecutionOutcome::Succeeded);
    assert_eq!(unavailable.selected_model, None);
    assert_removed(unavailable.temporary_root);
}
