use crate::anthropic::request::WebSearchInvocation;
use crate::error::ApiError;
use crate::search_service::{self, SearchRequest, SearchResult};
use crate::upstream::NanClient;
use axum::response::sse::{Event, Sse};
use axum::response::{IntoResponse, Response};
use futures_util::stream;
use serde_json::{Value, json};
use std::convert::Infallible;

const TOOL_USE_ID: &str = "srvtoolu_nan_search";

enum SearchOutcome {
    Success(Vec<SearchResult>),
    Error(&'static str),
}

pub(crate) async fn execute(
    client: &NanClient,
    invocation: WebSearchInvocation,
    model: &str,
) -> Response {
    let outcome = match search_service::execute(
        client,
        SearchRequest {
            query: invocation.query.clone(),
            max_results: invocation.max_results,
            allowed_domains: invocation.allowed_domains,
            blocked_domains: invocation.blocked_domains,
        },
    )
    .await
    {
        Ok(results) => SearchOutcome::Success(results),
        Err(error) => SearchOutcome::Error(error_code(&error)),
    };

    if invocation.stream {
        streaming_response(&invocation.query, &outcome, model)
    } else {
        json_response(&invocation.query, &outcome, model)
    }
}

fn error_code(error: &ApiError) -> &'static str {
    match error {
        ApiError::UpstreamStatus { status, .. }
            if *status == reqwest::StatusCode::TOO_MANY_REQUESTS =>
        {
            "too_many_requests"
        }
        ApiError::InvalidRequest(_) => "invalid_tool_input",
        ApiError::UpstreamStatus { status, .. } if status.is_client_error() => "invalid_tool_input",
        _ => "unavailable",
    }
}

fn json_response(query: &str, outcome: &SearchOutcome, model: &str) -> Response {
    axum::Json(json!({
        "id": "msg_nan_web_search",
        "type": "message",
        "role": "assistant",
        "model": model,
        "content": content_blocks(query, outcome),
        "stop_reason": "end_turn",
        "stop_sequence": null,
        "usage": usage(outcome)
    }))
    .into_response()
}

fn streaming_response(query: &str, outcome: &SearchOutcome, model: &str) -> Response {
    let mut events = vec![anthropic_event(
        "message_start",
        &json!({
            "type": "message_start",
            "message": {
                "id": "msg_nan_web_search",
                "type": "message",
                "role": "assistant",
                "model": model,
                "content": [],
                "stop_reason": null,
                "stop_sequence": null,
                "usage": {"input_tokens": 0, "output_tokens": 0}
            }
        }),
    )];
    events.push(anthropic_event(
        "content_block_start",
        &json!({
            "type": "content_block_start",
            "index": 0,
            "content_block": {
                "type": "server_tool_use",
                "id": TOOL_USE_ID,
                "name": "web_search",
                "input": {}
            }
        }),
    ));
    events.push(anthropic_event(
        "content_block_delta",
        &json!({
            "type": "content_block_delta",
            "index": 0,
            "delta": {
                "type": "input_json_delta",
                "partial_json": json!({"query": query}).to_string()
            }
        }),
    ));
    events.push(content_block_stop(0));
    events.push(anthropic_event(
        "content_block_start",
        &json!({
            "type": "content_block_start",
            "index": 1,
            "content_block": result_block(outcome)
        }),
    ));
    events.push(content_block_stop(1));
    if let SearchOutcome::Success(results) = outcome {
        let summary = search_service::result_summary(results);
        events.push(anthropic_event(
            "content_block_start",
            &json!({
                "type": "content_block_start",
                "index": 2,
                "content_block": {"type": "text", "text": ""}
            }),
        ));
        events.push(anthropic_event(
            "content_block_delta",
            &json!({
                "type": "content_block_delta",
                "index": 2,
                "delta": {"type": "text_delta", "text": summary}
            }),
        ));
        events.push(content_block_stop(2));
    }
    events.push(anthropic_event(
        "message_delta",
        &json!({
            "type": "message_delta",
            "delta": {"stop_reason": "end_turn", "stop_sequence": null},
            "usage": usage(outcome)
        }),
    ));
    events.push(anthropic_event(
        "message_stop",
        &json!({"type": "message_stop"}),
    ));

    Sse::new(stream::iter(
        events.into_iter().map(Ok::<Event, Infallible>),
    ))
    .into_response()
}

fn content_blocks(query: &str, outcome: &SearchOutcome) -> Vec<Value> {
    let mut content = vec![
        json!({
            "type": "server_tool_use",
            "id": TOOL_USE_ID,
            "name": "web_search",
            "input": {"query": query}
        }),
        result_block(outcome),
    ];
    if let SearchOutcome::Success(results) = outcome {
        content.push(json!({
            "type": "text",
            "text": search_service::result_summary(results)
        }));
    }
    content
}

fn result_block(outcome: &SearchOutcome) -> Value {
    let content = match outcome {
        SearchOutcome::Success(results) => Value::Array(
            results
                .iter()
                .map(|result| {
                    json!({
                        "type": "web_search_result",
                        "title": result.title,
                        "url": result.url
                    })
                })
                .collect(),
        ),
        SearchOutcome::Error(error_code) => json!({
            "type": "web_search_tool_result_error",
            "error_code": error_code
        }),
    };
    json!({
        "type": "web_search_tool_result",
        "tool_use_id": TOOL_USE_ID,
        "content": content
    })
}

fn usage(outcome: &SearchOutcome) -> Value {
    let count = usize::from(matches!(outcome, SearchOutcome::Success(_)));
    json!({
        "input_tokens": 0,
        "output_tokens": 0,
        "server_tool_use": {"web_search_requests": count}
    })
}

fn content_block_stop(index: usize) -> Event {
    anthropic_event(
        "content_block_stop",
        &json!({"type": "content_block_stop", "index": index}),
    )
}

fn anthropic_event(name: &'static str, data: &Value) -> Event {
    Event::default().event(name).data(data.to_string())
}

#[cfg(test)]
mod tests {
    use crate::search_service::{SearchResult, result_summary};

    #[test]
    fn includes_search_snippets_in_the_claude_code_result() {
        let summary = result_summary(&[SearchResult {
            title: "Tokio runtime".to_owned(),
            url: "https://tokio.rs".to_owned(),
            snippet: "An asynchronous runtime for Rust.".to_owned(),
        }]);

        assert!(summary.contains("An asynchronous runtime for Rust."));
        assert!(summary.contains("https://tokio.rs"));
    }
}
