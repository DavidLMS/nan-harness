use crate::error::ApiError;
use crate::timeouts::{STREAM_INACTIVITY_TIMEOUT, with_inactivity_timeout};
use crate::usage::UsageValues;
use async_stream::stream;
use bytes::Bytes;
use futures_util::{Stream, StreamExt as _};
use nan_harness_coordinator::{
    AttemptOutcome, CaptureLeg, CaptureRequest, RequestLease, RetryDirective, TokenUsage,
};
use std::time::Duration;

const FINAL_ERROR_BODY_LIMIT: usize = 64 * 1024;
const FINAL_ERROR_BODY_TIMEOUT: Duration = Duration::from_secs(2);
const USAGE_OBSERVATION_LIMIT: usize = 64 * 1024;
pub(crate) const FINAL_ERROR_FALLBACK_MESSAGE: &str = "NaN request failed";

pub(crate) struct UpstreamResponse {
    response: reqwest::Response,
    lease: Option<RequestLease>,
    capture: Option<CaptureRequest>,
    usage: UsageParser,
}

pub(crate) struct CoordinatedBody {
    source: std::pin::Pin<Box<dyn Stream<Item = Result<Bytes, ApiError>> + Send>>,
    lease: Option<RequestLease>,
    capture: Option<CaptureRequest>,
    usage: UsageParser,
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

/// Observes provider usage without retaining an unbounded response body.
/// Providers may split an SSE line or JSON document across arbitrary HTTP
/// chunks, so the parser keeps only a bounded incomplete record.
#[derive(Debug, Default)]
struct UsageParser {
    buffer: Vec<u8>,
    usage: Option<UsageValues>,
    unavailable: bool,
}

impl UsageParser {
    fn observe(&mut self, bytes: &[u8]) {
        if self.unavailable {
            return;
        }
        let Some(length) = self.buffer.len().checked_add(bytes.len()) else {
            self.mark_unavailable();
            return;
        };
        if length > USAGE_OBSERVATION_LIMIT {
            self.mark_unavailable();
            return;
        }
        self.buffer.extend_from_slice(bytes);
        self.parse_complete_records();
    }

    fn finish(&mut self) -> Option<UsageValues> {
        if !self.unavailable {
            self.parse_complete_records();
            if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&self.buffer) {
                self.usage = usage_value(&value).or(self.usage);
            } else if let Some(line) = self.buffer.strip_suffix(b"\r").map(ToOwned::to_owned) {
                self.parse_sse_line(&line);
            }
        }
        self.usage
    }

    fn mark_unavailable(&mut self) {
        self.unavailable = true;
        self.buffer.clear();
        self.usage = None;
    }

    fn parse_complete_records(&mut self) {
        let starts_with_json = self
            .buffer
            .iter()
            .find(|byte| !byte.is_ascii_whitespace())
            .is_some_and(|byte| matches!(byte, b'{' | b'['));
        if starts_with_json {
            return;
        }
        while let Some(index) = self
            .buffer
            .iter()
            .position(|byte| matches!(byte, b'\n' | b'\r'))
        {
            let line = self.buffer[..index].to_owned();
            self.parse_sse_line(&line);
            let mut consumed = index + 1;
            if self.buffer[index] == b'\r' && self.buffer.get(consumed) == Some(&b'\n') {
                consumed += 1;
            }
            self.buffer.drain(..consumed);
        }
    }

    fn parse_sse_line(&mut self, line: &[u8]) {
        let Some(data) = line.strip_prefix(b"data:") else {
            return;
        };
        let data = data.strip_prefix(b" ").unwrap_or(data);
        if data == b"[DONE]" {
            return;
        }
        if let Ok(value) = serde_json::from_slice::<serde_json::Value>(data)
            && let Some(usage) = usage_value(&value)
        {
            self.usage = Some(usage);
        }
    }

    fn value(&self) -> Option<UsageValues> {
        self.usage
    }
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
            usage: UsageParser::default(),
        }
    }

    #[cfg(test)]
    pub(crate) fn uncoordinated(response: reqwest::Response) -> Self {
        Self {
            response,
            lease: None,
            capture: None,
            usage: UsageParser::default(),
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
            usage: _,
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
            usage,
        } = self;
        let result = response.bytes().await;
        if let Ok(bytes) = &result
            && let Some(capture) = &capture
        {
            capture.record(CaptureLeg::ProviderResponse, bytes);
        }
        let observed_usage = result
            .as_ref()
            .ok()
            .and_then(|bytes| parse_usage(bytes).or_else(|| usage.value()));
        complete_body(&mut lease, result.is_ok(), observed_usage).await;
        result
    }

    pub(crate) async fn chunk(&mut self) -> Result<Option<Bytes>, reqwest::Error> {
        let chunk = match self.response.chunk().await {
            Ok(chunk) => chunk,
            Err(error) => {
                complete_body(&mut self.lease, false, None).await;
                return Err(error);
            }
        };
        if let Some(bytes) = &chunk {
            self.usage.observe(bytes);
            if let Some(capture) = &self.capture {
                capture.record(CaptureLeg::ProviderResponse, bytes);
            }
        } else {
            complete_body(&mut self.lease, true, self.usage.finish()).await;
        }
        Ok(chunk)
    }

    pub(crate) fn into_coordinated_body(self) -> CoordinatedBody {
        let Self {
            response,
            lease,
            capture,
            usage,
        } = self;
        CoordinatedBody {
            source: Box::pin(with_inactivity_timeout(
                response.bytes_stream(),
                STREAM_INACTIVITY_TIMEOUT,
            )),
            lease,
            capture,
            usage,
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
                self.usage.observe(&bytes);
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
        let usage = (outcome == AttemptOutcome::Success)
            .then(|| self.usage.finish())
            .flatten()
            .map(to_coordinator_usage);
        let directive = match &mut self.lease {
            Some(lease) => lease.observe_with_usage(outcome, None, usage).await,
            None => RetryDirective::Complete,
        };
        self.finished = Some(directive);
        directive
    }
}

async fn complete_body(
    lease: &mut Option<RequestLease>,
    succeeded: bool,
    usage: Option<UsageValues>,
) {
    if let Some(lease) = lease {
        let outcome = if succeeded {
            AttemptOutcome::Success
        } else {
            AttemptOutcome::Transport
        };
        let _ = lease
            .observe_with_usage(outcome, None, usage.map(to_coordinator_usage))
            .await;
    }
}

fn to_coordinator_usage(usage: UsageValues) -> TokenUsage {
    TokenUsage {
        input_tokens: usage.input,
        output_tokens: usage.output,
    }
}

fn parse_usage(bytes: &[u8]) -> Option<UsageValues> {
    let mut parser = UsageParser::default();
    parser.observe(bytes);
    parser.finish()
}

fn usage_value(value: &serde_json::Value) -> Option<UsageValues> {
    let usage = value
        .get("usage")
        .or_else(|| {
            value
                .get("response")
                .and_then(|response| response.get("usage"))
        })
        .unwrap_or(value);
    let input = usage
        .get("prompt_tokens")
        .or_else(|| usage.get("input_tokens"))
        .and_then(serde_json::Value::as_u64)?;
    let output = usage
        .get("completion_tokens")
        .or_else(|| usage.get("output_tokens"))
        .and_then(serde_json::Value::as_u64)?;
    let reasoning = usage
        .get("completion_tokens_details")
        .or_else(|| usage.get("output_tokens_details"))
        .and_then(|details| details.get("reasoning_tokens"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    Some(UsageValues {
        input,
        output,
        reasoning,
    })
}

async fn complete_final_error(lease: &mut Option<RequestLease>) {
    if let Some(lease) = lease {
        let _ = lease.observe(AttemptOutcome::Terminal, None).await;
    }
}

#[cfg(test)]
#[path = "response_tests.rs"]
mod tests;
