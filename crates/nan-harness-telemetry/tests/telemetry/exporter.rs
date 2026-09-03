use axum::Router;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use nan_harness_telemetry::event::REOPEN_TERMINAL_GUIDANCE_TEXT;
use nan_harness_telemetry::glitchtip::{
    DEFAULT_EXPORT_TIMEOUT, ErrorReportExporter, ExportError, GlitchTipExporter,
};
use nan_harness_telemetry::panic::PendingReportStore;
use serde_json::Value;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::oneshot;

use crate::{report, report_with_guidance};

#[tokio::test]
async fn glitchtip_receives_a_bounded_envelope_with_only_allowlisted_context() {
    let (address, request) = start_capture_server().await;
    let exporter = GlitchTipExporter::new(
        &format!("http://public_key@{address}/42"),
        Duration::from_secs(1),
    )
    .expect("test DSN should be valid");

    exporter
        .export(&report_with_guidance(false))
        .await
        .expect("envelope should be accepted");
    let captured = request.await.expect("request should be captured");
    let lines = captured
        .body
        .split(|byte| *byte == b'\n')
        .collect::<Vec<_>>();
    let envelope_header: Value =
        serde_json::from_slice(lines[0]).expect("envelope header should be JSON");
    let item_header: Value = serde_json::from_slice(lines[1]).expect("item header should be JSON");
    let event: Value = serde_json::from_slice(lines[2]).expect("event should be JSON");

    assert_eq!(captured.path, "/api/42/envelope/");
    assert_eq!(
        captured.content_type.as_deref(),
        Some("application/x-sentry-envelope")
    );
    assert!(
        captured
            .authorization
            .starts_with("Sentry sentry_version=7")
    );
    assert_eq!(envelope_header["event_id"], event["event_id"]);
    assert_eq!(item_header["type"], "event");
    assert_eq!(
        event["contexts"]["nan_harness"]["failure"]["code"],
        "NH-TEST-001"
    );
    assert_eq!(
        event["fingerprint"],
        serde_json::json!(["NH-TEST-001", "invalid-response"])
    );
    assert_eq!(
        event["user"]["id"],
        event["contexts"]["nan_harness"]["installationId"]
    );
    assert_eq!(event["tags"]["diagnostic.reason"], "invalid-response");
    assert_eq!(event["tags"]["error.classification"], "environmental");
    assert_eq!(event["tags"]["user_guidance.id"], "reopen-terminal");
    assert_eq!(event["tags"]["user_guidance.shown"], "true");
    assert_eq!(
        event["contexts"]["nan_harness"]["userGuidance"]["text"],
        REOPEN_TERMINAL_GUIDANCE_TEXT
    );
    let body = String::from_utf8(captured.body).expect("envelope should be UTF-8");
    assert!(!body.contains("NAN_API_KEY"));
    assert!(!body.contains("/Users/"));
    assert!(!body.contains("prompt"));
    assert!(!body.contains("tool output"));
    assert!(!body.contains("qwen3.6"));
    assert!(event["tags"].get("operation.model").is_none());
    assert!(
        event["contexts"]["nan_harness"]["operation"]
            .get("model")
            .is_none()
    );
}

#[test]
fn glitchtip_dsn_requires_https_unless_it_targets_loopback() {
    for value in [
        "https://public_key@example.com/42",
        "http://public_key@127.0.0.1:8080/42",
        "http://public_key@localhost:3000/42",
        "http://public_key@[::1]:9000/42",
    ] {
        GlitchTipExporter::new(value, Duration::from_secs(1))
            .unwrap_or_else(|error| panic!("{value} should be accepted: {error:?}"));
    }

    assert!(matches!(
        GlitchTipExporter::new("http://public_key@example.com/42", Duration::from_secs(1)),
        Err(ExportError::UnsupportedDsn)
    ));
}

#[tokio::test]
async fn exporter_timeout_is_best_effort_and_pending_consent_is_bounded() {
    let address = start_slow_server().await;
    let exporter = GlitchTipExporter::new(
        &format!("http://public_key@{address}/42"),
        Duration::from_millis(20),
    )
    .expect("test DSN should be valid");
    let result = exporter.export(&report(false)).await;

    assert!(matches!(result, Err(ExportError::Request(_))));

    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let pending = PendingReportStore::new(directory.path());
    pending
        .save(&report(false))
        .expect("pending report should save");
    assert!(
        pending
            .load()
            .expect("pending report should load")
            .is_some()
    );
    pending.delete().expect("pending report should delete");
    assert!(!pending.path().exists());
}

#[tokio::test]
async fn exporter_retries_one_transient_timeout() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("listener should bind");
    let address = listener.local_addr().expect("address should exist");
    let app = Router::new()
        .route(
            "/api/42/envelope/",
            post(|State(attempts): State<Arc<AtomicUsize>>| async move {
                if attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
                StatusCode::OK
            }),
        )
        .with_state(Arc::clone(&attempts));
    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("retry server should run");
    });
    let exporter = GlitchTipExporter::new(
        &format!("http://public_key@{address}/42"),
        Duration::from_millis(20),
    )
    .expect("test DSN should be valid");

    exporter
        .export(&report(false))
        .await
        .expect("the retry should succeed");

    assert_eq!(attempts.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn exporter_does_not_retry_permanent_rejections() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("listener should bind");
    let address = listener.local_addr().expect("address should exist");
    let app = Router::new()
        .route(
            "/api/42/envelope/",
            post(|State(attempts): State<Arc<AtomicUsize>>| async move {
                attempts.fetch_add(1, Ordering::SeqCst);
                StatusCode::BAD_REQUEST
            }),
        )
        .with_state(Arc::clone(&attempts));
    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("rejection server should run");
    });
    let exporter = GlitchTipExporter::new(
        &format!("http://public_key@{address}/42"),
        Duration::from_secs(1),
    )
    .expect("test DSN should be valid");

    let result = exporter.export(&report(false)).await;

    assert!(matches!(
        result,
        Err(ExportError::Status(StatusCode::BAD_REQUEST))
    ));
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
}

#[tokio::test]
#[ignore = "requires NAN_HARNESS_GLITCHTIP_DSN and creates a real GlitchTip issue"]
async fn live_glitchtip_accepts_the_sanitized_error_contract() {
    let dsn = std::env::var("NAN_HARNESS_GLITCHTIP_DSN")
        .expect("NAN_HARNESS_GLITCHTIP_DSN should be configured");
    let exporter = GlitchTipExporter::new(&dsn, DEFAULT_EXPORT_TIMEOUT)
        .expect("GlitchTip DSN should be valid");

    exporter
        .export(&report(false))
        .await
        .expect("GlitchTip should accept the sanitized report");
}

#[derive(Debug)]
struct CapturedRequest {
    path: String,
    content_type: Option<String>,
    authorization: String,
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
        .route("/api/42/envelope/", post(capture_request))
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
        authorization: headers
            .get("x-sentry-auth")
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

async fn start_slow_server() -> std::net::SocketAddr {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("listener should bind");
    let address = listener.local_addr().expect("address should exist");
    let app = Router::new().route(
        "/api/42/envelope/",
        post(|| async {
            tokio::time::sleep(Duration::from_millis(200)).await;
            StatusCode::OK
        }),
    );
    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("slow server should run");
    });
    address
}
