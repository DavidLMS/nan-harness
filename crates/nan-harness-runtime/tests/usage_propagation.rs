use axum::Json;
use axum::Router;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use nan_harness_core::launch_plan::{PROVIDER_BASE_URL_PLACEHOLDER, TerminalMode};
use nan_harness_core::{LaunchPlan, SecretRef, SecretStore, SecretValue};
use nan_harness_runtime::{
    CancellationToken, ExecutionOutcome, ModelUsageSnapshot, ProviderUsageSnapshot, ResolvedConfig,
    Supervisor,
};
use std::collections::BTreeMap;
use std::io::{Read as _, Write as _};
use std::net::TcpStream;

const DIRECT_PLAN: &str =
    include_str!("../../nan-harness-core/tests/fixtures/launch-plan.direct.json");
const CHILD_MARKER: &str = "NAN_HARNESS_TEST_USAGE_CHILD";
const CHILD_BASE_URL: &str = "NAN_HARNESS_TEST_PROVIDER_BASE_URL";

#[tokio::test]
async fn supervisor_propagates_usage_after_bridge_shutdown_cross_platform() {
    let (provider_base_url, provider_task) = start_provider().await;
    let working_directory = tempfile::tempdir().expect("working directory should exist");
    let mut plan: LaunchPlan = serde_json::from_str(DIRECT_PLAN).expect("valid direct fixture");
    plan.harness.executable = std::env::current_exe()
        .expect("test executable should be available")
        .to_string_lossy()
        .into_owned();
    plan.environment
        .public
        .insert(CHILD_MARKER.to_owned(), "1".to_owned());
    plan.environment.public.insert(
        CHILD_BASE_URL.to_owned(),
        PROVIDER_BASE_URL_PLACEHOLDER.to_owned(),
    );
    plan.process.arguments = vec![
        "--exact".to_owned(),
        "usage_child_sends_one_chat_request".to_owned(),
        "--nocapture".to_owned(),
    ];
    plan.process.working_directory = working_directory.path().to_string_lossy().into_owned();
    plan.process.terminal = TerminalMode::Captured;

    let report = Supervisor::new()
        .execute(
            &plan,
            &test_config(provider_base_url),
            &CancellationToken::new(),
        )
        .await
        .expect("cross-platform direct launch should complete");
    provider_task.abort();

    assert_eq!(report.outcome, ExecutionOutcome::Succeeded);
    assert_eq!(
        report.provider_usage,
        Some(ProviderUsageSnapshot {
            models: BTreeMap::from([(
                "qwen3.8-flash".to_owned(),
                ModelUsageSnapshot {
                    responses_with_usage: 1,
                    input_tokens: 21,
                    output_tokens: 8,
                    reasoning_tokens: 3,
                    ..ModelUsageSnapshot::default()
                },
            )]),
        })
    );
}

#[test]
fn usage_child_sends_one_chat_request() {
    if std::env::var_os(CHILD_MARKER).is_none() {
        return;
    }
    let base_url = std::env::var(CHILD_BASE_URL).expect("bridge URL should be present");
    let token = std::env::var("NAN_API_KEY").expect("session token should be present");
    let endpoint = base_url
        .strip_prefix("http://")
        .expect("bridge URL should use loopback HTTP");
    let (authority, path) = endpoint
        .split_once('/')
        .expect("bridge URL should include a base path");
    let body = r#"{"model":"qwen3.8-flash","messages":[]}"#;
    let mut connection = TcpStream::connect(authority).expect("bridge should accept a connection");
    write!(
        connection,
        "POST /{path}/chat/completions HTTP/1.1\r\nHost: {authority}\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .expect("request should be written");
    connection.flush().expect("request should be flushed");
    let mut response = Vec::new();
    connection
        .read_to_end(&mut response)
        .expect("response should be readable");
    assert!(response.starts_with(b"HTTP/1.1 200"), "{response:?}");
}

fn test_config(provider_base_url: String) -> ResolvedConfig {
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

async fn start_provider() -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("provider should bind");
    let address = listener.local_addr().expect("provider address");
    let router = Router::new()
        .route("/v1/models", get(models))
        .route("/v1/chat/completions", post(chat_completions));
    let task = tokio::spawn(async move {
        axum::serve(listener, router)
            .await
            .expect("provider should serve");
    });
    (format!("http://{address}/v1"), task)
}

async fn models(headers: HeaderMap) -> Response {
    if !provider_authorized(&headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    Json(serde_json::json!({
        "object": "list",
        "data": [{"id": "qwen3.6", "object": "model"}]
    }))
    .into_response()
}

async fn chat_completions(headers: HeaderMap) -> Response {
    if !provider_authorized(&headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    Json(serde_json::json!({
        "id": "chatcmpl-cross-platform",
        "choices": [],
        "usage": {
            "prompt_tokens": 21,
            "completion_tokens": 8,
            "completion_tokens_details": {"reasoning_tokens": 3}
        }
    }))
    .into_response()
}

fn provider_authorized(headers: &HeaderMap) -> bool {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        == Some("Bearer test-key")
}
