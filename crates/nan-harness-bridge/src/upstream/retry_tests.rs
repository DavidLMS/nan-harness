use super::*;
use axum::Router;
use axum::body::Body;
use axum::http::Response;
use axum::routing::post;
use std::sync::atomic::AtomicUsize;
use tokio::net::TcpListener;

mod coordinated;

async fn provider(
    status: u16,
    hint: &str,
    payload: &'static str,
) -> (NanClient, Arc<AtomicUsize>, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
    let endpoint = format!("http://{}/", listener.local_addr().expect("address"));
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
    let task = tokio::spawn(async move { axum::serve(listener, app).await.expect("provider") });
    let client = NanClient {
        client: reqwest::Client::new(),
        chat_endpoint: endpoint.clone(),
        search_endpoint: endpoint,
        api_key: Arc::new(SecretValue::new("synthetic-key").expect("secret")),
        coordinator: None,
        capture: CaptureSink::new("retry-wait-test"),
        next_request_id: Arc::new(AtomicU64::new(1)),
    };
    (client, sends, task)
}

#[tokio::test]
async fn retry_excessive_hints_return_known_status_without_a_second_send() {
    let future = httpdate::fmt_http_date(std::time::SystemTime::now() + Duration::from_hours(1));
    for status in [429, 503] {
        for hint in ["46", "18446744073709551615", future.as_str()] {
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
    let (client, sends, task) = provider(429, "0", "failure").await;
    let response = client.send(&Value::Null, b"{}").await.expect("response");
    assert_eq!(response.status().as_u16(), 429);
    assert_eq!(sends.load(Ordering::SeqCst), 3);
    task.abort();
}

#[tokio::test]
async fn retry_pause_cancellation_does_not_send_again() {
    let (client, sends, task) = provider(503, "30", "failure").await;
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
                .header("retry-after", "46")
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
