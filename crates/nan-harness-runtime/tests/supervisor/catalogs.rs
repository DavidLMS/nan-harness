use super::support::{DIRECT_PLAN, assert_removed, start_model_provider, test_config_with_url};
use nan_harness_core::LaunchPlan;
use nan_harness_core::coding_models_from_provider_ids;
use nan_harness_core::launch_plan::{
    AIDER_MODEL_METADATA_PLACEHOLDER, AIDER_MODEL_SETTINGS_PLACEHOLDER,
    CLINE_MODEL_CATALOG_PLACEHOLDER, DEEPSEEK_MODEL_CATALOG_PLACEHOLDER,
    GOOSE_MODEL_CATALOG_PLACEHOLDER, HERMES_MODEL_CATALOG_PLACEHOLDER,
    KIMI_CODE_MODEL_CATALOG_PLACEHOLDER, OPENCLAW_MODEL_ALIASES_PLACEHOLDER,
    OPENCLAW_MODEL_CATALOG_PLACEHOLDER, OPENCODE_MODEL_CATALOG_PLACEHOLDER,
    PI_MODEL_CATALOG_PLACEHOLDER, QWEN_CODE_MODEL_CATALOG_PLACEHOLDER,
    SELECTED_MODEL_CAPABILITIES_PLACEHOLDER, SELECTED_MODEL_CONTEXT_WINDOW_PLACEHOLDER,
    SELECTED_MODEL_DISPLAY_NAME_PLACEHOLDER, SELECTED_MODEL_MAX_OUTPUT_TOKENS_PLACEHOLDER,
    TerminalMode,
};
use nan_harness_runtime::{CancellationToken, ExecutionOutcome, LaunchSession, Supervisor};
use nan_harness_test_support::scripted_provider::{ProviderScenario, ScriptedProvider};

#[tokio::test]
async fn supervisor_materializes_new_text_models_in_every_direct_catalog_format() {
    let (provider_base_url, provider_task) = start_model_provider().await;
    let working_directory = tempfile::tempdir().expect("working directory should exist");
    let mut plan: LaunchPlan = serde_json::from_str(DIRECT_PLAN).expect("valid direct fixture");
    "/bin/sh".clone_into(&mut plan.harness.executable);
    for (name, placeholder) in [
        ("AIDER_METADATA", AIDER_MODEL_METADATA_PLACEHOLDER),
        ("AIDER_SETTINGS", AIDER_MODEL_SETTINGS_PLACEHOLDER),
        ("CLINE_MODELS", CLINE_MODEL_CATALOG_PLACEHOLDER),
        ("DEEPSEEK_MODELS", DEEPSEEK_MODEL_CATALOG_PLACEHOLDER),
        ("GOOSE_MODELS", GOOSE_MODEL_CATALOG_PLACEHOLDER),
        ("HERMES_MODELS", HERMES_MODEL_CATALOG_PLACEHOLDER),
        ("OPENCODE_MODELS", OPENCODE_MODEL_CATALOG_PLACEHOLDER),
        ("OPENCLAW_ALIASES", OPENCLAW_MODEL_ALIASES_PLACEHOLDER),
        ("OPENCLAW_MODELS", OPENCLAW_MODEL_CATALOG_PLACEHOLDER),
        ("PI_MODELS", PI_MODEL_CATALOG_PLACEHOLDER),
        ("QWEN_MODELS", QWEN_CODE_MODEL_CATALOG_PLACEHOLDER),
        ("KIMI_MODELS", KIMI_CODE_MODEL_CATALOG_PLACEHOLDER),
        (
            "SELECTED_CAPABILITIES",
            SELECTED_MODEL_CAPABILITIES_PLACEHOLDER,
        ),
        (
            "SELECTED_CONTEXT",
            SELECTED_MODEL_CONTEXT_WINDOW_PLACEHOLDER,
        ),
        ("SELECTED_NAME", SELECTED_MODEL_DISPLAY_NAME_PLACEHOLDER),
        (
            "SELECTED_OUTPUT",
            SELECTED_MODEL_MAX_OUTPUT_TOKENS_PLACEHOLDER,
        ),
    ] {
        plan.environment
            .public
            .insert(name.to_owned(), placeholder.to_owned());
    }
    plan.process.arguments = vec![
        "-c".to_owned(),
        concat!(
            "for name in AIDER_METADATA AIDER_SETTINGS CLINE_MODELS DEEPSEEK_MODELS ",
            "GOOSE_MODELS HERMES_MODELS OPENCODE_MODELS OPENCLAW_ALIASES ",
            "OPENCLAW_MODELS PI_MODELS QWEN_MODELS KIMI_MODELS; do ",
            "eval \"value=\\${$name}\"; ",
            "printf '%s' \"$value\" | grep -Fq 'deepseek-v4-flash-0731' || exit 21; ",
            "! printf '%s' \"$value\" | grep -Fq 'qwen3-embedding' || exit 22; ",
            "! printf '%s' \"$value\" | grep -Fq 'whisper' || exit 23; ",
            "! printf '%s' \"$value\" | grep -Fq 'minimax-h3' || exit 24; ",
            "done; ",
            "test \"$SELECTED_CAPABILITIES\" = 'image_in,thinking' && ",
            "test \"$SELECTED_CONTEXT\" = '262144' && ",
            "test \"$SELECTED_NAME\" = 'NaN · Qwen 3.6' && ",
            "test \"$SELECTED_OUTPUT\" = '65536' && ",
            "printf '%s' \"$OPENCODE_MODELS\" | grep -Fq 'capabilities not yet profiled' && ",
            "printf '%s' \"$GOOSE_MODELS\" | grep -Fq 'capabilities not yet profiled' && ",
            "printf '%s' \"$KIMI_MODELS\" | grep -Fq 'provider = \"__kimi_env__\"' && ",
            "printf '%s' \"$KIMI_MODELS\" | grep -Fq 'nan/mimo-v2.5' && ",
            "! printf '%s' \"$KIMI_MODELS\" | grep -Fq 'nan/qwen3.6'"
        )
        .to_owned(),
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
        .expect("direct catalog launch should complete");
    provider_task.abort();

    assert_eq!(report.outcome, ExecutionOutcome::Succeeded);
    assert_removed(report.temporary_root);
}

#[tokio::test]
async fn launch_session_reuses_or_skips_model_discovery_as_required() {
    let provider = ScriptedProvider::start(ProviderScenario::inventory("unused"))
        .await
        .expect("scripted provider should start");
    let config = test_config_with_url(provider.base_url().to_owned());
    let working_directory = tempfile::tempdir().expect("working directory should exist");
    let mut catalog_plan: LaunchPlan =
        serde_json::from_str(DIRECT_PLAN).expect("valid direct fixture");
    "/bin/sh".clone_into(&mut catalog_plan.harness.executable);
    catalog_plan.environment.public.insert(
        "MODEL_CATALOG".to_owned(),
        OPENCODE_MODEL_CATALOG_PLACEHOLDER.to_owned(),
    );
    catalog_plan.process.arguments = vec!["-c".to_owned(), "exit 0".to_owned()];
    catalog_plan.process.working_directory =
        working_directory.path().to_string_lossy().into_owned();
    catalog_plan.process.terminal = TerminalMode::Captured;
    let supervisor = Supervisor::new();
    let session = LaunchSession::new(&config);

    for _ in 0..2 {
        let report = supervisor
            .execute_in_session(&catalog_plan, &session, &CancellationToken::new())
            .await
            .expect("catalog launch should complete");
        assert_eq!(report.outcome, ExecutionOutcome::Succeeded);
        assert_removed(report.temporary_root);
    }
    assert_eq!(provider.model_requests(), 1);

    let seeded = LaunchSession::with_model_catalog(
        &config,
        coding_models_from_provider_ids(["qwen3.6".to_owned()]),
    );
    let report = supervisor
        .execute_in_session(&catalog_plan, &seeded, &CancellationToken::new())
        .await
        .expect("seeded catalog launch should complete");
    assert_eq!(report.outcome, ExecutionOutcome::Succeeded);
    assert_removed(report.temporary_root);
    assert_eq!(provider.model_requests(), 1);

    let mut direct_plan = catalog_plan;
    direct_plan.environment.public.remove("MODEL_CATALOG");
    let direct_session = LaunchSession::new(&config);
    let report = supervisor
        .execute_in_session(&direct_plan, &direct_session, &CancellationToken::new())
        .await
        .expect("direct launch without a catalog should complete");
    assert_eq!(report.outcome, ExecutionOutcome::Succeeded);
    assert_removed(report.temporary_root);
    assert_eq!(provider.model_requests(), 1);

    provider.shutdown().await.expect("provider should stop");
}
