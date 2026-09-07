use super::{FINAL_ERROR_BODY_LIMIT, FinalErrorBody, UpstreamResponse};
use axum::body::Bytes;
use nan_harness_coordinator::{CaptureRequest, CaptureSink, DiagnosticsStatus, enable_diagnostics};
use reqwest::Body;
use std::convert::Infallible;
use std::ffi::OsStr;
use std::future::pending;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

const CAPTURE_SCENARIO: &str = "NAN_TEST_FINAL_ERROR_CAPTURE_SCENARIO";
const PROFILE_FILE: &str = "LLVM_PROFILE_FILE";

struct DropSignal(Arc<AtomicBool>);

impl Drop for DropSignal {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn final_error_reader_accepts_a_complete_body_at_the_exact_limit() {
    let payload = padded_json(
        "exact-limit-marker",
        "exact-limit-secret",
        FINAL_ERROR_BODY_LIMIT,
    );
    let response = response(Body::from(payload.clone()), None);

    let FinalErrorBody::Complete(body) = response.read_final_error_body().await else {
        panic!("an exact-limit body should be complete");
    };

    assert_eq!(body.as_bytes(), payload);
}

#[tokio::test]
async fn final_error_reader_discards_a_cut_off_body() {
    let dropped = Arc::new(AtomicBool::new(false));
    let response = response(failing_body(Arc::clone(&dropped)), None);

    let result = response.read_final_error_body().await;

    assert!(matches!(result, FinalErrorBody::Incomplete));
    assert!(dropped.load(Ordering::SeqCst));
}

#[tokio::test]
async fn final_error_reader_drops_its_body_when_cancelled() {
    let polls = Arc::new(AtomicUsize::new(0));
    let dropped = Arc::new(AtomicBool::new(false));
    let response = response(pending_body(Arc::clone(&polls), Arc::clone(&dropped)), None);
    let task = tokio::spawn(response.read_final_error_body());
    let deadline = Instant::now() + Duration::from_secs(1);
    while polls.load(Ordering::SeqCst) == 0 {
        assert!(Instant::now() < deadline, "body should be polled");
        tokio::task::yield_now().await;
    }

    task.abort();
    assert!(task.await.is_err_and(|error| error.is_cancelled()));
    assert!(dropped.load(Ordering::SeqCst));
}

#[tokio::test]
async fn final_error_capture_keeps_only_complete_bodies() {
    if std::env::var(CAPTURE_SCENARIO).as_deref() == Ok("capture") {
        exercise_capture().await;
        return;
    }

    let directory = tempfile::tempdir().expect("private capture directory");
    let test_name = "upstream::response::tests::final_error_capture_keeps_only_complete_bodies";
    let profile_file = std::env::var_os(PROFILE_FILE);
    let mut child = isolated_child_command(test_name, directory.path(), profile_file.as_deref())
        .spawn()
        .expect("isolated capture test process");
    let result = tokio::time::timeout(Duration::from_secs(10), child.wait()).await;
    if result.is_err() {
        child
            .kill()
            .await
            .expect("terminate timed-out capture test");
        child.wait().await.expect("reap timed-out capture test");
    }
    assert!(
        result
            .expect("capture test exceeded ten seconds")
            .expect("capture test status")
            .success()
    );
}

fn isolated_child_command(
    test_name: &str,
    directory: &Path,
    profile_file: Option<&OsStr>,
) -> tokio::process::Command {
    let mut command = tokio::process::Command::new(std::env::current_exe().expect("test binary"));
    command.args(["--exact", test_name, "--nocapture"]);
    command
        .env_clear()
        .env(CAPTURE_SCENARIO, "capture")
        .env("NAN_HARNESS_CONFIG_DIR", directory)
        .env("NAN_HARNESS_INTERNAL_MANAGED_PROCESS", "1")
        .kill_on_drop(true);
    if let Some(profile_file) = profile_file {
        command.env(PROFILE_FILE, profile_file);
    }
    command
}

async fn exercise_capture() {
    let status = enable_diagnostics().expect("diagnostic capture should enable");

    let (complete_sink, complete_capture) = capture_request("final-error-complete");
    let exact = padded_json("complete-marker", "complete-secret", FINAL_ERROR_BODY_LIMIT);
    let complete = response(Body::from(exact), Some(complete_capture.clone()))
        .read_final_error_body()
        .await;
    assert!(matches!(complete, FinalErrorBody::Complete(_)));
    drop(complete_capture);
    drop(complete_sink);

    let oversized_dropped = Arc::new(AtomicBool::new(false));
    let (oversized_sink, oversized_capture) = capture_request("final-error-oversized");
    let oversized = padded_json(
        "oversized-marker",
        "oversized-secret",
        FINAL_ERROR_BODY_LIMIT + 1,
    );
    let oversized_body = finite_body(oversized, Arc::clone(&oversized_dropped));
    let oversized = response(oversized_body, Some(oversized_capture.clone()))
        .read_final_error_body()
        .await;
    assert!(matches!(oversized, FinalErrorBody::Incomplete));
    assert!(oversized_dropped.load(Ordering::SeqCst));
    drop(oversized_capture);
    drop(oversized_sink);

    let cut_off_dropped = Arc::new(AtomicBool::new(false));
    let (cut_off_sink, cut_off_capture) = capture_request("final-error-cut-off");
    let cut_off = response(
        failing_body(Arc::clone(&cut_off_dropped)),
        Some(cut_off_capture.clone()),
    )
    .read_final_error_body()
    .await;
    assert!(matches!(cut_off, FinalErrorBody::Incomplete));
    assert!(cut_off_dropped.load(Ordering::SeqCst));
    drop(cut_off_capture);
    drop(cut_off_sink);

    let cancellation_polls = Arc::new(AtomicUsize::new(0));
    let cancellation_dropped = Arc::new(AtomicBool::new(false));
    let (cancellation_sink, cancellation_capture) = capture_request("final-error-cancellation");
    let cancellation = response(
        pending_body(
            Arc::clone(&cancellation_polls),
            Arc::clone(&cancellation_dropped),
        ),
        Some(cancellation_capture.clone()),
    );
    let task = tokio::spawn(cancellation.read_final_error_body());
    let deadline = Instant::now() + Duration::from_secs(1);
    while cancellation_polls.load(Ordering::SeqCst) == 0 {
        assert!(Instant::now() < deadline, "capture body should be polled");
        tokio::task::yield_now().await;
    }
    task.abort();
    assert!(task.await.is_err_and(|error| error.is_cancelled()));
    assert!(cancellation_dropped.load(Ordering::SeqCst));
    drop(cancellation_capture);
    drop(cancellation_sink);

    let (captured, markers) = wait_for_capture(&status, 3).await;
    assert!(captured.contains("complete-marker"), "{captured}");
    assert!(captured.contains("[REDACTED]"), "{captured}");
    assert!(!captured.contains("complete-secret"), "{captured}");
    assert!(!captured.contains("oversized-marker"), "{captured}");
    assert!(!captured.contains("oversized-secret"), "{captured}");
    assert!(!captured.contains("cut-off-private-marker"), "{captured}");
    assert_eq!(markers, 3);
}

fn capture_request(launch_id: &str) -> (CaptureSink, CaptureRequest) {
    let sink = CaptureSink::new(launch_id);
    let request = sink
        .begin_request(format!("request-{launch_id}"))
        .expect("capture request should start");
    (sink, request)
}

async fn wait_for_capture(status: &DiagnosticsStatus, expected_markers: usize) -> (String, usize) {
    let capture_id = status.capture_id.as_ref().expect("active capture id");
    let directory = status.directory.join("captures").join(capture_id);
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let entries = std::fs::read_dir(&directory).expect("capture directory should be readable");
        let mut captured = String::new();
        let mut markers = 0;
        for entry in entries.filter_map(Result::ok) {
            match entry.path().extension().and_then(OsStr::to_str) {
                Some("jsonl") => captured.push_str(
                    &std::fs::read_to_string(entry.path()).expect("capture should be UTF-8"),
                ),
                Some("incomplete") => markers += 1,
                _ => {}
            }
        }
        if markers == expected_markers {
            return (captured, markers);
        }
        assert!(Instant::now() < deadline, "capture writers should finish");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

fn response(body: Body, capture: Option<CaptureRequest>) -> UpstreamResponse {
    let response = reqwest::Response::from(
        axum::http::Response::builder()
            .status(400)
            .body(body)
            .expect("synthetic final response"),
    );
    UpstreamResponse::new(response, None, capture)
}

fn padded_json(marker: &str, secret: &str, size: usize) -> Vec<u8> {
    let mut body = format!(r#"{{"marker":"{marker}","api_key":"{secret}"}}"#).into_bytes();
    assert!(body.len() <= size);
    body.resize(size, b' ');
    body
}

fn finite_body(payload: Vec<u8>, dropped: Arc<AtomicBool>) -> Body {
    let signal = DropSignal(dropped);
    Body::wrap_stream(async_stream::stream! {
        let _signal = signal;
        for chunk in payload.chunks(4 * 1024) {
            yield Ok::<Bytes, Infallible>(Bytes::copy_from_slice(chunk));
        }
    })
}

fn failing_body(dropped: Arc<AtomicBool>) -> Body {
    let signal = DropSignal(dropped);
    Body::wrap_stream(async_stream::stream! {
        let _signal = signal;
        yield Ok::<Bytes, std::io::Error>(Bytes::from_static(
            br#"{"message":"cut-off-private-marker""#,
        ));
        yield Err(std::io::Error::other("synthetic body cut-off"));
    })
}

fn pending_body(polls: Arc<AtomicUsize>, dropped: Arc<AtomicBool>) -> Body {
    let signal = DropSignal(dropped);
    Body::wrap_stream(async_stream::stream! {
        let _signal = signal;
        polls.fetch_add(1, Ordering::SeqCst);
        pending::<()>().await;
        yield Ok::<Bytes, Infallible>(Bytes::new());
    })
}
