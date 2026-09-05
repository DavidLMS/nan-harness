use crate::error::ApiError;
use crate::timeouts::{STREAM_INACTIVITY_TIMEOUT, map_body_error};
use async_stream::stream;
use bytes::Bytes;
use futures_util::{Stream, StreamExt as _};
use nan_harness_coordinator::{
    AttemptOutcome, CaptureLeg, CaptureRequest, RequestLease, RetryDirective,
};
const DONE_MARKER: &[u8] = b"data: [DONE]";
const COMPACT_DONE_MARKER: &[u8] = b"data:[DONE]";

pub(crate) struct UpstreamResponse {
    response: reqwest::Response,
    lease: Option<RequestLease>,
    capture: Option<CaptureRequest>,
}

pub(crate) struct CoordinatedBody {
    source: std::pin::Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>>,
    lease: Option<RequestLease>,
    capture: Option<CaptureRequest>,
    finished: Option<RetryDirective>,
}
#[derive(Default)]
pub(crate) struct DoneMarkerDetector {
    line: Vec<u8>,
    overflow: bool,
}

impl DoneMarkerDetector {
    pub(crate) fn push(&mut self, bytes: &[u8]) -> bool {
        let mut found = false;
        for &byte in bytes {
            if byte == b'\n' {
                found |= self.finish_line();
                self.line.clear();
                self.overflow = false;
            } else if !self.overflow {
                if self.line.len() < DONE_MARKER.len() + 1 {
                    self.line.push(byte);
                } else {
                    self.line.clear();
                    self.overflow = true;
                }
            }
        }
        found
    }

    pub(crate) fn finish(&self) -> bool {
        self.finish_line()
    }

    pub(crate) fn finish_line(&self) -> bool {
        !self.overflow && is_done_line(&self.line)
    }
}

fn is_done_line(line: &[u8]) -> bool {
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    line == DONE_MARKER || line == COMPACT_DONE_MARKER
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

    pub(crate) async fn text(self) -> Result<String, reqwest::Error> {
        let Self {
            response,
            mut lease,
            capture,
        } = self;
        let result = response.text().await;
        if let Ok(text) = &result
            && let Some(capture) = &capture
        {
            capture.record(CaptureLeg::ProviderResponse, text.as_bytes());
        }
        complete_body(&mut lease, result.is_ok()).await;
        result
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

    pub(crate) fn bytes_stream(
        self,
    ) -> impl Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static {
        let Self {
            response,
            mut lease,
            capture,
        } = self;
        let source = response.bytes_stream();
        stream! {
            futures_util::pin_mut!(source);
            let mut terminal = DoneMarkerDetector::default();
            while let Some(item) = source.next().await {
                if let Ok(bytes) = &item
                    && let Some(capture) = &capture
                {
                    capture.record(CaptureLeg::ProviderResponse, bytes);
                }
                let failed = item.is_err();
                let done = item
                    .as_ref()
                    .is_ok_and(|bytes| terminal.push(bytes));
                if failed {
                    if let Some(lease) = &mut lease {
                        let _ = lease.observe(AttemptOutcome::Transport, None).await;
                    }
                    yield item;
                    return;
                }
                if done
                    && let Some(lease) = &mut lease
                {
                    let _ = lease.observe(AttemptOutcome::Success, None).await;
                }
                yield item;
            }
            if let Some(lease) = &mut lease {
                let outcome = if terminal.finish() {
                    AttemptOutcome::Success
                } else {
                    AttemptOutcome::InvalidResponse
                };
                let _ = lease.observe(outcome, None).await;
            }
        }
    }

    pub(crate) fn into_coordinated_body(self) -> CoordinatedBody {
        let Self {
            response,
            lease,
            capture,
        } = self;
        CoordinatedBody {
            source: Box::pin(response.bytes_stream()),
            lease,
            capture,
            finished: None,
        }
    }
}

impl CoordinatedBody {
    pub(crate) async fn next(&mut self) -> Result<Option<Bytes>, ApiError> {
        let Ok(item) = tokio::time::timeout(STREAM_INACTIVITY_TIMEOUT, self.source.next()).await
        else {
            self.finish(AttemptOutcome::Timeout).await;
            return Err(ApiError::UpstreamTimeout(
                crate::error::UpstreamTimeoutPhase::Inactivity,
            ));
        };
        match item {
            Some(Ok(bytes)) => {
                if let Some(capture) = &self.capture {
                    capture.record(CaptureLeg::ProviderResponse, &bytes);
                }
                Ok(Some(bytes))
            }
            Some(Err(error)) => {
                self.finish(AttemptOutcome::Transport).await;
                Err(map_body_error(error))
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
#[cfg(test)]
mod tests {
    use super::DoneMarkerDetector;
    #[test]
    fn done_marker_requires_a_complete_sse_line() {
        let mut split = DoneMarkerDetector::default();
        assert!(!split.push(b"data: [DO"));
        assert!(split.push(b"NE]\r\n\r\n"));
        let mut compact = DoneMarkerDetector::default();
        assert!(!compact.push(b"data:[DO"));
        assert!(compact.push(b"NE]\n"));
        let mut embedded = DoneMarkerDetector::default();
        assert!(!embedded.push(b"data: mentioned data: [DO"));
        assert!(!embedded.push(b"NE] in output\n"));
        assert!(!embedded.finish());
    }
}
