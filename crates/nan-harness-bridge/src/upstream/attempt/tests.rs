use super::{UpstreamAttempt, classify_attempt, fallback_delay, retry_after};
use crate::error::{ApiError, UpstreamTimeoutPhase};
use axum::body::Bytes;
use nan_harness_coordinator::{
    AttemptOutcome, CaptureRequest, CaptureSink, DiagnosticsStatus, enable_diagnostics,
};
use reqwest::Body;
use reqwest::header::{HeaderMap, HeaderValue, RETRY_AFTER};
use std::convert::Infallible;
use std::ffi::OsStr;
use std::future::pending;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

const CAPTURE_LIMIT: usize = 64 * 1024;
const CAPTURE_SCENARIO: &str = "NAN_TEST_RETRY_CAPTURE_SCENARIO";
const PROFILE_FILE: &str = "LLVM_PROFILE_FILE";

struct DropSignal(Arc<AtomicBool>);

impl Drop for DropSignal {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn retryable_http_statuses_preserve_the_final_response() {
    for status in [408, 425, 429, 500, 502, 503, 504] {
        let response = || {
            reqwest::Response::from(
                axum::http::Response::builder()
                    .status(status)
                    .header("retry-after", "7")
                    .body(Body::from("synthetic provider failure"))
                    .expect("synthetic response"),
            )
        };
        let retry = classify_attempt(Ok(response()), false, None).await;
        let expected = if status == 429 {
            AttemptOutcome::RateLimited
        } else {
            AttemptOutcome::ServerError
        };
        assert!(matches!(retry, UpstreamAttempt::Retry));
        assert_eq!(super::status_outcome(response().status()), expected);
        assert_eq!(
            retry_after(response().headers()),
            Some(Duration::from_secs(7))
        );
        let UpstreamAttempt::Complete(final_response) =
            classify_attempt(Ok(response()), true, None).await
        else {
            panic!("the final HTTP response must remain available to the caller");
        };
        assert_eq!(final_response.status().as_u16(), status);
        assert_eq!(
            final_response.text().await.expect("body"),
            "synthetic provider failure"
        );
    }
}

#[tokio::test]
async fn retry_without_capture_drops_the_body_without_polling_it() {
    let polls = Arc::new(AtomicUsize::new(0));
    let dropped = Arc::new(AtomicBool::new(false));
    let response = retry_response(
        503,
        unfinished_body(Arc::clone(&polls), Arc::clone(&dropped)),
    );

    let retry = tokio::time::timeout(
        Duration::from_millis(100),
        classify_attempt(Ok(response), false, None),
    )
    .await
    .expect("capture-off classification must not wait for the response body");

    assert!(matches!(retry, UpstreamAttempt::Retry));
    assert_eq!(polls.load(Ordering::SeqCst), 0);
    assert!(dropped.load(Ordering::SeqCst));
}

#[tokio::test]
async fn retry_capture_keeps_only_complete_bodies_within_the_limit() {
    run_isolated_capture_scenario("bounds").await;
}

#[tokio::test]
async fn retry_capture_marks_body_failures_and_cancellation_incomplete() {
    run_isolated_capture_scenario("failures").await;
}

async fn run_isolated_capture_scenario(scenario: &str) {
    if std::env::var(CAPTURE_SCENARIO).is_ok_and(|selected| selected == scenario) {
        exercise_capture_scenario(scenario).await;
        return;
    }

    let directory = tempfile::tempdir().expect("private capture directory");
    let test_name = if scenario == "bounds" {
        "upstream::attempt::tests::retry_capture_keeps_only_complete_bodies_within_the_limit"
    } else {
        "upstream::attempt::tests::retry_capture_marks_body_failures_and_cancellation_incomplete"
    };
    let profile_file = std::env::var_os(PROFILE_FILE);
    let mut child = isolated_child_command(
        test_name,
        scenario,
        directory.path(),
        profile_file.as_deref(),
    )
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
    scenario: &str,
    directory: &Path,
    profile_file: Option<&OsStr>,
) -> tokio::process::Command {
    let mut command = tokio::process::Command::new(std::env::current_exe().expect("test binary"));
    command.args(["--exact", test_name, "--nocapture"]);
    command
        .env_clear()
        .env(CAPTURE_SCENARIO, scenario)
        .env("NAN_HARNESS_CONFIG_DIR", directory)
        .env("NAN_HARNESS_INTERNAL_MANAGED_PROCESS", "1")
        .kill_on_drop(true);
    if let Some(profile_file) = profile_file {
        command.env(PROFILE_FILE, profile_file);
    }
    command
}

async fn exercise_capture_scenario(scenario: &str) {
    let status = enable_diagnostics().expect("diagnostic capture should enable");
    match scenario {
        "bounds" => exercise_capture_bounds(&status).await,
        "failures" => exercise_capture_failures(&status).await,
        unexpected => panic!("unexpected capture scenario {unexpected}"),
    }
}

async fn exercise_capture_bounds(status: &DiagnosticsStatus) {
    let sink = CaptureSink::new("retry-capture-bounds");
    let capture = sink
        .begin_request("request-bounds")
        .expect("capture request should start");
    let exact = padded_json("exact-marker", "exact-secret", CAPTURE_LIMIT);
    let exact_response = retry_response(429, Body::from(exact));
    assert!(matches!(
        classify_attempt(Ok(exact_response), false, Some(&capture)).await,
        UpstreamAttempt::Retry
    ));

    let consumed = Arc::new(AtomicUsize::new(0));
    let dropped = Arc::new(AtomicBool::new(false));
    let oversized = finite_body(
        padded_json("oversized-marker", "oversized-secret", CAPTURE_LIMIT * 4),
        4 * 1024,
        Arc::clone(&consumed),
        Arc::clone(&dropped),
    );
    let oversized_response = retry_response(503, oversized);
    assert!(matches!(
        classify_attempt(Ok(oversized_response), false, Some(&capture)).await,
        UpstreamAttempt::Retry
    ));
    assert!(consumed.load(Ordering::SeqCst) <= CAPTURE_LIMIT + 4 * 1024);
    assert!(dropped.load(Ordering::SeqCst));

    drop(capture);
    drop(sink);
    let (captured, markers) = wait_for_capture(status, 1).await;
    assert!(captured.contains("exact-marker"), "{captured}");
    assert!(captured.contains("[REDACTED]"), "{captured}");
    assert!(!captured.contains("exact-secret"), "{captured}");
    assert!(!captured.contains("oversized-marker"), "{captured}");
    assert!(!captured.contains("oversized-secret"), "{captured}");
    assert!(!captured.contains("private-cookie"), "{captured}");
    assert_eq!(markers, 1);
}

async fn exercise_capture_failures(status: &DiagnosticsStatus) {
    let read_dropped = Arc::new(AtomicBool::new(false));
    let (read_sink, read_capture) = capture_request("retry-read-error");
    let read_response = retry_response(503, failing_body(Arc::clone(&read_dropped)));
    assert!(matches!(
        classify_attempt(Ok(read_response), false, Some(&read_capture)).await,
        UpstreamAttempt::Retry
    ));
    assert!(read_dropped.load(Ordering::SeqCst));
    drop(read_capture);
    drop(read_sink);

    let progress = Arc::new(AtomicUsize::new(0));
    let timeout_dropped = Arc::new(AtomicBool::new(false));
    let (timeout_sink, timeout_capture) = capture_request("retry-timeout");
    let timeout_response = retry_response(
        503,
        progressing_body(Arc::clone(&progress), Arc::clone(&timeout_dropped)),
    );
    let started = Instant::now();
    let retry = tokio::time::timeout(
        Duration::from_secs(3),
        classify_attempt(Ok(timeout_response), false, Some(&timeout_capture)),
    )
    .await
    .expect("the absolute retry-body deadline should beat the outer test deadline");
    assert!(matches!(retry, UpstreamAttempt::Retry));
    assert!(started.elapsed() < Duration::from_secs(3));
    assert!(progress.load(Ordering::SeqCst) > 1);
    assert!(timeout_dropped.load(Ordering::SeqCst));
    drop(timeout_capture);
    drop(timeout_sink);

    let cancellation_dropped = Arc::new(AtomicBool::new(false));
    let (cancellation_sink, cancellation_capture) = capture_request("retry-cancellation");
    let cancellation_response =
        retry_response(503, pending_body(Arc::clone(&cancellation_dropped)));
    assert!(
        tokio::time::timeout(
            Duration::from_millis(100),
            classify_attempt(
                Ok(cancellation_response),
                false,
                Some(&cancellation_capture),
            ),
        )
        .await
        .is_err()
    );
    assert!(cancellation_dropped.load(Ordering::SeqCst));
    drop(cancellation_capture);
    drop(cancellation_sink);

    let (captured, markers) = wait_for_capture(status, 3).await;
    assert!(!captured.contains("read-error-secret"), "{captured}");
    assert!(!captured.contains("timeout-secret"), "{captured}");
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
                Some("jsonl") => {
                    captured.push_str(
                        &std::fs::read_to_string(entry.path()).expect("capture should be UTF-8"),
                    );
                }
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

fn padded_json(marker: &str, secret: &str, size: usize) -> Vec<u8> {
    let mut body = format!(r#"{{"marker":"{marker}","api_key":"{secret}"}}"#).into_bytes();
    assert!(body.len() <= size);
    body.resize(size, b' ');
    body
}

fn retry_response(status: u16, body: Body) -> reqwest::Response {
    reqwest::Response::from(
        axum::http::Response::builder()
            .status(status)
            .header("set-cookie", "private-cookie")
            .body(body)
            .expect("synthetic retry response"),
    )
}

fn unfinished_body(polls: Arc<AtomicUsize>, dropped: Arc<AtomicBool>) -> Body {
    let signal = DropSignal(dropped);
    Body::wrap_stream(async_stream::stream! {
        let _signal = signal;
        polls.fetch_add(1, Ordering::SeqCst);
        pending::<()>().await;
        yield Ok::<Bytes, Infallible>(Bytes::new());
    })
}

fn finite_body(
    payload: Vec<u8>,
    chunk_size: usize,
    consumed: Arc<AtomicUsize>,
    dropped: Arc<AtomicBool>,
) -> Body {
    let signal = DropSignal(dropped);
    Body::wrap_stream(async_stream::stream! {
        let _signal = signal;
        for chunk in payload.chunks(chunk_size) {
            consumed.fetch_add(chunk.len(), Ordering::SeqCst);
            yield Ok::<Bytes, Infallible>(Bytes::copy_from_slice(chunk));
        }
    })
}

fn failing_body(dropped: Arc<AtomicBool>) -> Body {
    let signal = DropSignal(dropped);
    Body::wrap_stream(async_stream::stream! {
        let _signal = signal;
        yield Ok::<Bytes, std::io::Error>(Bytes::from_static(
            br#"{"api_key":"read-error-secret","unfinished":"#,
        ));
        yield Err(std::io::Error::other("synthetic body failure"));
    })
}

fn progressing_body(progress: Arc<AtomicUsize>, dropped: Arc<AtomicBool>) -> Body {
    let signal = DropSignal(dropped);
    Body::wrap_stream(async_stream::stream! {
        let _signal = signal;
        loop {
            tokio::time::sleep(Duration::from_millis(50)).await;
            progress.fetch_add(1, Ordering::SeqCst);
            yield Ok::<Bytes, Infallible>(Bytes::from_static(b"timeout-secret"));
        }
    })
}

fn pending_body(dropped: Arc<AtomicBool>) -> Body {
    let signal = DropSignal(dropped);
    Body::wrap_stream(async_stream::stream! {
        let _signal = signal;
        pending::<()>().await;
        yield Ok::<Bytes, Infallible>(Bytes::new());
    })
}

#[tokio::test]
async fn non_retryable_http_responses_are_returned_immediately() {
    for status in [200, 400, 401, 403, 404, 422, 501] {
        let response = reqwest::Response::from(
            axum::http::Response::builder()
                .status(status)
                .body(Body::from("synthetic response"))
                .expect("synthetic response"),
        );
        assert!(matches!(classify_attempt(Ok(response), false, None).await,
            UpstreamAttempt::Complete(response) if response.status().as_u16() == status));
    }
}

#[test]
fn retry_after_accepts_delta_seconds_and_http_dates() {
    let mut headers = HeaderMap::new();
    headers.insert(RETRY_AFTER, HeaderValue::from_static("7"));
    assert_eq!(retry_after(&headers), Some(Duration::from_secs(7)));

    headers.insert(
        RETRY_AFTER,
        HeaderValue::from_static("Sun, 06 Nov 1994 08:49:37 GMT"),
    );
    assert_eq!(retry_after(&headers), Some(Duration::ZERO));

    headers.insert(RETRY_AFTER, HeaderValue::from_static("0"));
    assert_eq!(retry_after(&headers), Some(Duration::ZERO));
    headers.insert(
        RETRY_AFTER,
        HeaderValue::from_static("18446744073709551615"),
    );
    assert_eq!(retry_after(&headers), Some(Duration::from_secs(u64::MAX)));
    for invalid in ["nonsense", "-1", "18446744073709551616"] {
        headers.insert(RETRY_AFTER, HeaderValue::from_str(invalid).expect("header"));
        assert_eq!(retry_after(&headers), None);
    }
    let future = httpdate::fmt_http_date(std::time::SystemTime::now() + Duration::from_mins(2));
    headers.insert(RETRY_AFTER, HeaderValue::from_str(&future).expect("header"));
    let delay = retry_after(&headers).expect("future hint");
    assert!((Duration::from_secs(118)..=Duration::from_mins(2)).contains(&delay));
}

#[test]
fn fallback_retry_delays_scale_by_attempt_when_uncoordinated() {
    assert_eq!(
        fallback_delay(Some(Duration::from_secs(2)), 1, AttemptOutcome::RateLimited),
        Duration::from_secs(2)
    );
    assert_eq!(
        fallback_delay(None, 2, AttemptOutcome::ServerError),
        Duration::from_millis(500)
    );
}

#[tokio::test]
async fn initial_response_timeouts_retry_until_the_final_attempt() {
    let retry = classify_attempt(
        Err(ApiError::UpstreamTimeout(
            UpstreamTimeoutPhase::InitialResponse,
        )),
        false,
        None,
    )
    .await;
    assert!(matches!(retry, UpstreamAttempt::Retry));

    let failed = classify_attempt(
        Err(ApiError::UpstreamTimeout(
            UpstreamTimeoutPhase::InitialResponse,
        )),
        true,
        None,
    )
    .await;
    assert!(matches!(
        failed,
        UpstreamAttempt::Failed(ApiError::UpstreamTimeout(
            UpstreamTimeoutPhase::InitialResponse
        ))
    ));
}

#[test]
fn isolated_capture_child_forwards_profile_file_conditionally() {
    let directory = tempfile::tempdir().expect("private capture directory");
    let command = isolated_child_command(
        "test-name",
        "bounds",
        directory.path(),
        Some(OsStr::new("coverage/%p-%m.profraw")),
    );
    let profile_file = command
        .as_std()
        .get_envs()
        .find(|(name, _)| *name == OsStr::new(PROFILE_FILE))
        .and_then(|(_, value)| value);
    assert_eq!(profile_file, Some(OsStr::new("coverage/%p-%m.profraw")));

    let command = isolated_child_command("test-name", "bounds", directory.path(), None);
    let profile_file = command
        .as_std()
        .get_envs()
        .find(|(name, _)| *name == OsStr::new(PROFILE_FILE))
        .and_then(|(_, value)| value);
    assert_eq!(profile_file, None);
}
