use super::capture_harness_response;
use axum::body::{Body, to_bytes};
use axum::response::Response;
use nan_harness_coordinator::{CaptureSink, enable_diagnostics};
use std::time::Duration;

#[tokio::test]
async fn capture_admission_rejection_preserves_response_bytes() {
    const CHILD: &str = "NAN_CAPTURE_ADMISSION_TEST_CHILD";
    if std::env::var_os(CHILD).is_some() {
        exercise_transparency().await;
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "upstream_capture::tests::capture_admission_rejection_preserves_response_bytes",
            "--nocapture",
        ])
        .env_clear()
        .env(CHILD, "1")
        .env("HOME", directory.path())
        .env("APPDATA", directory.path())
        .env("XDG_CONFIG_HOME", directory.path())
        .env("NAN_HARNESS_CONFIG_DIR", directory.path())
        .env("NAN_HARNESS_CREDENTIAL_BACKEND", "file")
        .env("NAN_HARNESS_INTERNAL_MANAGED_PROCESS", "1")
        .kill_on_drop(true);
    for name in ["SystemRoot", "LLVM_PROFILE_FILE"] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    let mut child = command.spawn().unwrap();
    let result = tokio::time::timeout(Duration::from_secs(15), child.wait()).await;
    if result.is_err() {
        child.kill().await.unwrap();
        child.wait().await.unwrap();
    }
    assert!(result.unwrap().unwrap().success());
}

async fn exercise_transparency() {
    let status = enable_diagnostics().unwrap();
    let sink = CaptureSink::new("admission-transparency");
    // One body frame exceeds the encoder input bound; the harness still gets
    // every byte, including binary data, with capture enabled or disabled.
    let payload = vec![0xff; 8 * 1024 * 1024 + 1];
    for capture in [None, sink.begin_request("synthetic-response")] {
        let response = Response::builder()
            .status(201)
            .header("x-synthetic", "unchanged")
            .body(Body::from(payload.clone()))
            .unwrap();
        let response = capture_harness_response(response, capture);
        assert_eq!(response.status(), 201);
        assert_eq!(response.headers()["x-synthetic"], "unchanged");
        assert_eq!(
            to_bytes(response.into_body(), usize::MAX).await.unwrap(),
            payload
        );
    }
    drop(sink);
    let directory = status
        .directory
        .join("captures")
        .join(status.capture_id.unwrap());
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    loop {
        if std::fs::read_dir(&directory)
            .unwrap()
            .filter_map(Result::ok)
            .any(|entry| {
                entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "incomplete")
            })
        {
            break;
        }
        assert!(tokio::time::Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}
