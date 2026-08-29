#![cfg(unix)]

use axum::Json;
use axum::Router;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use nan_harness_core::launch_plan::{
    AIDER_MODEL_METADATA_PLACEHOLDER, AIDER_MODEL_SETTINGS_PLACEHOLDER, ArtifactLifecycle,
    CLINE_MODEL_CATALOG_PLACEHOLDER, CODEX_MODEL_CATALOG_PLACEHOLDER,
    DEEPSEEK_MODEL_CATALOG_PLACEHOLDER, GOOSE_MODEL_CATALOG_PLACEHOLDER,
    HERMES_MODEL_CATALOG_PLACEHOLDER, KIMI_CODE_MODEL_CATALOG_PLACEHOLDER, ListenAddress,
    OPENCLAW_MODEL_ALIASES_PLACEHOLDER, OPENCLAW_MODEL_CATALOG_PLACEHOLDER,
    OPENCODE_MODEL_CATALOG_PLACEHOLDER, PI_MODEL_CATALOG_PLACEHOLDER,
    PROVIDER_BASE_URL_PLACEHOLDER, Protocol, QWEN_CODE_MODEL_CATALOG_PLACEHOLDER,
    SELECTED_MODEL_CAPABILITIES_PLACEHOLDER, SELECTED_MODEL_CONTEXT_WINDOW_PLACEHOLDER,
    SELECTED_MODEL_DISPLAY_NAME_PLACEHOLDER, SELECTED_MODEL_MAX_OUTPUT_TOKENS_PLACEHOLDER,
    TemporaryArtifact, TemporaryArtifactKind, TemporaryArtifactMode, TerminalMode, Transport,
};
use nan_harness_core::{HarnessKind, LaunchPlan, SecretRef, SecretStore, SecretValue};
use nan_harness_runtime::{
    CancellationToken, ExecutionOutcome, ResolvedConfig, SignalKind, Supervisor,
};
use std::path::Path;
use std::time::{Duration, Instant};

const DIRECT_PLAN: &str =
    include_str!("../../nan-harness-core/tests/fixtures/launch-plan.direct.json");
const BRIDGE_PLAN: &str =
    include_str!("../../nan-harness-core/tests/fixtures/launch-plan.bridge.json");

#[tokio::test]
async fn supervisor_preserves_success_and_failure_exit_codes_and_cleans_up() {
    let success = execute_shell("exit 0", true, None, None, None).await;
    assert_eq!(success.outcome, ExecutionOutcome::Succeeded);
    assert_eq!(success.exit_code, 0);
    assert_removed(success.temporary_root);

    let failure = execute_shell("exit 7", true, None, None, None).await;
    assert_eq!(failure.outcome, ExecutionOutcome::Failed);
    assert_eq!(failure.exit_code, 7);
    assert_removed(failure.temporary_root);

    let normalized = execute_shell("exit 7", false, None, None, None).await;
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
    let report = execute_shell("while :; do :; done", true, Some(&cancellation), None, None).await;
    task.await.expect("cancellation task should finish");

    assert_eq!(
        report.outcome,
        ExecutionOutcome::Cancelled(SignalKind::Interrupt)
    );
    assert_eq!(report.exit_code, 130);
    assert_removed(report.temporary_root);
}

#[tokio::test]
async fn supervisor_force_cancels_a_child_that_ignores_the_first_signal() {
    let cancellation = CancellationToken::new();
    let ready_directory = tempfile::tempdir().expect("ready directory should exist");
    let ready_path = ready_directory.path().join("trap-ready");
    let trigger = cancellation.clone();
    let trigger_ready_path = ready_path.clone();
    let task = tokio::spawn(async move {
        let ready = tokio::time::timeout(Duration::from_secs(1), async {
            while !trigger_ready_path.exists() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .is_ok();
        if !ready {
            trigger.cancel(SignalKind::Interrupt);
            return false;
        }
        trigger.cancel(SignalKind::Interrupt);
        tokio::time::sleep(Duration::from_millis(80)).await;
        trigger.cancel(SignalKind::Interrupt);
        true
    });
    let started = Instant::now();
    let report = execute_shell(
        "trap '' INT; : > \"$1\"; while :; do :; done",
        true,
        Some(&cancellation),
        Some(1_000),
        Some(&ready_path),
    )
    .await;
    let elapsed = started.elapsed();
    assert!(
        task.await.expect("cancellation task should finish"),
        "child should install its signal handler before cancellation"
    );

    assert_eq!(
        report.outcome,
        ExecutionOutcome::Cancelled(SignalKind::Interrupt)
    );
    assert_eq!(report.exit_code, 130);
    assert!(
        elapsed < Duration::from_millis(500),
        "second cancellation should skip the grace period (elapsed: {elapsed:?})"
    );
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
            "printf '%s\\n' \"$NAN_HARNESS_PROVIDER_BASE_URL\" | ",
            "grep -Eq '^http://127\\.0\\.0\\.1:[0-9]+/v1$' && ",
            "test \"$NAN_HARNESS_PROVIDER_BASE_URL\" != \"${NAN_HARNESS_PROVIDER_BASE_URL%/v1}\" && ",
            "grep -Fq \"$NAN_HARNESS_PROVIDER_BASE_URL\" \"$1\""
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
async fn supervisor_gives_direct_children_only_a_launch_scoped_session_token() {
    let report = execute_shell(
        "test \"${#NAN_API_KEY}\" -eq 64 && test \"$NAN_API_KEY\" != test-key",
        true,
        None,
        None,
        None,
    )
    .await;

    assert_eq!(report.outcome, ExecutionOutcome::Succeeded);
    assert_removed(report.temporary_root);
}

#[tokio::test]
async fn supervisor_can_run_direct_chat_without_the_gateway() {
    let working_directory = tempfile::tempdir().expect("working directory should exist");
    let mut plan: LaunchPlan = serde_json::from_str(DIRECT_PLAN).expect("valid direct fixture");
    "/bin/sh".clone_into(&mut plan.harness.executable);
    plan.environment.public.insert(
        "NAN_HARNESS_PROVIDER_BASE_URL".to_owned(),
        PROVIDER_BASE_URL_PLACEHOLDER.to_owned(),
    );
    plan.process.arguments = vec![
        "-c".to_owned(),
        concat!(
            "test \"$NAN_API_KEY\" = test-key && ",
            "test \"$NAN_HARNESS_PROVIDER_BASE_URL\" = http://127.0.0.1:9/v1"
        )
        .to_owned(),
    ];
    plan.process.working_directory = working_directory.path().to_string_lossy().into_owned();
    plan.process.terminal = TerminalMode::Captured;

    let report = Supervisor::new()
        .without_direct_chat_gateway()
        .execute(&plan, &test_config(), &CancellationToken::new())
        .await
        .expect("direct launch should complete without a gateway");

    assert_eq!(report.outcome, ExecutionOutcome::Succeeded);
    assert_eq!(report.chat_usage_observed, None);
    assert!(report.bridge_diagnostics.is_empty());
    assert_removed(report.temporary_root);
}

#[tokio::test]
async fn direct_chat_gateway_is_enabled_by_default() {
    let report = execute_shell(
        "test \"${#NAN_API_KEY}\" -eq 64 && test \"$NAN_API_KEY\" != test-key",
        true,
        None,
        None,
        None,
    )
    .await;

    assert_eq!(report.outcome, ExecutionOutcome::Succeeded);
}

#[tokio::test]
async fn supervisor_reports_direct_chat_usage_after_the_bridge_waits() {
    let with_usage = execute_direct_chat_request(true).await;
    assert_eq!(with_usage.outcome, ExecutionOutcome::Succeeded);
    assert_eq!(with_usage.chat_usage_observed, Some(true));
    assert_removed(with_usage.temporary_root);

    let without_usage = execute_direct_chat_request(false).await;
    assert_eq!(without_usage.outcome, ExecutionOutcome::Succeeded);
    assert_eq!(without_usage.chat_usage_observed, Some(false));
    assert_removed(without_usage.temporary_root);
}

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
            "grep -Fq '\"availableModels\":[\"anthropic/nan/qwen3.6\",\"anthropic/nan/mimo-v2.5\",\"anthropic/nan/gemma4\",\"anthropic/nan/deepseek-v4-flash-0731\"]' \"$1\" && ",
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
    assert_eq!(report.chat_usage_observed, None);
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

async fn execute_shell(
    script: &str,
    preserve_exit_code: bool,
    cancellation: Option<&CancellationToken>,
    grace_period_ms: Option<u32>,
    ready_path: Option<&Path>,
) -> nan_harness_runtime::ExecutionReport {
    let working_directory = tempfile::tempdir().expect("working directory should exist");
    let mut plan: LaunchPlan = serde_json::from_str(DIRECT_PLAN).expect("valid direct fixture");
    "/bin/sh".clone_into(&mut plan.harness.executable);
    plan.process.arguments = vec!["-c".to_owned(), script.to_owned()];
    if let Some(ready_path) = ready_path {
        plan.process.arguments.extend([
            "nan-harness-test".to_owned(),
            ready_path.to_string_lossy().into_owned(),
        ]);
    }
    plan.process.working_directory = working_directory.path().to_string_lossy().into_owned();
    plan.process.terminal = TerminalMode::Captured;
    plan.process.preserve_exit_code = preserve_exit_code;
    if let Some(grace_period_ms) = grace_period_ms {
        plan.cleanup.grace_period_ms = grace_period_ms;
    }
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

async fn execute_direct_chat_request(with_usage: bool) -> nan_harness_runtime::ExecutionReport {
    let (provider_base_url, provider_task) = start_chat_provider(with_usage).await;
    let working_directory = tempfile::tempdir().expect("working directory should exist");
    let mut plan: LaunchPlan = serde_json::from_str(DIRECT_PLAN).expect("valid direct fixture");
    "/bin/sh".clone_into(&mut plan.harness.executable);
    plan.environment.public.insert(
        "NAN_HARNESS_PROVIDER_BASE_URL".to_owned(),
        PROVIDER_BASE_URL_PLACEHOLDER.to_owned(),
    );
    plan.process.arguments = vec![
        "-c".to_owned(),
        concat!(
            "curl --fail --silent --show-error --header \"Authorization: Bearer $NAN_API_KEY\" ",
            "--header 'Content-Type: application/json' ",
            "--data '{\"model\":\"qwen3.6\",\"messages\":[]}' ",
            "$NAN_HARNESS_PROVIDER_BASE_URL/chat/completions >/dev/null"
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
        .expect("direct chat launch should complete");
    provider_task.abort();
    report
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

async fn start_chat_provider(with_usage: bool) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("provider should bind");
    let address = listener.local_addr().expect("provider address");
    let router = Router::new().route("/v1/models", get(fake_models)).route(
        "/v1/chat/completions",
        post(move || std::future::ready(fake_chat_completions(with_usage))),
    );
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
            {"id": "qwen3-embedding", "object": "model"},
            {"id": "whisper", "object": "model"},
            {"id": "deepseek-v4-flash-0731", "object": "model"}
        ]
    }))
    .into_response()
}

fn fake_chat_completions(with_usage: bool) -> Response {
    let mut body = serde_json::json!({
        "id": "chatcmpl-test",
        "object": "chat.completion",
        "choices": []
    });
    if with_usage {
        body["usage"] = serde_json::json!({
            "prompt_tokens": 1,
            "completion_tokens": 2,
            "completion_tokens_details": {"reasoning_tokens": 0}
        });
    }
    Json(body).into_response()
}

fn assert_removed(path: Option<std::path::PathBuf>) {
    let path = path.expect("fixture includes a temporary artifact");
    assert!(!path.exists());
}
