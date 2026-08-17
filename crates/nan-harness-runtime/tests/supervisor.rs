#![cfg(unix)]

use axum::Json;
use axum::Router;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use nan_harness_core::launch_plan::{
    ArtifactLifecycle, CODEX_MODEL_CATALOG_PLACEHOLDER, ListenAddress,
    PROVIDER_BASE_URL_PLACEHOLDER, Protocol, TemporaryArtifact, TemporaryArtifactKind,
    TemporaryArtifactMode, TerminalMode, Transport,
};
use nan_harness_core::{HarnessKind, LaunchPlan, SecretRef, SecretStore, SecretValue};
use nan_harness_runtime::{
    CancellationToken, ExecutionOutcome, ResolvedConfig, SignalKind, Supervisor,
};
use std::time::Duration;

const DIRECT_PLAN: &str =
    include_str!("../../nan-harness-core/tests/fixtures/launch-plan.direct.json");
const BRIDGE_PLAN: &str =
    include_str!("../../nan-harness-core/tests/fixtures/launch-plan.bridge.json");

#[tokio::test]
async fn supervisor_preserves_success_and_failure_exit_codes_and_cleans_up() {
    let success = execute_shell("exit 0", true, None).await;
    assert_eq!(success.outcome, ExecutionOutcome::Succeeded);
    assert_eq!(success.exit_code, 0);
    assert_removed(success.temporary_root);

    let failure = execute_shell("exit 7", true, None).await;
    assert_eq!(failure.outcome, ExecutionOutcome::Failed);
    assert_eq!(failure.exit_code, 7);
    assert_removed(failure.temporary_root);

    let normalized = execute_shell("exit 7", false, None).await;
    assert_eq!(normalized.exit_code, 1);
    assert_removed(normalized.temporary_root);
}

#[tokio::test]
async fn supervisor_cancels_a_child_and_cleans_up() {
    let cancellation = CancellationToken::new();
    let trigger = cancellation.clone();
    let task = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(40)).await;
        trigger.cancel(SignalKind::Interrupt);
    });
    let report = execute_shell("while :; do :; done", true, Some(&cancellation)).await;
    task.await.expect("cancellation task should finish");

    assert_eq!(
        report.outcome,
        ExecutionOutcome::Cancelled(SignalKind::Interrupt)
    );
    assert_eq!(report.exit_code, 130);
    assert_removed(report.temporary_root);
}

#[tokio::test]
async fn supervisor_resolves_provider_urls_in_direct_overlays() {
    let working_directory = tempfile::tempdir().expect("working directory should exist");
    let mut plan: LaunchPlan = serde_json::from_str(DIRECT_PLAN).expect("valid direct fixture");
    "/bin/sh".clone_into(&mut plan.harness.executable);
    let Transport::DirectChat { base_url, .. } = &mut plan.transport else {
        panic!("fixture should use direct chat");
    };
    PROVIDER_BASE_URL_PLACEHOLDER.clone_into(base_url);
    plan.environment.public.insert(
        "NAN_HARNESS_PROVIDER_BASE_URL".to_owned(),
        PROVIDER_BASE_URL_PLACEHOLDER.to_owned(),
    );
    plan.temporary_artifacts[0].content_template = Some(format!(
        "{{\"baseURL\":\"{PROVIDER_BASE_URL_PLACEHOLDER}\"}}"
    ));
    plan.process.arguments = vec![
        "-c".to_owned(),
        concat!(
            "test \"$NAN_HARNESS_PROVIDER_BASE_URL\" = 'http://127.0.0.1:9/v1' && ",
            "grep -Fq 'http://127.0.0.1:9/v1' \"$1\""
        )
        .to_owned(),
        "nan-harness-test".to_owned(),
        "{artifact:opencode-config}".to_owned(),
    ];
    plan.process.working_directory = working_directory.path().to_string_lossy().into_owned();
    plan.process.terminal = TerminalMode::Captured;

    let report = Supervisor::new()
        .execute(&plan, &test_config(), &CancellationToken::new())
        .await
        .expect("direct launch should complete");

    assert_eq!(report.outcome, ExecutionOutcome::Succeeded);
    assert_removed(report.temporary_root);
}

#[tokio::test]
async fn supervisor_prepares_and_cleans_an_anthropic_bridge_launch() {
    let (provider_base_url, provider_task) = start_model_provider().await;
    let working_directory = tempfile::tempdir().expect("working directory should exist");
    let mut plan: LaunchPlan = serde_json::from_str(BRIDGE_PLAN).expect("valid bridge fixture");
    "/bin/sh".clone_into(&mut plan.harness.executable);
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
            "grep -Fq '\"availableModels\":[\"anthropic/nan/qwen3.6\",\"anthropic/nan/mimo-v2.5\",\"anthropic/nan/gemma4\"]' \"$1\" && ",
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
    assert_removed(report.temporary_root);
}

#[tokio::test]
async fn supervisor_materializes_a_codex_catalog_for_the_responses_bridge() {
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
    plan.temporary_artifacts = vec![TemporaryArtifact {
        id: "codex-model-catalog".to_owned(),
        kind: TemporaryArtifactKind::File,
        path_hint: "catalog.json".to_owned(),
        mode: TemporaryArtifactMode::OwnerFile,
        content_template: Some(CODEX_MODEL_CATALOG_PLACEHOLDER.to_owned()),
        lifecycle: ArtifactLifecycle::Launch,
    }];
    plan.process.arguments = vec![
        "-c".to_owned(),
        concat!(
            "catalog=${1#--catalog=} && ",
            "test -f \"$catalog\" && ",
            "grep -Fq '\"slug\":\"qwen3.6\"' \"$catalog\" && ",
            "grep -Fq '\"apply_patch_tool_type\":\"freeform\"' \"$catalog\""
        )
        .to_owned(),
        "nan-harness-test".to_owned(),
        "--catalog={artifact:codex-model-catalog}".to_owned(),
    ];
    plan.process.working_directory = working_directory.path().to_string_lossy().into_owned();
    plan.process.terminal = TerminalMode::Captured;

    let report = Supervisor::new()
        .execute(&plan, &test_config(), &CancellationToken::new())
        .await
        .expect("responses bridge launch should complete");

    assert_eq!(report.outcome, ExecutionOutcome::Succeeded);
    assert_removed(report.temporary_root);
}

async fn execute_shell(
    script: &str,
    preserve_exit_code: bool,
    cancellation: Option<&CancellationToken>,
) -> nan_harness_runtime::ExecutionReport {
    let working_directory = tempfile::tempdir().expect("working directory should exist");
    let mut plan: LaunchPlan = serde_json::from_str(DIRECT_PLAN).expect("valid direct fixture");
    "/bin/sh".clone_into(&mut plan.harness.executable);
    plan.process.arguments = vec!["-c".to_owned(), script.to_owned()];
    plan.process.working_directory = working_directory.path().to_string_lossy().into_owned();
    plan.process.terminal = TerminalMode::Captured;
    plan.process.preserve_exit_code = preserve_exit_code;
    let default_cancellation = CancellationToken::new();

    Supervisor::new()
        .execute(
            &plan,
            &test_config(),
            cancellation.unwrap_or(&default_cancellation),
        )
        .await
        .expect("direct execution should complete")
}

fn test_config() -> ResolvedConfig {
    test_config_with_url("http://127.0.0.1:9/v1".to_owned())
}

fn test_config_with_url(provider_base_url: String) -> ResolvedConfig {
    let reference = SecretRef::new("nan_api_key").expect("valid secret reference");
    let mut secrets = SecretStore::new();
    secrets.insert(
        reference.clone(),
        SecretValue::new("test-key").expect("valid secret value"),
    );
    ResolvedConfig {
        provider_base_url,
        provider_credential_ref: reference,
        secrets,
    }
}

async fn start_model_provider() -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("provider should bind");
    let address = listener.local_addr().expect("provider address");
    let router = Router::new().route("/v1/models", get(fake_models));
    let task = tokio::spawn(async move {
        axum::serve(listener, router)
            .await
            .expect("provider should serve");
    });
    (format!("http://{address}/v1"), task)
}

async fn fake_models(headers: HeaderMap) -> Response {
    if headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        != Some("Bearer test-key")
    {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    Json(serde_json::json!({
        "object": "list",
        "data": [
            {"id": "qwen3.6", "object": "model"},
            {"id": "mimo-v2.5", "object": "model"},
            {"id": "gemma4", "object": "model"},
            {"id": "qwen3-embedding", "object": "model"}
        ]
    }))
    .into_response()
}

fn assert_removed(path: Option<std::path::PathBuf>) {
    let path = path.expect("fixture includes a temporary artifact");
    assert!(!path.exists());
}
