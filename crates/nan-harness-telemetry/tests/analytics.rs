use axum::Router;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use nan_harness_telemetry::analytics::{AnalyticsError, UmamiExporter, UsageEvent};
use nan_harness_telemetry::consent::{InstallationId, TelemetryPreference, TelemetrySettingsStore};
use nan_harness_telemetry::event::{HarnessKind, OperationKind, Transport};
use serde_json::Value;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::oneshot;

#[tokio::test]
async fn umami_receives_only_the_allowlisted_invocation_contract() {
    let (address, request) = start_capture_server().await;
    let exporter = UmamiExporter::new(
        &format!("http://{address}"),
        "59cf95d9-bb3d-410d-95c5-5ac94a24b74e",
        Duration::from_secs(1),
    )
    .expect("loopback Umami endpoint should be valid");
    let installation_id = enabled_installation_id();

    exporter
        .export(
            &installation_id,
            UsageEvent::new(
                Some(HarnessKind::ClaudeCode),
                OperationKind::HarnessRun,
                Some(Transport::AnthropicBridge),
            ),
        )
        .await
        .expect("Umami should accept the event");

    let captured = request.await.expect("request should be captured");
    let body: Value = serde_json::from_slice(&captured.body).expect("body should be JSON");
    let payload = body["payload"]
        .as_object()
        .expect("payload should be an object");
    let data = payload["data"]
        .as_object()
        .expect("event data should be an object");

    assert_eq!(captured.path, "/api/send");
    assert_eq!(captured.content_type.as_deref(), Some("application/json"));
    assert!(captured.user_agent.starts_with("Mozilla/5.0"));
    assert!(!captured.user_agent.contains(installation_id.as_str()));
    assert!(!captured.user_agent.contains("NaNHarness/"));
    assert_eq!(body["type"], "event");
    assert_eq!(payload["website"], "59cf95d9-bb3d-410d-95c5-5ac94a24b74e");
    assert_eq!(payload["hostname"], "nan-harness.cli");
    assert_eq!(payload["url"], "/cli");
    assert_eq!(payload["name"], "nan-harness-claude-code");
    assert_eq!(payload["id"], installation_id.as_str());
    assert_eq!(payload["tag"], "harness:claude-code");
    assert_eq!(data["harness"], "claude-code");
    assert_eq!(data["nanHarnessVersion"], env!("CARGO_PKG_VERSION"));
    assert!(!data.contains_key("nanVersion"));
    assert_eq!(data["operation"], "harness-run");
    assert_eq!(data["transport"], "anthropic-bridge");
    assert_eq!(payload.len(), 7);
    assert_eq!(data.len(), 7);

    let serialized = String::from_utf8(captured.body).expect("body should be UTF-8");
    for forbidden in [
        "prompt",
        "output",
        "arguments",
        "model",
        "repository",
        "username",
        "apiKey",
        "/Users/",
    ] {
        assert!(!serialized.contains(forbidden));
    }
}

#[tokio::test]
async fn umami_classifies_an_operation_without_a_harness() {
    let (address, request) = start_capture_server().await;
    let exporter = UmamiExporter::new(
        &format!("http://{address}"),
        "59cf95d9-bb3d-410d-95c5-5ac94a24b74e",
        Duration::from_secs(1),
    )
    .expect("loopback Umami endpoint should be valid");
    let installation_id = enabled_installation_id();

    exporter
        .export(
            &installation_id,
            UsageEvent::new(None, OperationKind::Update, None),
        )
        .await
        .expect("Umami should accept the event");

    let captured = request.await.expect("request should be captured");
    let body: Value = serde_json::from_slice(&captured.body).expect("body should be JSON");
    let payload = body["payload"]
        .as_object()
        .expect("payload should be an object");
    let data = payload["data"]
        .as_object()
        .expect("event data should be an object");

    assert_eq!(payload["name"], "nan-operation-update");
    assert_eq!(payload["tag"], "operation:update");
    assert_eq!(data["nanHarnessVersion"], env!("CARGO_PKG_VERSION"));
    assert!(!data.contains_key("nanVersion"));
    assert_eq!(data["operation"], "update");
    assert!(!data.contains_key("harness"));
    assert!(!data.contains_key("transport"));
    assert_eq!(payload.len(), 7);
    assert_eq!(data.len(), 5);
}

#[test]
fn umami_configuration_rejects_unsafe_or_malformed_destinations() {
    assert!(matches!(
        UmamiExporter::new(
            "http://analytics.example.com",
            "59cf95d9-bb3d-410d-95c5-5ac94a24b74e",
            Duration::from_secs(1),
        ),
        Err(AnalyticsError::InsecureEndpoint)
    ));
    assert!(matches!(
        UmamiExporter::new(
            "https://analytics.example.com/private/path",
            "59cf95d9-bb3d-410d-95c5-5ac94a24b74e",
            Duration::from_secs(1),
        ),
        Err(AnalyticsError::UnsupportedEndpoint)
    ));
    assert!(matches!(
        UmamiExporter::new(
            "https://analytics.example.com",
            "not-a-website-id",
            Duration::from_secs(1),
        ),
        Err(AnalyticsError::InvalidWebsiteId)
    ));
}

fn enabled_installation_id() -> InstallationId {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let settings = TelemetrySettingsStore::new(directory.path());
    settings
        .set(TelemetryPreference::On)
        .expect("telemetry should enable");
    settings
        .active_installation_id()
        .expect("settings should load")
        .expect("enabled telemetry should have an installation ID")
}

#[derive(Debug)]
struct CapturedRequest {
    path: String,
    content_type: Option<String>,
    user_agent: String,
    body: Vec<u8>,
}

async fn start_capture_server() -> (std::net::SocketAddr, oneshot::Receiver<CapturedRequest>) {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("listener should bind");
    let address = listener.local_addr().expect("address should exist");
    let (sender, receiver) = oneshot::channel();
    let sender = Arc::new(Mutex::new(Some(sender)));
    let app = Router::new()
        .route("/api/send", post(capture_request))
        .with_state(sender);
    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("capture server should run");
    });
    (address, receiver)
}

async fn capture_request(
    State(sender): State<Arc<Mutex<Option<oneshot::Sender<CapturedRequest>>>>>,
    headers: HeaderMap,
    uri: axum::http::Uri,
    body: Bytes,
) -> StatusCode {
    let request = CapturedRequest {
        path: uri.path().to_owned(),
        content_type: headers
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .map(ToOwned::to_owned),
        user_agent: headers
            .get("user-agent")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_owned(),
        body: body.to_vec(),
    };
    if let Some(sender) = sender
        .lock()
        .expect("capture sender lock should not be poisoned")
        .take()
    {
        let _ = sender.send(request);
    }
    StatusCode::OK
}
