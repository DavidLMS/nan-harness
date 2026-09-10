use super::*;
use axum::Router;
use axum::body::Body;
use axum::http::Response;
use axum::routing::post;
use std::sync::atomic::AtomicUsize;
use tokio::net::TcpListener;

mod coordinated;

const CHILD_SCENARIO: &str = "NAN_TEST_RETRY_WAIT_UNCOORDINATED";

async fn run_in_isolated_child(test_name: &str) -> bool {
    if std::env::var(CHILD_SCENARIO).is_ok_and(|scenario| scenario == test_name) {
        return false;
    }
    let directory = tempfile::tempdir().expect("isolated retry configuration");
    let mut command =
        tokio::process::Command::new(std::env::current_exe().expect("test executable"));
    command
        .args([
            "--exact",
            &format!("upstream::retry_tests::{test_name}"),
            "--nocapture",
        ])
        .env_clear()
        .env(CHILD_SCENARIO, test_name)
        .env("NAN_HARNESS_CONFIG_DIR", directory.path())
        .env("NAN_HARNESS_INTERNAL_MANAGED_PROCESS", "1")
        .kill_on_drop(true);
    if let Some(profile) = std::env::var_os("LLVM_PROFILE_FILE") {
        command.env("LLVM_PROFILE_FILE", profile);
    }
    let result = tokio::time::timeout(Duration::from_secs(20), command.status())
        .await
        .expect("isolated retry test deadline")
        .expect("child status");
    assert!(result.success());
    true
}

async fn provider(
    status: u16,
    hint: &str,
    payload: &'static str,
) -> (NanClient, Arc<AtomicUsize>, tokio::task::JoinHandle<()>) {
    let sends = Arc::new(AtomicUsize::new(0));
    let attempts = Arc::clone(&sends);
    let hint = hint.to_owned();
    let app = Router::new().route(
        "/",
        post(move || {
            attempts.fetch_add(1, Ordering::SeqCst);
            let hint = hint.clone();
            async move {
                Response::builder()
                    .status(status)
                    .header("retry-after", hint)
                    .body(Body::from(payload))
                    .expect("response")
            }
        }),
    );
    let (client, task) = provider_app(app).await;
    (client, sends, task)
}

async fn provider_app(app: Router) -> (NanClient, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
    let endpoint = format!("http://{}/", listener.local_addr().expect("address"));
    let task = tokio::spawn(async move { axum::serve(listener, app).await.expect("provider") });
    let client = NanClient {
        client: reqwest::Client::new(),
        chat_endpoint: endpoint.clone(),
        search_endpoint: endpoint,
        api_key: Arc::new(SecretValue::new("synthetic-key").expect("secret")),
        coordinator: None,
        session_budget_enabled: false,
        capture: CaptureSink::new("retry-wait-test"),
        next_request_id: Arc::new(AtomicU64::new(1)),
    };
    (client, task)
}

#[tokio::test]
async fn retry_excessive_hints_return_known_status_without_a_second_send() {
    if run_in_isolated_child("retry_excessive_hints_return_known_status_without_a_second_send")
        .await
    {
        return;
    }
    let future = httpdate::fmt_http_date(std::time::SystemTime::now() + Duration::from_hours(1));
    for status in [429, 503] {
        for hint in [
            if status == 429 { "121" } else { "46" },
            "18446744073709551615",
            future.as_str(),
        ] {
            let (client, sends, task) = provider(status, hint, "synthetic failure").await;
            let response =
                tokio::time::timeout(Duration::from_secs(1), client.send(&Value::Null, b"{}"))
                    .await
                    .expect("oversized hint must return promptly")
                    .expect("known response");
            assert_eq!(response.status().as_u16(), status);
            assert!(matches!(response.read_final_error_body().await,
                FinalErrorBody::Complete(body) if body == "synthetic failure"));
            assert_eq!(sends.load(Ordering::SeqCst), 1);
            task.abort();
        }
    }
}

#[tokio::test]
async fn retry_zero_hint_preserves_the_attempt_limit() {
    if run_in_isolated_child("retry_zero_hint_preserves_the_attempt_limit").await {
        return;
    }
    let (client, sends, task) = provider(429, "0", "failure").await;
    let response = client.send(&Value::Null, b"{}").await.expect("response");
    assert_eq!(response.status().as_u16(), 429);
    assert_eq!(sends.load(Ordering::SeqCst), 3);
    task.abort();
}

#[tokio::test]
async fn retry_pause_cancellation_does_not_send_again() {
    if run_in_isolated_child("retry_pause_cancellation_does_not_send_again").await {
        return;
    }
    let (client, sends, task) = provider(429, "30", "failure").await;
    let result =
        tokio::time::timeout(Duration::from_millis(100), client.send(&Value::Null, b"{}")).await;
    assert!(result.is_err());
    assert_eq!(sends.load(Ordering::SeqCst), 1);
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(sends.load(Ordering::SeqCst), 1);
    task.abort();
}

#[tokio::test]
async fn retry_budget_is_cumulative_across_send_calls() {
    if run_in_isolated_child("retry_budget_is_cumulative_across_send_calls").await {
        return;
    }
    let (client, sends, task) = provider(503, "1", "failure").await;
    let capture = client.begin_capture(b"{}");
    let mut budget = SendBudget::new(8);
    assert!(budget.reserve_retry_wait(Duration::from_secs(44)));
    let response = client
        .send_with_priority(
            &Value::Null,
            RequestPriority::Foreground,
            RequestCache::Default,
            &capture,
            &mut budget,
        )
        .await
        .expect("response");
    assert_eq!(response.status().as_u16(), 503);
    assert_eq!(sends.load(Ordering::SeqCst), 2);
    let response = client
        .send_with_priority(
            &Value::Null,
            RequestPriority::Foreground,
            RequestCache::Bypass,
            &capture,
            &mut budget,
        )
        .await
        .expect("response");
    assert_eq!(response.status().as_u16(), 503);
    assert_eq!(sends.load(Ordering::SeqCst), 3);
    task.abort();
}

#[test]
fn retry_budget_reserves_whole_pauses_and_preserves_the_remainder() {
    let mut budget = SendBudget::new(8);
    assert!(budget.reserve_retry_wait(Duration::from_secs(20)));
    assert!(!budget.reserve_retry_wait(Duration::from_secs(26)));
    assert!(!budget.reserve_retry_wait(Duration::MAX));
    assert!(budget.reserve_retry_wait(Duration::from_secs(25)));
    assert!(budget.reserve_retry_wait(Duration::ZERO));
    assert!(!budget.reserve_retry_wait(Duration::from_nanos(1)));
    assert!(!budget.is_exhausted());
}

#[tokio::test]
async fn retry_budget_preserves_bounded_final_error_body_handling() {
    for status in [429, 503] {
        let response = reqwest::Response::from(
            Response::builder()
                .status(status)
                .header("retry-after", "121")
                .body(reqwest::Body::from(vec![b'x'; 64 * 1024 + 1]))
                .expect("response"),
        );
        let mut lease = RetryLease::new(None);
        let mut budget = SendBudget::new(3);
        let UpstreamAttempt::Complete(response) = lease
            .finish_attempt(Ok(response), false, None, 1, &mut budget)
            .await
        else {
            panic!("excessive hint must preserve response");
        };
        let response = UpstreamResponse::uncoordinated(response);
        assert_eq!(response.status().as_u16(), status);
        assert!(matches!(
            response.read_final_error_body().await,
            FinalErrorBody::Incomplete
        ));
    }
}

#[tokio::test]
async fn retry_budget_does_not_charge_successful_response_time() {
    if run_in_isolated_child("retry_budget_does_not_charge_successful_response_time").await {
        return;
    }
    let (client, _, task) = provider(200, "0", "healthy response").await;
    let capture = client.begin_capture(b"{}");
    let mut budget = SendBudget::new(8);
    let response = client
        .send_with_priority(
            &Value::Null,
            RequestPriority::Foreground,
            RequestCache::Default,
            &capture,
            &mut budget,
        )
        .await
        .expect("healthy response");
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(response.status().as_u16(), 200);
    assert!(budget.reserve_retry_wait(Duration::from_secs(45)));
    task.abort();
}

#[test]
fn mixed_retry_waits_preserve_both_allowances_without_partial_reservations() {
    for rate_limit_first in [false, true] {
        let mut budget = SendBudget::new(8);
        if rate_limit_first {
            assert!(budget.reserve_wait(Duration::from_secs(75), AttemptOutcome::RateLimited));
        }
        assert!(budget.reserve_retry_wait(Duration::from_secs(44)));
        assert!(!budget.reserve_retry_wait(Duration::from_secs(2)));
        if !rate_limit_first {
            assert!(budget.reserve_wait(Duration::from_secs(75), AttemptOutcome::RateLimited));
        }
        assert!(!budget.reserve_wait(Duration::from_secs(2), AttemptOutcome::RateLimited));
        assert!(budget.reserve_retry_wait(Duration::from_secs(1)));
        assert!(!budget.reserve_wait(Duration::from_nanos(1), AttemptOutcome::RateLimited));
        assert!(!budget.reserve_retry_wait(Duration::from_nanos(1)));
        assert!(budget.reserve_wait(Duration::ZERO, AttemptOutcome::RateLimited));
    }
}

fn synthetic_quota_response(status: u16, hint: Option<&str>) -> reqwest::Response {
    let mut response = Response::builder().status(status);
    if let Some(hint) = hint {
        response = response.header("retry-after", hint);
    }
    reqwest::Response::from(
        response
            .body(reqwest::Body::from("synthetic quota response"))
            .expect("response"),
    )
}

#[tokio::test(start_paused = true)]
async fn hintless_quota_recovers_after_ten_seconds_within_the_same_send_budget() {
    let started = tokio::time::Instant::now();
    let mut budget = SendBudget::new(3);
    let mut sends = 0;
    loop {
        budget.consume().expect("send allowance");
        sends += 1;
        let status = if started.elapsed() < Duration::from_secs(10) {
            429
        } else {
            200
        };
        let result = RetryLease::new(None)
            .finish_attempt(
                Ok(synthetic_quota_response(status, None)),
                budget.is_exhausted(),
                None,
                sends,
                &mut budget,
            )
            .await;
        match result {
            UpstreamAttempt::Retry => {}
            UpstreamAttempt::Complete(response) => {
                assert_eq!(response.status(), 200);
                break;
            }
            UpstreamAttempt::Failed(error) => panic!("unexpected error: {error}"),
        }
    }
    assert_eq!(sends, 2);
    assert!((Duration::from_secs(15)..=Duration::from_secs(20)).contains(&started.elapsed()));
    assert_eq!(budget.remaining, 1);
}

#[tokio::test(start_paused = true)]
async fn persistent_hintless_quota_returns_original_response_without_final_sleep() {
    let started = tokio::time::Instant::now();
    let mut budget = SendBudget::new(3);
    for attempt in 1..=MAX_ATTEMPTS {
        budget.consume().expect("send allowance");
        let before = tokio::time::Instant::now();
        let result = RetryLease::new(None)
            .finish_attempt(
                Ok(synthetic_quota_response(429, None)),
                budget.is_exhausted(),
                None,
                attempt,
                &mut budget,
            )
            .await;
        if attempt < MAX_ATTEMPTS {
            assert!(matches!(result, UpstreamAttempt::Retry));
        } else {
            let UpstreamAttempt::Complete(response) = result else {
                panic!("original response")
            };
            assert_eq!(response.status(), 429);
            assert_eq!(
                response.text().await.expect("body"),
                "synthetic quota response"
            );
            assert_eq!(before.elapsed(), Duration::ZERO);
        }
    }
    assert!((Duration::from_secs(45)..=Duration::from_mins(1)).contains(&started.elapsed()));
    assert!(budget.is_exhausted());
}

#[tokio::test(start_paused = true)]
async fn quota_hints_can_use_120_seconds_but_never_unlock_other_waits() {
    let mut budget = SendBudget::new(8);
    for (status, hint, expected_retry) in [
        (503, "45", true),
        (429, "75", true),
        (429, "1", false),
        (503, "1", false),
    ] {
        let started = tokio::time::Instant::now();
        let result = RetryLease::new(None)
            .finish_attempt(
                Ok(synthetic_quota_response(status, Some(hint))),
                false,
                None,
                1,
                &mut budget,
            )
            .await;
        assert_eq!(matches!(result, UpstreamAttempt::Retry), expected_retry);
        assert_eq!(
            started.elapsed().as_secs(),
            if expected_retry {
                hint.parse().expect("seconds")
            } else {
                0
            }
        );
    }
}

#[tokio::test]
async fn uncoordinated_http_request_recovers_after_quota_window() {
    if run_in_isolated_child("uncoordinated_http_request_recovers_after_quota_window").await {
        return;
    }
    let started = tokio::time::Instant::now();
    let sends = Arc::new(AtomicUsize::new(0));
    let attempts = Arc::clone(&sends);
    let app = Router::new().route(
        "/",
        post(move || {
            attempts.fetch_add(1, Ordering::SeqCst);
            async move {
                let status = if started.elapsed() < Duration::from_secs(10) {
                    429
                } else {
                    200
                };
                Response::builder()
                    .status(status)
                    .body(Body::from("synthetic result"))
                    .expect("response")
            }
        }),
    );
    let (client, task) = provider_app(app).await;
    tokio::time::pause();
    let response = tokio::select! {
        result = client.send(&Value::Null, b"{}") => result.expect("response"),
        () = advance_with_io() => panic!("request exceeded virtual deadline"),
    };
    assert_eq!(response.status(), 200);
    assert_eq!(sends.load(Ordering::SeqCst), 2);
    assert!(started.elapsed() >= Duration::from_secs(15));
    task.abort();
}

// Keep virtual time from auto-jumping to a network timeout while loopback I/O
// becomes ready. Each tick gives the real socket reactor time to make progress.
async fn advance_with_io() {
    for _ in 0..120 {
        for _ in 0..1_000 {
            tokio::task::yield_now().await;
        }
        tokio::time::advance(Duration::from_secs(1)).await;
    }
}
