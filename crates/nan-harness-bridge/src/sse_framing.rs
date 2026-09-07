use crate::error::ApiError;
use bytes::Bytes;
use futures_util::{Stream, StreamExt};

/// Maximum raw bytes accepted in one unfinished SSE wire event.
pub(crate) const MAX_SSE_EVENT_BYTES: usize = 8 * 1024 * 1024;

pub(crate) fn guard<S>(source: S) -> impl Stream<Item = Result<Bytes, ApiError>>
where
    S: Stream<Item = Result<Bytes, ApiError>>,
{
    guard_with_limit(source, MAX_SSE_EVENT_BYTES)
}

fn guard_with_limit<S>(
    source: S,
    max_event_bytes: usize,
) -> impl Stream<Item = Result<Bytes, ApiError>>
where
    S: Stream<Item = Result<Bytes, ApiError>>,
{
    async_stream::stream! {
        futures_util::pin_mut!(source);
        let mut counter = FrameCounter::default();
        while let Some(item) = source.next().await {
            let chunk = match item {
                Ok(chunk) => chunk,
                Err(error) => {
                    yield Err(error);
                    return;
                }
            };
            let mut start = 0;
            let mut index = 0;
            while index < chunk.len() {
                let boundary = match counter.push(chunk[index], max_event_bytes) {
                    Ok(boundary) => boundary,
                    Err(error) => {
                        yield Err(error);
                        return;
                    }
                };
                index += 1;
                if boundary {
                    yield Ok(chunk.slice(start..index));
                    start = index;
                }
            }
            if start < chunk.len() {
                yield Ok(chunk.slice(start..));
            }
        }
        // A trailing CR is a complete SSE line ending. Supplying its equivalent
        // CRLF form lets the streaming parser finish that line at upstream EOF.
        if counter.ends_with_cr() {
            yield Ok(Bytes::from_static(b"\n"));
        }
    }
}

/// Counts field/comment bytes and the line endings of non-empty lines. The
/// empty line that dispatches an event is excluded. Both bytes of a CRLF line
/// ending count for a non-empty line and are excluded for a dispatching line.
#[derive(Default)]
struct FrameCounter {
    event_bytes: usize,
    line_has_content: bool,
    previous_was_cr: bool,
    previous_cr_dispatched: bool,
}

impl FrameCounter {
    fn push(&mut self, byte: u8, limit: usize) -> Result<bool, ApiError> {
        if self.previous_was_cr {
            self.previous_was_cr = false;
            if byte == b'\n' {
                if self.previous_cr_dispatched {
                    return Ok(true);
                }
                self.count_byte(limit)?;
                return Ok(false);
            }
        }

        match byte {
            b'\r' => self.end_line(limit, true),
            b'\n' => self.end_line(limit, false),
            _ => {
                self.count_byte(limit)?;
                self.line_has_content = true;
                Ok(false)
            }
        }
    }

    fn end_line(&mut self, limit: usize, carriage_return: bool) -> Result<bool, ApiError> {
        let dispatched = !self.line_has_content;
        if dispatched {
            self.event_bytes = 0;
        } else {
            self.count_byte(limit)?;
        }
        self.line_has_content = false;
        self.previous_was_cr = carriage_return;
        self.previous_cr_dispatched = carriage_return && dispatched;
        Ok(dispatched)
    }

    fn count_byte(&mut self, limit: usize) -> Result<(), ApiError> {
        if self.event_bytes == limit {
            return Err(framing_limit_error());
        }
        self.event_bytes += 1;
        Ok(())
    }

    const fn ends_with_cr(&self) -> bool {
        self.previous_was_cr
    }
}

fn framing_limit_error() -> ApiError {
    ApiError::InvalidUpstream("SSE event exceeded the 8 MiB framing limit".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use eventsource_stream::Eventsource;
    use futures_util::{StreamExt, stream};
    use std::fmt::Write as _;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    fn exact_event(limit: usize) -> Bytes {
        let mut wire = b"data: ".to_vec();
        wire.resize(limit - 1, b'x');
        wire.extend_from_slice(b"\n\n");
        Bytes::from(wire)
    }

    fn chunk_every_byte(wire: &str) -> Vec<Result<Bytes, ApiError>> {
        wire.as_bytes()
            .chunks(1)
            .map(|chunk| Ok(Bytes::copy_from_slice(chunk)))
            .collect()
    }

    #[tokio::test]
    async fn accepts_exact_default_limit_and_rejects_one_more_byte() {
        let exact = exact_event(MAX_SSE_EVENT_BYTES);
        let accepted = guard(stream::iter([Ok(exact.clone())]))
            .collect::<Vec<_>>()
            .await;
        assert!(accepted.iter().all(Result::is_ok));
        assert_eq!(
            accepted
                .into_iter()
                .map(Result::unwrap)
                .map(|chunk| chunk.len())
                .sum::<usize>(),
            exact.len()
        );

        let dropped = Arc::new(AtomicBool::new(false));
        let source_dropped = Arc::clone(&dropped);
        let source = async_stream::stream! {
            let _drop_probe = DropProbe(source_dropped);
            yield Ok(Bytes::from(vec![b'x'; MAX_SSE_EVENT_BYTES + 1]));
            std::future::pending::<()>().await;
        };
        let mut guarded = Box::pin(guard(source));
        let result = tokio::time::timeout(Duration::from_millis(100), guarded.next())
            .await
            .expect("overflow must be detected before upstream EOF")
            .expect("guard must report overflow");
        let error = result.expect_err("one byte over the limit must fail");
        assert!(matches!(error, ApiError::InvalidUpstream(_)));
        assert!(error.to_string().contains("8 MiB framing limit"));
        drop(guarded);
        assert!(dropped.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn rejects_multiline_event_over_limit_before_eof() {
        let limit = 64;
        let source = async_stream::stream! {
            yield Ok::<_, ApiError>(Bytes::from_static(
                b"data: 1234567890\ndata: 1234567890\ndata: 1234567890\ndata: 1234567890\n",
            ));
            std::future::pending::<()>().await;
        };
        let mut guarded = Box::pin(guard_with_limit(source, limit));
        let result = tokio::time::timeout(Duration::from_millis(100), guarded.next())
            .await
            .expect("multiline overflow must be detected before upstream EOF")
            .expect("guard must report overflow");
        assert!(matches!(result, Err(ApiError::InvalidUpstream(_))));
    }

    #[tokio::test]
    async fn preserves_newlines_split_utf8_comments_and_multiline_data() {
        for newline in ["\n", "\r\n", "\r"] {
            let wire = [
                ": keep-alive",
                "id: response-7",
                "event: custom",
                "data: first",
                "data: café 👍",
                "",
                "data: tail",
                "",
                "",
            ]
            .join(newline);
            let parsed = guard(stream::iter(chunk_every_byte(&wire)))
                .eventsource()
                .collect::<Vec<_>>()
                .await
                .into_iter()
                .collect::<Result<Vec<_>, _>>()
                .expect("valid SSE must parse");

            assert_eq!(parsed.len(), 2, "newline {newline:?}");
            assert_eq!(parsed[0].event, "custom");
            assert_eq!(parsed[0].id, "response-7");
            assert_eq!(parsed[0].data, "first\ncafé 👍");
            assert_eq!(parsed[1].id, "response-7");
            assert_eq!(parsed[1].data, "tail");
        }
    }

    #[tokio::test]
    async fn splits_large_chunks_at_small_event_boundaries() {
        let mut wire = String::new();
        for index in 0..256 {
            write!(wire, "data: {index}\n\n").expect("writing to a String cannot fail");
        }
        assert!(wire.len() > 32);
        let parsed = guard_with_limit(stream::iter([Ok(Bytes::from(wire))]), 32)
            .eventsource()
            .collect::<Vec<_>>()
            .await;

        assert_eq!(parsed.len(), 256);
        assert!(parsed.into_iter().all(|event| event.is_ok()));
    }

    #[tokio::test]
    async fn accepts_long_streams_of_individually_bounded_events() {
        let chunks = (0..4_096)
            .map(|index| Ok(Bytes::from(format!("data: {index}\n\n"))))
            .collect::<Vec<Result<_, ApiError>>>();
        let parsed = guard_with_limit(stream::iter(chunks), 32)
            .eventsource()
            .collect::<Vec<_>>()
            .await;

        assert_eq!(parsed.len(), 4_096);
        assert!(parsed.into_iter().all(|event| event.is_ok()));
    }

    #[tokio::test]
    async fn cancellation_drops_the_upstream_stream() {
        let dropped = Arc::new(AtomicBool::new(false));
        let source_dropped = Arc::clone(&dropped);
        let source = async_stream::stream! {
            let _drop_probe = DropProbe(source_dropped);
            yield Ok::<_, ApiError>(Bytes::from_static(b"data: partial"));
            std::future::pending::<()>().await;
        };
        let mut guarded = Box::pin(guard(source));

        assert!(guarded.next().await.expect("first chunk").is_ok());
        drop(guarded);
        assert!(dropped.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn preserves_upstream_failure_type() {
        let source = stream::iter([Err(ApiError::InvalidRequest("synthetic".to_owned()))]);
        let guarded = guard(source);
        futures_util::pin_mut!(guarded);
        let result = guarded.next().await.expect("upstream failure");

        assert!(matches!(result, Err(ApiError::InvalidRequest(_))));
    }

    struct DropProbe(Arc<AtomicBool>);

    impl Drop for DropProbe {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
}
