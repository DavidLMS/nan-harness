use crate::error::ApiError;
use crate::timeouts::{STREAM_INACTIVITY_TIMEOUT, with_inactivity_timeout};
use async_stream::stream;
use bytes::Bytes;
use futures_util::{Stream, StreamExt as _};
use nan_harness_coordinator::{
    AttemptOutcome, CaptureLeg, CaptureRequest, RequestLease, RetryDirective,
};
use std::time::Duration;

const FINAL_ERROR_BODY_LIMIT: usize = 64 * 1024;
const FINAL_ERROR_BODY_TIMEOUT: Duration = Duration::from_secs(2);
pub(crate) const FINAL_ERROR_FALLBACK_MESSAGE: &str = "NaN request failed";

pub(crate) struct UpstreamResponse {
    response: reqwest::Response,
    lease: Option<RequestLease>,
    capture: Option<CaptureRequest>,
}

pub(crate) struct CoordinatedBody {
    source: std::pin::Pin<Box<dyn Stream<Item = Result<Bytes, ApiError>> + Send>>,
    lease: Option<RequestLease>,
    capture: Option<CaptureRequest>,
    finished: Option<RetryDirective>,
}

pub(crate) enum FinalErrorBody {
    Complete(String),
    Incomplete,
}

struct FinalErrorCapture {
    capture: Option<CaptureRequest>,
    complete: bool,
}

impl FinalErrorCapture {
    fn record(&self, payload: &[u8]) {
        if let Some(capture) = &self.capture {
            capture.record(CaptureLeg::ProviderResponse, payload);
        }
    }

    fn complete(&mut self) {
        self.complete = true;
    }
}

impl Drop for FinalErrorCapture {
    fn drop(&mut self) {
        if !self.complete
            && let Some(capture) = &self.capture
        {
            capture.mark_incomplete();
        }
    }
}

impl UpstreamResponse {
    pub(crate) fn new(
        response: reqwest::Response,
        lease: Option<RequestLease>,
        capture: Option<CaptureRequest>,
    ) -> Self {
        Self {
            response,
            lease,
            capture,
        }
    }

    #[cfg(test)]
    pub(crate) fn uncoordinated(response: reqwest::Response) -> Self {
        Self {
            response,
            lease: None,
            capture: None,
        }
    }

    pub(crate) fn status(&self) -> reqwest::StatusCode {
        self.response.status()
    }

    pub(crate) fn content_length(&self) -> Option<u64> {
        self.response.content_length()
    }

    pub(crate) fn capture_handle(&self) -> Option<CaptureRequest> {
        self.capture.clone()
    }

    pub(crate) async fn read_final_error_body(self) -> FinalErrorBody {
        let Self {
            mut response,
            mut lease,
            capture,
        } = self;
        let mut capture = FinalErrorCapture {
            capture,
            complete: false,
        };
        let payload = tokio::time::timeout(
            FINAL_ERROR_BODY_TIMEOUT,
            read_bounded_final_error_body(&mut response),
        )
        .await
        .ok()
        .flatten();
        let body = payload.map(|payload| {
            capture.record(&payload);
            String::from_utf8_lossy(&payload).into_owned()
        });
        complete_final_error(&mut lease).await;
        match body {
            Some(body) => {
                capture.complete();
                FinalErrorBody::Complete(body)
            }
            None => FinalErrorBody::Incomplete,
        }
    }

    pub(crate) async fn bytes(self) -> Result<Bytes, reqwest::Error> {
        let Self {
            response,
            mut lease,
            capture,
        } = self;
        let result = response.bytes().await;
        if let Ok(bytes) = &result
            && let Some(capture) = &capture
        {
            capture.record(CaptureLeg::ProviderResponse, bytes);
        }
        complete_body(&mut lease, result.is_ok()).await;
        result
    }

    pub(crate) async fn chunk(&mut self) -> Result<Option<Bytes>, reqwest::Error> {
        let chunk = match self.response.chunk().await {
            Ok(chunk) => chunk,
            Err(error) => {
                complete_body(&mut self.lease, false).await;
                return Err(error);
            }
        };
        if let Some(bytes) = &chunk {
            if let Some(capture) = &self.capture {
                capture.record(CaptureLeg::ProviderResponse, bytes);
            }
        } else {
            complete_body(&mut self.lease, true).await;
        }
        Ok(chunk)
    }

    pub(crate) fn into_coordinated_body(self) -> CoordinatedBody {
        let Self {
            response,
            lease,
            capture,
        } = self;
        CoordinatedBody {
            source: Box::pin(with_inactivity_timeout(
                response.bytes_stream(),
                STREAM_INACTIVITY_TIMEOUT,
            )),
            lease,
            capture,
            finished: None,
        }
    }
}

async fn read_bounded_final_error_body(response: &mut reqwest::Response) -> Option<Vec<u8>> {
    let capacity = match response.content_length() {
        Some(length) => {
            let length = usize::try_from(length).ok()?;
            (length <= FINAL_ERROR_BODY_LIMIT).then_some(length)?
        }
        None => 0,
    };
    let mut payload = Vec::with_capacity(capacity);
    while let Some(chunk) = response.chunk().await.ok()? {
        if chunk.len() > FINAL_ERROR_BODY_LIMIT.saturating_sub(payload.len()) {
            return None;
        }
        payload.extend_from_slice(&chunk);
    }
    Some(payload)
}

impl CoordinatedBody {
    /// Leaves semantic completion to the translating protocol, not raw bytes.
    pub(crate) fn bytes_stream(&mut self) -> impl Stream<Item = Result<Bytes, ApiError>> + '_ {
        stream! {
            loop {
                match self.next().await {
                    Ok(Some(bytes)) => yield Ok(bytes),
                    Ok(None) => break,
                    Err(error) => {
                        yield Err(error);
                        break;
                    }
                }
            }
        }
    }

    pub(crate) async fn next(&mut self) -> Result<Option<Bytes>, ApiError> {
        match self.source.next().await {
            Some(Ok(bytes)) => {
                if let Some(capture) = &self.capture {
                    capture.record(CaptureLeg::ProviderResponse, &bytes);
                }
                Ok(Some(bytes))
            }
            Some(Err(error)) => {
                let outcome = match &error {
                    ApiError::UpstreamTimeout(_) => AttemptOutcome::Timeout,
                    _ => AttemptOutcome::Transport,
                };
                self.finish(outcome).await;
                Err(error)
            }
            None => Ok(None),
        }
    }

    pub(crate) async fn finish(&mut self, outcome: AttemptOutcome) -> RetryDirective {
        if let Some(directive) = self.finished {
            return directive;
        }
        let directive = match &mut self.lease {
            Some(lease) => lease.observe(outcome, None).await,
            None => RetryDirective::Complete,
        };
        self.finished = Some(directive);
        directive
    }
}

async fn complete_body(lease: &mut Option<RequestLease>, succeeded: bool) {
    if let Some(lease) = lease {
        let outcome = if succeeded {
            AttemptOutcome::Success
        } else {
            AttemptOutcome::Transport
        };
        let _ = lease.observe(outcome, None).await;
    }
}

async fn complete_final_error(lease: &mut Option<RequestLease>) {
    if let Some(lease) = lease {
        let _ = lease.observe(AttemptOutcome::Terminal, None).await;
    }
}

#[cfg(test)]
#[path = "response_tests.rs"]
mod tests;
