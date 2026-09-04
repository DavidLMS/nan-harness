use crate::error::{ApiError, UpstreamTimeoutPhase};
use bytes::Bytes;
use eventsource_stream::EventStreamError;
use futures_util::{Stream, StreamExt};
use std::future::Future;
use std::time::Duration;

/// Bounds the time spent waiting for upstream response headers.
pub(crate) const INITIAL_RESPONSE_TIMEOUT: Duration = Duration::from_secs(90);

/// Bounds the time between successful reads from an upstream response body.
pub(crate) const STREAM_INACTIVITY_TIMEOUT: Duration = Duration::from_mins(2);

/// Waits for an upstream request to produce its initial response without
/// imposing a deadline on the response body once headers have arrived.
pub(crate) async fn with_initial_response_timeout<F, T>(
    future: F,
    timeout: Duration,
) -> Result<T, ApiError>
where
    F: Future<Output = Result<T, reqwest::Error>>,
{
    match tokio::time::timeout(timeout, future).await {
        Ok(Ok(result)) => Ok(result),
        Ok(Err(error)) if error.is_timeout() => Err(ApiError::UpstreamTimeout(
            UpstreamTimeoutPhase::InitialResponse,
        )),
        Ok(Err(error)) => Err(ApiError::UpstreamTransport(error)),
        Err(_) => Err(ApiError::UpstreamTimeout(
            UpstreamTimeoutPhase::InitialResponse,
        )),
    }
}

/// Wraps an upstream byte stream with an inactivity deadline.
///
/// The deadline is reset after every successful chunk, so long-running
/// responses remain healthy as long as the provider continues making progress.
pub(crate) fn with_inactivity_timeout<S>(
    stream: S,
    timeout: Duration,
) -> impl Stream<Item = Result<Bytes, ApiError>>
where
    S: Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
{
    async_stream::stream! {
        let mut stream = Box::pin(stream);
        loop {
            match tokio::time::timeout(timeout, stream.next()).await {
                Ok(Some(Ok(chunk))) => yield Ok(chunk),
                Ok(Some(Err(error))) => {
                    yield Err(map_body_error(error));
                    break;
                }
                Ok(None) => break,
                Err(_) => {
                    yield Err(ApiError::UpstreamTimeout(UpstreamTimeoutPhase::Inactivity));
                    break;
                }
            }
        }
    }
}

/// Converts an SSE parser error while preserving typed upstream failures from
/// [`with_inactivity_timeout`].
pub(crate) fn map_sse_error(error: EventStreamError<ApiError>) -> ApiError {
    match error {
        EventStreamError::Transport(error) => error,
        error => ApiError::InvalidUpstream(format!("invalid SSE stream: {error}")),
    }
}

/// Converts a non-streaming response-body read failure into the bridge's
/// typed upstream error. `reqwest` applies its read timeout per body read.
pub(crate) fn map_body_error(error: reqwest::Error) -> ApiError {
    if error.is_timeout() {
        ApiError::UpstreamTimeout(UpstreamTimeoutPhase::Inactivity)
    } else {
        ApiError::UpstreamTransport(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use futures_util::{StreamExt, TryStreamExt, stream};

    #[tokio::test]
    async fn initial_response_timeout_is_typed() {
        let result: Result<(), ApiError> = with_initial_response_timeout(
            std::future::pending::<Result<(), reqwest::Error>>(),
            Duration::from_millis(1),
        )
        .await;

        assert!(matches!(
            result,
            Err(ApiError::UpstreamTimeout(
                UpstreamTimeoutPhase::InitialResponse
            ))
        ));
    }

    #[test]
    fn initial_response_timeout_allows_ninety_seconds() {
        assert_eq!(INITIAL_RESPONSE_TIMEOUT, Duration::from_secs(90));
    }

    #[tokio::test]
    async fn inactivity_timeout_is_reset_after_each_chunk() {
        let source = stream::unfold(0_u8, |index| async move {
            if index == 6 {
                return None;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
            Some((Ok::<_, reqwest::Error>(Bytes::from(vec![index])), index + 1))
        });
        let chunks = with_inactivity_timeout(source, Duration::from_millis(50))
            .try_collect::<Vec<_>>()
            .await
            .expect("stream should remain active while chunks arrive");

        assert_eq!(
            chunks,
            vec![
                Bytes::from(vec![0]),
                Bytes::from(vec![1]),
                Bytes::from(vec![2]),
                Bytes::from(vec![3]),
                Bytes::from(vec![4]),
                Bytes::from(vec![5])
            ]
        );
    }

    #[test]
    fn stream_inactivity_timeout_allows_two_minutes_between_chunks() {
        assert_eq!(STREAM_INACTIVITY_TIMEOUT, Duration::from_mins(2));
    }

    #[tokio::test]
    async fn inactivity_timeout_is_typed() {
        let source = stream::pending::<Result<Bytes, reqwest::Error>>();
        let mut guarded = Box::pin(with_inactivity_timeout(source, Duration::from_millis(1)));
        let result = guarded.next().await;

        assert!(matches!(
            result,
            Some(Err(ApiError::UpstreamTimeout(
                UpstreamTimeoutPhase::Inactivity
            )))
        ));
    }

    #[test]
    fn sse_transport_error_preserves_typed_timeout() {
        let error = map_sse_error(EventStreamError::Transport(ApiError::UpstreamTimeout(
            UpstreamTimeoutPhase::Inactivity,
        )));

        assert!(matches!(
            error,
            ApiError::UpstreamTimeout(UpstreamTimeoutPhase::Inactivity)
        ));
    }

    #[test]
    fn timeout_emits_the_transport_diagnostic_contract() {
        let error = ApiError::UpstreamTimeout(UpstreamTimeoutPhase::InitialResponse);
        let diagnostic = crate::diagnostics::BridgeDiagnostic::from_api_error(
            &error,
            crate::diagnostics::BridgeEndpoint::Responses,
        );

        assert_eq!(diagnostic.code, "NH-BRIDGE-103");
        assert_eq!(
            diagnostic.reason,
            crate::diagnostics::BridgeDiagnosticReason::UpstreamTransport
        );
        assert_eq!(
            diagnostic.endpoint,
            crate::diagnostics::BridgeEndpoint::Responses
        );
    }
}
