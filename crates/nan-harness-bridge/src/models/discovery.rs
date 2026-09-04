use super::parsing::{
    parse_models_response, read_bounded_models_response, read_discovery_error_prefix,
};
use crate::error::BridgeError;
use crate::upstream_capture::record_response_metadata;
use nan_harness_coordinator::{
    AttemptOutcome, CaptureLeg, CaptureSink, CoordinatorClient, EndpointKind, RetryDirective,
};
use nan_harness_core::{CodingModelProfile, SecretValue, coding_models_from_provider_ids};
use reqwest::header::{ACCEPT, RETRY_AFTER};
use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

const DISCOVERY_BUDGET: Duration = Duration::from_secs(45);
const DISCOVERY_ATTEMPTS: u8 = 3;

enum DiscoveryAttempt {
    Complete(BTreeSet<String>),
    Retry {
        outcome: AttemptOutcome,
        retry_after: Option<Duration>,
    },
    Failed {
        error: BridgeError,
        outcome: AttemptOutcome,
    },
}

/// Discovers and classifies the conversational models available to one NaN credential.
///
/// Known non-conversational endpoints are removed. Unknown IDs remain available with
/// conservative metadata so newly released text models work before the next harness release.
///
/// # Errors
///
/// Returns [`BridgeError`] when the model endpoint cannot be queried or decoded.
pub async fn discover_coding_models(
    provider_base_url: &str,
    provider_api_key: Arc<SecretValue>,
) -> Result<Vec<CodingModelProfile>, BridgeError> {
    let provider_ids = discover_provider_ids(provider_base_url, provider_api_key).await?;
    Ok(coding_models_from_provider_ids(provider_ids))
}

async fn discover_provider_ids(
    provider_base_url: &str,
    provider_api_key: Arc<SecretValue>,
) -> Result<BTreeSet<String>, BridgeError> {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .build()
        .map_err(BridgeError::BuildClient)?;
    let endpoint = format!("{}/models", provider_base_url.trim_end_matches('/'));
    let (coordinator, capture) = request_support(provider_base_url, &provider_api_key);
    let capture = capture.begin_request(format!("models_{}", std::process::id()));
    if let Some(capture) = &capture {
        capture.record(CaptureLeg::ProviderRequest, b"");
    }
    let started = Instant::now();
    for attempt in 1..=DISCOVERY_ATTEMPTS {
        let remaining = DISCOVERY_BUDGET.saturating_sub(started.elapsed());
        let mut lease = match &coordinator {
            Some(coordinator) => {
                coordinator
                    .acquire(EndpointKind::Models, None, remaining)
                    .await?
            }
            None => None,
        };
        match discovery_attempt(
            &client,
            &endpoint,
            &provider_api_key,
            remaining,
            attempt == DISCOVERY_ATTEMPTS,
            capture.as_ref(),
        )
        .await
        {
            DiscoveryAttempt::Complete(models) => {
                observe(&mut lease, AttemptOutcome::Success).await;
                return Ok(models);
            }
            DiscoveryAttempt::Retry {
                outcome,
                retry_after,
            } => {
                wait_for_retry_with_hint(&mut lease, outcome, retry_after, attempt, started).await;
            }
            DiscoveryAttempt::Failed { error, outcome } => {
                observe(&mut lease, outcome).await;
                return Err(error);
            }
        }
    }
    unreachable!("bounded discovery retry loop always returns")
}

async fn discovery_attempt(
    client: &reqwest::Client,
    endpoint: &str,
    provider_api_key: &SecretValue,
    remaining: Duration,
    final_attempt: bool,
    capture: Option<&nan_harness_coordinator::CaptureRequest>,
) -> DiscoveryAttempt {
    let mut response = match send_request(client, endpoint, provider_api_key, remaining).await {
        Ok(response) => response,
        Err(error) if !final_attempt => {
            return DiscoveryAttempt::Retry {
                outcome: transport_outcome(&error),
                retry_after: None,
            };
        }
        Err(error) => {
            return DiscoveryAttempt::Failed {
                outcome: transport_outcome(&error),
                error: BridgeError::ModelDiscoveryTransport(error),
            };
        }
    };
    record_response_metadata(capture, &response);
    if !response.status().is_success() {
        return classify_status_response(&mut response, final_attempt, capture).await;
    }
    let body = match read_bounded_models_response(&mut response).await {
        Ok(body) => body,
        Err(BridgeError::ModelDiscoveryTransport(error)) if !final_attempt => {
            return DiscoveryAttempt::Retry {
                outcome: transport_outcome(&error),
                retry_after: None,
            };
        }
        Err(error) => {
            return DiscoveryAttempt::Failed {
                error,
                outcome: AttemptOutcome::Terminal,
            };
        }
    };
    if let Some(capture) = capture {
        capture.record(CaptureLeg::ProviderResponse, &body);
    }
    match parse_models_response(&body) {
        Ok(models) => DiscoveryAttempt::Complete(models),
        Err(_) if !final_attempt => DiscoveryAttempt::Retry {
            outcome: AttemptOutcome::InvalidResponse,
            retry_after: None,
        },
        Err(error) => DiscoveryAttempt::Failed {
            error: BridgeError::InvalidModelDiscoveryResponse(error),
            outcome: AttemptOutcome::Terminal,
        },
    }
}

async fn classify_status_response(
    response: &mut reqwest::Response,
    final_attempt: bool,
    capture: Option<&nan_harness_coordinator::CaptureRequest>,
) -> DiscoveryAttempt {
    let status = response.status();
    let retry_after = retry_after(response.headers());
    let message = read_discovery_error_prefix(response).await;
    if let Some(capture) = capture {
        capture.record(CaptureLeg::ProviderResponse, message.as_bytes());
    }
    if retryable_status(status) && !final_attempt {
        let outcome = if status.as_u16() == 429 {
            AttemptOutcome::RateLimited
        } else {
            AttemptOutcome::ServerError
        };
        DiscoveryAttempt::Retry {
            outcome,
            retry_after,
        }
    } else {
        DiscoveryAttempt::Failed {
            error: BridgeError::ModelDiscoveryStatus { status, message },
            outcome: AttemptOutcome::Terminal,
        }
    }
}

fn request_support(
    provider_base_url: &str,
    provider_api_key: &SecretValue,
) -> (Option<CoordinatorClient>, CaptureSink) {
    let launch_id = format!("model_discovery_{}", std::process::id());
    (
        CoordinatorClient::new(provider_base_url, provider_api_key, &launch_id),
        CaptureSink::new(launch_id),
    )
}

fn transport_outcome(error: &reqwest::Error) -> AttemptOutcome {
    if error.is_timeout() {
        AttemptOutcome::Timeout
    } else {
        AttemptOutcome::Transport
    }
}

async fn send_request(
    client: &reqwest::Client,
    endpoint: &str,
    provider_api_key: &SecretValue,
    remaining: Duration,
) -> Result<reqwest::Response, reqwest::Error> {
    let request = provider_api_key.with_secret(|api_key| {
        client
            .get(endpoint)
            .header(ACCEPT, "application/json")
            .bearer_auth(api_key)
            .timeout(remaining.max(Duration::from_millis(1)))
    });
    request.send().await
}

async fn wait_for_retry_with_hint(
    lease: &mut Option<nan_harness_coordinator::RequestLease>,
    outcome: AttemptOutcome,
    retry_after: Option<Duration>,
    attempt: u8,
    started: Instant,
) {
    let delay = if let Some(lease) = lease {
        match lease.observe(outcome, retry_after).await {
            RetryDirective::RetryAfter(delay) => delay,
            RetryDirective::Complete => fallback_delay(retry_after, attempt),
        }
    } else {
        fallback_delay(retry_after, attempt)
    };
    let remaining = DISCOVERY_BUDGET.saturating_sub(started.elapsed());
    if delay < remaining {
        tokio::time::sleep(delay).await;
    }
}

fn fallback_delay(retry_after: Option<Duration>, attempt: u8) -> Duration {
    retry_after.unwrap_or_else(|| Duration::from_millis(250 * u64::from(attempt)))
}

async fn observe(
    lease: &mut Option<nan_harness_coordinator::RequestLease>,
    outcome: AttemptOutcome,
) {
    if let Some(lease) = lease {
        let _ = lease.observe(outcome, None).await;
    }
}

fn retryable_status(status: reqwest::StatusCode) -> bool {
    matches!(status.as_u16(), 408 | 425 | 429 | 500 | 502..=504)
}

fn retry_after(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    let value = headers.get(RETRY_AFTER)?.to_str().ok()?;
    value
        .parse::<u64>()
        .ok()
        .map(Duration::from_secs)
        .or_else(|| {
            httpdate::parse_http_date(value).ok().map(|deadline| {
                deadline
                    .duration_since(SystemTime::now())
                    .unwrap_or_default()
            })
        })
}

#[cfg(test)]
mod tests {
    use super::super::parsing::MAX_MODELS_RESPONSE_BYTES;
    use super::discover_coding_models;
    use crate::BridgeError;
    use axum::Router;
    use axum::body::{Body, Bytes};
    use axum::extract::State;
    use axum::http::{HeaderValue, Response, StatusCode, header};
    use axum::routing::get;
    use nan_harness_core::SecretValue;
    use std::convert::Infallible;
    use std::sync::Arc;

    #[derive(Clone)]
    struct CatalogResponse {
        status: StatusCode,
        chunks: Vec<Bytes>,
        content_length: Option<u64>,
    }

    async fn catalog_response(State(response): State<CatalogResponse>) -> Response<Body> {
        let stream =
            futures_util::stream::iter(response.chunks.into_iter().map(Ok::<Bytes, Infallible>));
        let mut result = Response::new(Body::from_stream(stream));
        *result.status_mut() = response.status;
        if let Some(content_length) = response.content_length {
            result.headers_mut().insert(
                header::CONTENT_LENGTH,
                HeaderValue::from_str(&content_length.to_string())
                    .expect("test content length should be valid"),
            );
        }
        result
    }

    async fn discover_from(response: CatalogResponse) -> Result<Vec<String>, BridgeError> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("test provider should bind");
        let address = listener.local_addr().expect("test provider address");
        let app = Router::new()
            .route("/v1/models", get(catalog_response))
            .with_state(response);
        let task = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("test provider should serve");
        });
        let result = discover_coding_models(
            &format!("http://{address}/v1"),
            Arc::new(SecretValue::new("test-key").expect("test key should be valid")),
        )
        .await
        .map(|models| models.into_iter().map(|model| model.id).collect());
        task.abort();
        result
    }

    fn padded_catalog(size: usize) -> Vec<u8> {
        let mut body = br#"{"data":[{"id":"qwen3.6"}]}"#.to_vec();
        assert!(body.len() <= size, "requested test body is too small");
        body.resize(size, b' ');
        body
    }

    #[tokio::test]
    async fn discovery_bounds_success_and_error_bodies() {
        let small = padded_catalog(64);
        assert_eq!(
            discover_from(CatalogResponse {
                status: StatusCode::OK,
                chunks: vec![Bytes::from(small.clone())],
                content_length: Some(small.len() as u64),
            })
            .await
            .expect("small catalog should be accepted"),
            ["qwen3.6"]
        );

        let declared = discover_from(CatalogResponse {
            status: StatusCode::OK,
            chunks: vec![Bytes::from(padded_catalog(MAX_MODELS_RESPONSE_BYTES + 1))],
            content_length: Some((MAX_MODELS_RESPONSE_BYTES + 1) as u64),
        })
        .await
        .expect_err("oversized declared catalog should be rejected");
        assert!(matches!(declared, BridgeError::ModelDiscoveryTooLarge));

        let oversized = padded_catalog(MAX_MODELS_RESPONSE_BYTES + 1);
        let chunked = discover_from(CatalogResponse {
            status: StatusCode::OK,
            chunks: vec![
                Bytes::copy_from_slice(&oversized[..MAX_MODELS_RESPONSE_BYTES]),
                Bytes::copy_from_slice(&oversized[MAX_MODELS_RESPONSE_BYTES..]),
            ],
            content_length: None,
        })
        .await
        .expect_err("oversized chunked catalog should be rejected");
        assert!(matches!(chunked, BridgeError::ModelDiscoveryTooLarge));

        let invalid = discover_from(CatalogResponse {
            status: StatusCode::OK,
            chunks: vec![Bytes::from_static(b"not-json")],
            content_length: Some(8),
        })
        .await
        .expect_err("invalid catalog should be rejected");
        assert!(matches!(
            invalid,
            BridgeError::InvalidModelDiscoveryResponse(_)
        ));

        let boundary = padded_catalog(MAX_MODELS_RESPONSE_BYTES);
        assert_eq!(
            discover_from(CatalogResponse {
                status: StatusCode::OK,
                chunks: vec![Bytes::from(boundary)],
                content_length: Some(MAX_MODELS_RESPONSE_BYTES as u64),
            })
            .await
            .expect("catalog at the exact boundary should be accepted"),
            ["qwen3.6"]
        );

        let mut status_body = br#"{"message":"bounded status"}"#.to_vec();
        status_body.resize(128 * 1024, b' ');
        let status = discover_from(CatalogResponse {
            status: StatusCode::BAD_GATEWAY,
            chunks: vec![Bytes::from(status_body)],
            content_length: Some(128 * 1024),
        })
        .await
        .expect_err("non-success response should remain a status error");
        assert!(matches!(
            status,
            BridgeError::ModelDiscoveryStatus {
                status: StatusCode::BAD_GATEWAY,
                ref message,
            } if message == "bounded status"
        ));
    }
}
