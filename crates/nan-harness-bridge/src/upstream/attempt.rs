use crate::error::ApiError;
use nan_harness_coordinator::{AttemptOutcome, CaptureRequest, RequestLease, RetryDirective};
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
                crate::upstream_capture::handle_retry_response_body(capture, response).await;
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

const fn status_outcome(status: reqwest::StatusCode) -> AttemptOutcome {
    if status.as_u16() == 429 {
        AttemptOutcome::RateLimited
    } else {
        AttemptOutcome::ServerError
    }
}

fn retryable_status(status: reqwest::StatusCode) -> bool {
    matches!(status.as_u16(), 408 | 425 | 429 | 500 | 502..=504)
}

fn is_retryable(error: &ApiError) -> bool {
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
mod tests;
