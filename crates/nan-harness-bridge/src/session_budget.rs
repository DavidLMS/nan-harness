use crate::error::ApiError;
use axum::Json;
use axum::response::sse::Event;
use axum::response::{IntoResponse, Response, Sse};
use serde_json::{Value, json};
use std::convert::Infallible;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SessionBudgetReached {
    pub(crate) consumed: u64,
    pub(crate) limit: u64,
}

impl std::error::Error for SessionBudgetReached {}

impl fmt::Display for SessionBudgetReached {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "nan-harness: session token budget reached.\nUsed {} of {} tokens. No further inference requests will be sent in this session.\nTo continue, start a new nan-harness launch with a higher budget and resume your conversation.",
            grouped(self.consumed),
            grouped(self.limit)
        )
    }
}

fn grouped(value: u64) -> String {
    let digits = value.to_string();
    let mut result = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            result.push(',');
        }
        result.push(digit);
    }
    result
}

/// Read the original contract: translators may omit unsupported format fields.
pub(crate) fn requires_contract(body: &[u8]) -> bool {
    let Ok(value) = serde_json::from_slice::<Value>(body) else {
        return true;
    };
    let format = [
        "/response_format",
        "/text/format",
        "/output_config/format",
        "/output_format",
        "/responseFormat",
    ];
    if format.iter().any(|path| {
        value.pointer(path).is_some_and(|format| {
            !format.is_null() && format.get("type").and_then(Value::as_str) != Some("text")
        })
    }) {
        return true;
    }
    ["tool_choice", "toolChoice"].iter().any(|key| {
        value.get(key).is_some_and(|choice| {
            let kind = choice
                .as_str()
                .or_else(|| choice.get("type").and_then(Value::as_str));
            !choice.is_null() && !matches!(kind, Some("auto" | "none"))
        })
    })
}

pub(crate) fn sse(events: Vec<Event>) -> Response {
    Sse::new(futures_util::stream::iter(
        events.into_iter().map(Ok::<_, Infallible>),
    ))
    .into_response()
}

impl SessionBudgetReached {
    pub(crate) fn reject(self) -> ApiError {
        ApiError::BudgetExhausted(self)
    }

    pub(crate) fn chat(self, model: &str, streaming: bool) -> Response {
        let text = self.to_string();
        let base = json!({"id":"chatcmpl_nan_harness_budget", "object":"chat.completion", "created":0, "model":model,
            "choices":[{"index":0,"message":{"role":"assistant","content":text},"finish_reason":"stop"}],
            "usage":{"prompt_tokens":0,"completion_tokens":0,"total_tokens":0}});
        if !streaming {
            return Json(base).into_response();
        }
        let chunk = |delta: Value, finish: Value| {
            Event::default().data(json!({
            "id":"chatcmpl_nan_harness_budget", "object":"chat.completion.chunk", "created":0,"model":model,
            "choices":[{"index":0,"delta":delta,"finish_reason":finish}]
        }).to_string())
        };
        sse(vec![
            chunk(json!({"role":"assistant","content":text}), Value::Null),
            chunk(json!({}), json!("stop")),
            Event::default().data("[DONE]"),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::{SessionBudgetReached, grouped, requires_contract};
    use serde_json::json;

    #[test]
    fn notice_formats_exact_and_overshot_budgets_without_losing_precision() {
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(1_000), "1,000");
        assert_eq!(grouped(u64::MAX), "18,446,744,073,709,551,615");
        let stop = SessionBudgetReached {
            consumed: 1_018_613,
            limit: 1_000_000,
        };
        assert_eq!(
            stop.to_string(),
            "nan-harness: session token budget reached.\nUsed 1,018,613 of 1,000,000 tokens. No further inference requests will be sent in this session.\nTo continue, start a new nan-harness launch with a higher budget and resume your conversation."
        );
    }

    #[test]
    fn text_formats_and_optional_tools_allow_notices_but_required_contracts_do_not() {
        for request in [
            json!({}),
            json!({"tool_choice":"auto"}),
            json!({"tool_choice":{"type":"none"}}),
            json!({"toolChoice":{"type":"auto"}}),
            json!({"text":{"format":{"type":"text"}}}),
            json!({"output_config":{"effort":"high"}}),
        ] {
            assert!(
                !requires_contract(request.to_string().as_bytes()),
                "{request}"
            );
        }
        for request in [
            json!({"tool_choice":"required"}),
            json!({"tool_choice":{"type":"function","name":"example"}}),
            json!({"toolChoice":{"type":"tool","toolName":"example"}}),
            json!({"output_format":{"type":"json_schema"}}),
            json!({"output_config":{"format":{"type":"json_schema"}}}),
        ] {
            assert!(
                requires_contract(request.to_string().as_bytes()),
                "{request}"
            );
        }
    }
}
