use crate::error::ApiError;
use nan_harness_coordinator::{
    AttemptOutcome, CaptureLeg, CaptureRequest, RequestLease, RetryDirective,
};
use reqwest::header::RETRY_AFTER;
use std::time::{Duration, SystemTime};

const RETRY_FALLBACK_BASE_MS: u64 = 250;

pub(crate) struct RetryLease {
    lease: Option<RequestLease>,
}

impl RetryLease {
    pub(crate) const fn new(lease: Option<RequestLease>) -> Self {
        Self { lease }
    }

    pub(crate) async fn headers_received(&mut self, elapsed: Duration) {
        if let Some(lease) = &mut self.lease {
            lease.headers_received(elapsed).await;
        }
    }

    pub(crate) async fn observe(&mut self, outcome: AttemptOutcome) {
        if let Some(lease) = &mut self.lease {
            let _ = lease.observe(outcome, None).await;
        }
    }

    pub(crate) async fn observe_error(&mut self, error: &ApiError) {
        let outcome = if is_retryable(error) {
            retryable_error_outcome(error)
        } else {
            AttemptOutcome::Terminal
        };
        self.observe(outcome).await;
    }

    pub(crate) fn into_inner(self) -> Option<RequestLease> {
        self.lease
    }

    pub(crate) async fn delay_for_retry(
        &mut self,
        outcome: AttemptOutcome,
        retry_after: Option<Duration>,
        attempt: u8,
    ) -> Duration {
        if let Some(lease) = &mut self.lease
            && let RetryDirective::RetryAfter(delay) = lease.observe(outcome, retry_after).await
        {
            return delay;
        }
        fallback_delay(retry_after, attempt)
    }
}

fn fallback_delay(retry_after: Option<Duration>, attempt: u8) -> Duration {
    retry_after
        .unwrap_or_else(|| Duration::from_millis(RETRY_FALLBACK_BASE_MS * u64::from(attempt)))
}

pub(crate) enum UpstreamAttempt {
    Complete(reqwest::Response),
    Retry {
        outcome: AttemptOutcome,
        retry_after: Option<Duration>,
    },
    Failed(ApiError),
}

pub(crate) async fn classify_attempt(
    result: Result<reqwest::Response, ApiError>,
    final_attempt: bool,
    capture: Option<&CaptureRequest>,
) -> UpstreamAttempt {
    match result {
        Ok(response) => {
            crate::upstream_capture::record_response_metadata(capture, &response);
            if retryable_status(response.status()) && !final_attempt {
                let retry_after = retry_after(response.headers());
                let outcome = status_outcome(response.status());
                if let Ok(payload) = response.bytes().await {
                    crate::upstream_capture::record_payload(
                        capture,
                        CaptureLeg::ProviderResponse,
                        &payload,
                    );
                }
                UpstreamAttempt::Retry {
                    outcome,
                    retry_after,
                }
            } else {
                UpstreamAttempt::Complete(response)
            }
        }
        Err(error) if is_retryable(&error) && !final_attempt => UpstreamAttempt::Retry {
            outcome: retryable_error_outcome(&error),
            retry_after: None,
        },
        Err(error) => UpstreamAttempt::Failed(error),
    }
}

pub(crate) fn retry_after(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
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

const fn status_outcome(status: reqwest::StatusCode) -> AttemptOutcome {
    if status.as_u16() == 429 {
        AttemptOutcome::RateLimited
    } else {
        AttemptOutcome::ServerError
    }
}

pub(crate) fn retryable_status(status: reqwest::StatusCode) -> bool {
    matches!(status.as_u16(), 408 | 425 | 429 | 500 | 502..=504)
}

pub(crate) fn is_retryable(error: &ApiError) -> bool {
    matches!(
        error,
        ApiError::UpstreamTransport(_) | ApiError::UpstreamTimeout(_)
    )
}

const fn retryable_error_outcome(error: &ApiError) -> AttemptOutcome {
    match error {
        ApiError::UpstreamTimeout(_) => AttemptOutcome::Timeout,
        _ => AttemptOutcome::Transport,
    }
}

#[cfg(test)]
mod tests {
    use super::{UpstreamAttempt, classify_attempt, fallback_delay, retry_after};
    use crate::error::{ApiError, UpstreamTimeoutPhase};
    use nan_harness_coordinator::AttemptOutcome;
    use reqwest::header::{HeaderMap, HeaderValue, RETRY_AFTER};
    use std::time::Duration;

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
    }

    #[test]
    fn fallback_retry_delays_scale_by_attempt_when_uncoordinated() {
        assert_eq!(
            fallback_delay(Some(Duration::from_secs(2)), 1),
            Duration::from_secs(2)
        );
        assert_eq!(fallback_delay(None, 2), Duration::from_millis(500));
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
        assert!(matches!(
            retry,
            UpstreamAttempt::Retry {
                outcome: AttemptOutcome::Timeout,
                retry_after: None,
            }
        ));

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
}
