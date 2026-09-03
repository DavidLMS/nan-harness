// Shared private fixtures and helpers for the supervisor suite: launch-plan
// templates, a generic shell-launch runner, test configuration with an
// in-memory credential, and the fake provider endpoints several concerns use.
use axum::Json;
use axum::Router;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use nan_harness_core::launch_plan::{LaunchPlan, TerminalMode};
use nan_harness_core::{SecretRef, SecretStore, SecretValue};
use nan_harness_runtime::{CancellationToken, ResolvedConfig, Supervisor};
use std::path::Path;

pub(super) const DIRECT_PLAN: &str =
    include_str!("../../../nan-harness-core/tests/fixtures/launch-plan.direct.json");
pub(super) const BRIDGE_PLAN: &str =
    include_str!("../../../nan-harness-core/tests/fixtures/launch-plan.bridge.json");

pub(super) async fn execute_shell(
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

pub(super) fn test_config() -> ResolvedConfig {
    test_config_with_url("http://127.0.0.1:9/v1".to_owned())
}

pub(super) fn test_config_with_url(provider_base_url: String) -> ResolvedConfig {
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

pub(super) async fn start_model_provider() -> (String, tokio::task::JoinHandle<()>) {
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

pub(super) async fn start_chat_provider(with_usage: bool) -> (String, tokio::task::JoinHandle<()>) {
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
            {"id": "minimax-h3", "object": "model"},
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

pub(super) fn assert_removed(path: Option<std::path::PathBuf>) {
    let path = path.expect("fixture includes a temporary artifact");
    assert!(!path.exists());
}
