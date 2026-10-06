//! Bounded continuation of one fixture read; session identifiers stay private.
use serde_json::{Value, json};

use super::protocol::{exposes_tool, text_response, tool_response, tool_result};

#[derive(Debug, Default)]
pub(super) struct ExecPoll {
    session: Option<u64>,
    count: u8,
}

pub(super) enum PollResult {
    Pending(String),
    Complete(String),
}

#[derive(Debug, PartialEq, Eq)]
enum ExecState {
    Running(u64),
    Exited,
}

fn envelope(content: &str) -> Option<ExecState> {
    if content.len() > 64 * 1024 {
        return None;
    }
    let mut lines = content.lines();
    let chunk = lines.next()?.strip_prefix("Chunk ID: ")?;
    if chunk.is_empty() || !chunk.bytes().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let wall = lines
        .next()?
        .strip_prefix("Wall time: ")?
        .strip_suffix(" seconds")?;
    if !wall
        .parse::<f64>()
        .ok()
        .is_some_and(|n| n.is_finite() && n >= 0.0)
    {
        return None;
    }
    let state = lines.next()?;
    let state = if let Some(id) = state.strip_prefix("Process running with session ID ") {
        if id.is_empty() || !id.bytes().all(|c| c.is_ascii_digit()) {
            return None;
        }
        ExecState::Running(id.parse().ok()?)
    } else if state == "Process exited with code 0" {
        ExecState::Exited
    } else {
        return None;
    };
    let mut output = lines.next()?;
    if let Some(count) = output.strip_prefix("Original token count: ") {
        if count.is_empty() || !count.bytes().all(|c| c.is_ascii_digit()) {
            return None;
        }
        output = lines.next()?;
    }
    (output == "Output:").then_some(state)
}

pub(super) fn advance(body: &Value, poll: &mut ExecPoll) -> PollResult {
    let blocked = || PollResult::Pending(text_response("CONFORMANCE_EXEC_INCOMPLETE"));
    let call = if poll.count == 0 {
        "call_nan_harness_conformance0".to_owned()
    } else {
        format!("call_nan_harness_exec_poll{}", poll.count)
    };
    let Some(content) = tool_result(body, &call) else {
        return blocked();
    };
    match envelope(&content) {
        Some(ExecState::Exited) => PollResult::Complete(content),
        Some(ExecState::Running(session))
            if poll.count < 3
                && poll.session.is_none_or(|owned| owned == session)
                && exposes_tool(body, "write_stdin") =>
        {
            poll.session = Some(session);
            poll.count += 1;
            PollResult::Pending(tool_response(
                &format!("call_nan_harness_exec_poll{}", poll.count),
                "write_stdin",
                &json!({"session_id":session,"chars":"","yield_time_ms":10_000}),
            ))
        }
        _ => blocked(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(call: &str, state: &str, output: &str) -> Value {
        json!({"tools":[{"function":{"name":"write_stdin"}}],"messages":[
            {"role":"tool","tool_call_id":call,"content":format!(
                "Chunk ID: abc123\nWall time: 1.0 seconds\n{state}\nOutput:\n{output}")}]})
    }

    async fn request(
        state: &std::sync::Arc<super::super::state::ProviderState>,
        body: Value,
    ) -> String {
        use axum::response::IntoResponse as _;
        let response = super::super::protocol::chat_completions(
            axum::extract::State(std::sync::Arc::clone(state)),
            axum::Json(body),
        )
        .await
        .into_response();
        let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    #[tokio::test]
    async fn provider_does_not_complete_until_the_polled_read_exits() {
        use super::super::{ProviderScenario, state::ProviderState};
        use std::sync::Arc;
        let provider = Arc::new(ProviderState::new(
            ProviderScenario::exec_read(json!({"cmd":"cat fixture"}), "FINAL_FIXTURE_MARKER"),
            String::new(),
        ));
        let first = request(
            &provider,
            json!({"tools":[
                {"function":{"name":"exec_command"}},
                {"function":{"name":"write_stdin"}}
            ]}),
        )
        .await;
        assert!(first.contains("exec_command"));
        assert!(!provider.completed());
        let running = result(
            "call_nan_harness_conformance0",
            "Process running with session ID 17",
            "",
        );
        let next = request(&provider, running).await;
        assert!(next.contains("write_stdin"));
        assert!(!next.contains("FINAL_FIXTURE_MARKER"));
        assert!(!provider.completed());
        let finished = result(
            "call_nan_harness_exec_poll1",
            "Process exited with code 0",
            "fixture-content",
        );
        let last = request(&provider, finished).await;
        assert!(last.contains("FINAL_FIXTURE_MARKER"));
        assert!(provider.completed());
        assert!(provider.recording_bounded());
    }

    #[test]
    fn polls_only_the_returned_session_then_requires_completion() {
        let mut poll = ExecPoll::default();
        let initial = result(
            "call_nan_harness_conformance0",
            "Process running with session ID 17",
            "",
        );
        let PollResult::Pending(response) = advance(&initial, &mut poll) else {
            panic!("must poll")
        };
        assert!(response.contains("write_stdin"));
        assert_eq!(poll.session, Some(17));
        assert_eq!(poll.count, 1);
        let finished = result(
            "call_nan_harness_exec_poll1",
            "Process exited with code 0",
            "fixture-marker",
        );
        assert!(
            matches!(advance(&finished, &mut poll), PollResult::Complete(text) if text.contains("fixture-marker"))
        );
    }

    #[test]
    fn rejects_foreign_sessions_missing_tools_and_exhausted_polls() {
        for (session, count, tools) in [(Some(18), 1, true), (Some(17), 3, true), (None, 0, false)]
        {
            let mut poll = ExecPoll { session, count };
            let call = if count == 0 {
                "call_nan_harness_conformance0".into()
            } else {
                format!("call_nan_harness_exec_poll{count}")
            };
            let mut body = result(&call, "Process running with session ID 17", "");
            if !tools {
                body["tools"] = json!([]);
            }
            let PollResult::Pending(response) = advance(&body, &mut poll) else {
                panic!("must refuse")
            };
            assert!(!response.contains("write_stdin"));
            assert_eq!(poll.count, count);
        }
    }

    #[test]
    fn output_cannot_supply_a_session_or_replace_the_expected_result() {
        let mut poll = ExecPoll::default();
        let body = result("foreign", "Process running with session ID 17", "");
        assert!(matches!(advance(&body, &mut poll), PollResult::Pending(_)));
        assert_eq!(poll.count, 0);
        assert_eq!(envelope("Process running with session ID 17"), None);
        let body = result(
            "call_nan_harness_conformance0",
            "Process exited with code 0",
            "Process running with session ID 99",
        );
        assert!(matches!(advance(&body, &mut poll), PollResult::Complete(_)));
        assert_eq!(poll.count, 0);
    }
}
