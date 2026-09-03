use crate::usage::{RequestUsageGuard, UsageValues};
use serde_json::Value;

const MAX_OBSERVATION_BYTES: usize = 1024 * 1024;
const OBSERVATION_COMPACTION_THRESHOLD: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ObservationKind {
    Streaming,
    NonStreaming,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ObservationAvailability {
    Available,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SseTerminal {
    Pending,
    Done,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SseLineMode {
    Keep,
    DiscardUntilNewline,
}

#[derive(Debug)]
pub(super) struct UsageObserver {
    kind: ObservationKind,
    guard: Option<RequestUsageGuard>,
    buffer: Vec<u8>,
    cursor: usize,
    usage: Option<UsageValues>,
    terminal: SseTerminal,
    availability: ObservationAvailability,
    line_mode: SseLineMode,
}

impl UsageObserver {
    pub(super) fn new(streaming: bool, guard: Option<RequestUsageGuard>) -> Self {
        Self {
            kind: if streaming {
                ObservationKind::Streaming
            } else {
                ObservationKind::NonStreaming
            },
            guard,
            buffer: Vec::new(),
            cursor: 0,
            usage: None,
            terminal: SseTerminal::Pending,
            availability: ObservationAvailability::Available,
            line_mode: SseLineMode::Keep,
        }
    }

    pub(super) fn observe(&mut self, chunk: &[u8]) {
        if self.guard.is_none() {
            return;
        }
        if self.kind == ObservationKind::Streaming {
            self.observe_sse_chunk(chunk);
        } else if self.availability == ObservationAvailability::Available {
            let Some(next_len) = self.buffer.len().checked_add(chunk.len()) else {
                self.mark_observation_unavailable();
                return;
            };
            if next_len > MAX_OBSERVATION_BYTES {
                self.mark_observation_unavailable();
            } else {
                self.buffer.extend_from_slice(chunk);
            }
        }
    }

    fn observe_sse_chunk(&mut self, mut chunk: &[u8]) {
        while !chunk.is_empty() {
            if self.line_mode == SseLineMode::DiscardUntilNewline {
                let Some(index) = chunk.iter().position(|byte| *byte == b'\n') else {
                    return;
                };
                self.line_mode = SseLineMode::Keep;
                chunk = &chunk[index + 1..];
                continue;
            }

            let pending = self.buffer.len().saturating_sub(self.cursor);
            if let Some(index) = chunk.iter().position(|byte| *byte == b'\n') {
                let line_length = index + 1;
                if pending.saturating_add(line_length) > MAX_OBSERVATION_BYTES {
                    self.mark_observation_unavailable();
                    self.line_mode = SseLineMode::DiscardUntilNewline;
                    chunk = &chunk[line_length..];
                    continue;
                }
                self.buffer.extend_from_slice(&chunk[..line_length]);
                chunk = &chunk[line_length..];
                self.observe_sse_lines();
            } else if pending.saturating_add(chunk.len()) > MAX_OBSERVATION_BYTES {
                self.mark_observation_unavailable();
                return;
            } else {
                self.buffer.extend_from_slice(chunk);
                return;
            }
        }
    }

    fn observe_sse_lines(&mut self) {
        while let Some(index) = self.buffer[self.cursor..]
            .iter()
            .position(|byte| *byte == b'\n')
        {
            let end = self.cursor + index;
            let line = &self.buffer[self.cursor..end];
            let (saw_done, usage) = Self::parse_sse_line(line);
            if saw_done {
                self.terminal = SseTerminal::Done;
            }
            if usage.is_some() {
                self.usage = usage;
            }
            self.cursor = end + 1;
            if self.cursor >= OBSERVATION_COMPACTION_THRESHOLD
                && self.cursor.saturating_mul(2) >= self.buffer.len()
            {
                self.buffer.drain(..self.cursor);
                self.cursor = 0;
            }
        }
    }

    fn parse_sse_line(line: &[u8]) -> (bool, Option<UsageValues>) {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let Some(data) = line.strip_prefix(b"data:") else {
            return (false, None);
        };
        let data = data.strip_prefix(b" ").unwrap_or(data);
        if data == b"[DONE]" {
            return (true, None);
        }
        if let Ok(value) = serde_json::from_slice::<Value>(data)
            && let Some(usage) = parse_usage(&value)
        {
            return (false, Some(usage));
        }
        (false, None)
    }

    fn mark_observation_unavailable(&mut self) {
        self.availability = ObservationAvailability::Unavailable;
        self.usage = None;
        self.buffer.clear();
        self.cursor = 0;
    }

    pub(super) fn finish(&mut self) {
        if self.guard.is_none() {
            return;
        }
        if self.kind == ObservationKind::Streaming && self.terminal != SseTerminal::Done {
            return;
        }
        if self.kind == ObservationKind::NonStreaming
            && self.availability == ObservationAvailability::Available
        {
            let Ok(value) = serde_json::from_slice::<Value>(&self.buffer) else {
                return;
            };
            self.usage = parse_usage(&value);
        }
        let values = (self.availability == ObservationAvailability::Available)
            .then_some(self.usage)
            .flatten();
        self.guard
            .as_mut()
            .expect("usage guard is present")
            .complete(values);
    }
}

fn parse_usage(value: &Value) -> Option<UsageValues> {
    let usage = value.get("usage")?.as_object()?;
    let prompt_tokens = usage.get("prompt_tokens")?.as_u64()?;
    let completion_tokens = usage.get("completion_tokens")?.as_u64()?;
    let reasoning_tokens = usage
        .get("completion_tokens_details")
        .and_then(Value::as_object)
        .and_then(|details| details.get("reasoning_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    Some(UsageValues {
        input: prompt_tokens,
        output: completion_tokens,
        reasoning: reasoning_tokens,
    })
}

#[cfg(test)]
mod tests {
    use super::UsageObserver;
    use crate::usage::{ModelUsageSnapshot, RequestUsageGuard, new_usage, snapshot};

    #[test]
    fn streaming_observation_handles_chunk_boundaries_and_crlf() {
        let usage = new_usage();
        let guard = RequestUsageGuard::new(&usage, "qwen3.6");
        let mut observer = UsageObserver::new(true, Some(guard));

        observer.observe(b"data: {\"usage\":{\"prompt_tokens\":5,");
        observer.observe(
            b"\"completion_tokens\":7,\"completion_tokens_details\":{\"reasoning_tokens\":2}}}\r\n\r\n",
        );
        observer.observe(b"data: [DONE]\r\n\r\n");
        observer.finish();

        assert_eq!(
            snapshot(&usage).models["qwen3.6"],
            ModelUsageSnapshot {
                responses_with_usage: 1,
                input_tokens: 5,
                output_tokens: 7,
                reasoning_tokens: 2,
                ..ModelUsageSnapshot::default()
            }
        );
    }

    #[test]
    fn non_streaming_observation_commits_only_complete_usage_objects() {
        let usage = new_usage();
        let guard = RequestUsageGuard::new(&usage, "qwen3.6");
        let mut observer = UsageObserver::new(false, Some(guard));

        observer.observe(b"{\"usage\":{\"prompt_tokens\":3,");
        observer.observe(b"\"completion_tokens\":2}}");
        observer.finish();

        assert_eq!(
            snapshot(&usage).models["qwen3.6"],
            ModelUsageSnapshot {
                responses_with_usage: 1,
                input_tokens: 3,
                output_tokens: 2,
                ..ModelUsageSnapshot::default()
            }
        );
    }

    #[test]
    fn streaming_observation_requires_the_done_terminal() {
        let usage = new_usage();
        let guard = RequestUsageGuard::new(&usage, "qwen3.6");
        let mut observer = UsageObserver::new(true, Some(guard));

        observer.observe(b"data: {\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":7}}\n\n");
        observer.finish();
        drop(observer);

        assert_eq!(
            snapshot(&usage).models["qwen3.6"],
            ModelUsageSnapshot {
                incomplete_responses: 1,
                ..ModelUsageSnapshot::default()
            }
        );
    }
}
