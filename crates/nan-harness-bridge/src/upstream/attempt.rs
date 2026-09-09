use crate::error::ApiError;
use nan_harness_coordinator::{AttemptOutcome, CaptureRequest, RequestLease, RetryDirective};
use reqwest::header::RETRY_AFTER;
use std::time::{Duration, SystemTime};

const RETRY_FALLBACK_BASE_MS: u64 = 250;

pub(crate) struct RetryLease {
    lease: Option<RequestLease>,
}

impl RetryLease {
    pub(crate) async fn finish_attempt(
        &mut self,
        result: Result<reqwest::Response, ApiError>,
        final_attempt: bool,
        capture: Option<&CaptureRequest>,
        attempt: u8,
        budget: &mut super::SendBudget,
    ) -> UpstreamAttempt {
        let retry = match &result {
            Ok(response) if retryable_status(response.status()) => Some((
                status_outcome(response.status()),
                retry_after(response.headers()),
            )),
            Err(error) if is_retryable(error) => Some((retryable_error_outcome(error), None)),
            _ => None,
        };
        let Some((outcome, hint)) = retry else {
            return classify_attempt(result, final_attempt, capture).await;
        };
        // Observe even when no local retry fits: other requests must honor the
        // provider cooldown, and the original response must remain available.
        let delay = self.delay_for_retry(outcome, hint, attempt).await;
        let retry_allowed = !final_attempt && budget.reserve_wait(delay, outcome);
        let classified = classify_attempt(result, !retry_allowed, capture).await;
        if retry_allowed {
            tokio::time::sleep(delay).await;
        }
        classified
    }

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
        // The coordinator wire format truncates to milliseconds. Round hints
        // upward first so a shared cooldown cannot expire before the hint.
        let coordinator_hint =
            retry_after.map(|delay| delay.saturating_add(Duration::from_nanos(999_999)));
        if let Some(lease) = &mut self.lease
            && let RetryDirective::RetryAfter(delay) =
                lease.observe(outcome, coordinator_hint).await
        {
            return delay.max(retry_after.unwrap_or_default());
        }
        fallback_delay(retry_after, attempt, outcome)
    }
}

fn fallback_delay(retry_after: Option<Duration>, attempt: u8, outcome: AttemptOutcome) -> Duration {
    retry_after.unwrap_or_else(|| {
        if outcome == AttemptOutcome::RateLimited {
            nan_harness_coordinator::rate_limit_backoff(attempt)
        } else {
            Duration::from_millis(RETRY_FALLBACK_BASE_MS * u64::from(attempt))
        }
    })
}

pub(crate) enum UpstreamAttempt {
    Complete(reqwest::Response),
    Retry,
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
                crate::upstream_capture::handle_retry_response_body(capture, response).await;
                UpstreamAttempt::Retry
            } else {
                UpstreamAttempt::Complete(response)
            }
        }
        Err(error) if is_retryable(&error) && !final_attempt => UpstreamAttempt::Retry,
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
