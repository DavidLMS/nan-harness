use super::report::parse_codex_reasoning;
#[cfg(unix)]
use super::{ExecutionOutcome, LaunchSession, Supervisor};
#[cfg(unix)]
use crate::config::ResolvedConfig;
#[cfg(unix)]
use crate::signals::CancellationToken;
#[cfg(unix)]
use nan_harness_bridge::ProviderUsageSnapshot;
#[cfg(unix)]
use nan_harness_core::launch_plan::{
    BRIDGE_BASE_URL_PLACEHOLDER, FX_GATEWAY_CHAT_URL_PLACEHOLDER, ListenAddress, TerminalMode,
    Transport,
};
#[cfg(unix)]
use nan_harness_core::{
    HarnessKind, LaunchPlan, SecretRef, SecretStore, SecretValue, coding_models_from_provider_ids,
};
use nan_harness_core::{ReasoningEffort, ReasoningPolicy, ReasoningSelection};

#[cfg(unix)]
#[tokio::test]
async fn fx_gateway_launch_uses_scoped_credentials_and_explicit_routes() {
    let working_directory = tempfile::tempdir().expect("working directory should exist");
    let mut plan: LaunchPlan = serde_json::from_str(include_str!(
        "../../../nan-harness-core/tests/fixtures/launch-plan.bridge.json"
    ))
    .expect("fixture should be valid");
    plan.harness.kind = HarnessKind::Fx;
    "/bin/sh".clone_into(&mut plan.harness.executable);
    let provider_credential_ref =
        SecretRef::new("nan_api_key").expect("valid provider credential reference");
    let session_token_ref =
        SecretRef::new("fx_gateway_session_token").expect("valid session token reference");
    plan.transport = Transport::FxGatewayBridge {
        listen: ListenAddress {
            host: "127.0.0.1".to_owned(),
            port: 0,
        },
        provider_credential_ref: provider_credential_ref.clone(),
        session_token_ref: session_token_ref.clone(),
    };
    plan.environment.public.clear();
    plan.environment.public.insert(
        "FX_GATEWAY_BASE_URL".to_owned(),
        BRIDGE_BASE_URL_PLACEHOLDER.to_owned(),
    );
    plan.environment.public.insert(
        "FX_GATEWAY_CHAT_URL".to_owned(),
        FX_GATEWAY_CHAT_URL_PLACEHOLDER.to_owned(),
    );
    plan.environment.secrets.clear();
    plan.environment
        .secrets
        .insert("AI_GATEWAY_API_KEY".to_owned(), session_token_ref);
    plan.environment.remove.clear();
    plan.temporary_artifacts.clear();
    plan.configuration_overlays.clear();
    plan.launch_scoped_files.clear();
    plan.observability.redact_environment_names.clear();
    plan.observability
        .redact_environment_names
        .insert("AI_GATEWAY_API_KEY".to_owned());
    plan.process.arguments = vec![
        "-c".to_owned(),
        concat!(
            "test \"${#AI_GATEWAY_API_KEY}\" -eq 64 && ",
            "test \"$AI_GATEWAY_API_KEY\" != test-key && ",
            "case \"$AI_GATEWAY_API_KEY\" in *[!0-9a-f]*) exit 9;; esac && ",
            "case \"$FX_GATEWAY_BASE_URL\" in http://127.0.0.1:*) ;; *) exit 8;; esac && ",
            "test \"$FX_GATEWAY_CHAT_URL\" = ",
            "\"$FX_GATEWAY_BASE_URL/v3/ai/language-model\""
        )
        .to_owned(),
    ];
    plan.process.working_directory = working_directory.path().to_string_lossy().into_owned();
    plan.process.terminal = TerminalMode::Captured;

    let mut secrets = SecretStore::new();
    secrets.insert(
        provider_credential_ref.clone(),
        SecretValue::new("test-key").expect("valid secret value"),
    );
    let config = ResolvedConfig {
        provider_base_url: "http://127.0.0.1:9/v1".to_owned(),
        provider_credential_ref,
        secrets,
    };
    let session = LaunchSession::with_model_catalog(
        &config,
        coding_models_from_provider_ids(["qwen3.6".to_owned()]),
    );

    let report = Supervisor::new()
        .execute_in_session(&plan, &session, &CancellationToken::new())
        .await
        .expect("fx gateway launch should complete");

    assert_eq!(report.outcome, ExecutionOutcome::Succeeded);
    assert_eq!(
        report.provider_usage,
        Some(ProviderUsageSnapshot::default())
    );
    assert!(report.bridge_diagnostics.is_empty());
    assert_eq!(report.temporary_root, None);
}

#[test]
fn codex_reasoning_state_uses_shared_policy_resolution() {
    assert_eq!(
        parse_codex_reasoning("medium", ReasoningPolicy::AlwaysOn),
        Some(ReasoningSelection::Toggle(true))
    );
    assert_eq!(
        parse_codex_reasoning(
            "medium",
            ReasoningPolicy::Toggle {
                default_enabled: false,
            }
        ),
        Some(ReasoningSelection::Toggle(true))
    );
    assert_eq!(
        parse_codex_reasoning(
            "medium",
            ReasoningPolicy::Effort {
                supported: [
                    ReasoningEffort::Low,
                    ReasoningEffort::Medium,
                    ReasoningEffort::High,
                ],
                default: ReasoningEffort::Medium,
            }
        ),
        Some(ReasoningSelection::Effort(ReasoningEffort::Medium))
    );
    assert_eq!(
        parse_codex_reasoning("medium", ReasoningPolicy::Unknown),
        Some(ReasoningSelection::Auto)
    );
}
