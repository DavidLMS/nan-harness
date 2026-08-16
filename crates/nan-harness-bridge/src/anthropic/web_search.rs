use crate::anthropic::request::WebSearchInvocation;
use crate::upstream::NanClient;
use axum::response::sse::{Event, Sse};
use axum::response::{IntoResponse, Response};
use futures_util::stream;
use reqwest::{StatusCode, Url};
use serde::Deserialize;
use serde_json::{Value, json};
use std::convert::Infallible;

const TOOL_USE_ID: &str = "srvtoolu_nan_search";

#[derive(Debug, Deserialize)]
struct NanSearchResponse {
    #[serde(default)]
    results: Vec<NanSearchResult>,
}

#[derive(Debug, Deserialize)]
struct NanSearchResult {
    title: String,
    url: String,
    #[serde(default)]
    snippet: String,
}

enum SearchOutcome {
    Success(Vec<NanSearchResult>),
    Error(&'static str),
}

pub(crate) async fn execute(
    client: &NanClient,
    invocation: WebSearchInvocation,
    model: &str,
) -> Response {
    let body = json!({
        "query": invocation.query,
        "count": invocation.max_results,
        "fetch_content": false
    });
    let outcome = match client.search(&body).await {
        Ok(response) if response.status().is_success() => response
            .json::<NanSearchResponse>()
            .await
            .map_or(SearchOutcome::Error("unavailable"), |response| {
                SearchOutcome::Success(filter_results(
                    response.results,
                    &invocation.allowed_domains,
                    &invocation.blocked_domains,
                ))
            }),
        Ok(response) => SearchOutcome::Error(error_code(response.status())),
        Err(_) => SearchOutcome::Error("unavailable"),
    };

    if invocation.stream {
        streaming_response(&invocation.query, &outcome, model)
    } else {
        json_response(&invocation.query, &outcome, model)
    }
}

fn filter_results(
    results: Vec<NanSearchResult>,
    allowed_domains: &[String],
    blocked_domains: &[String],
) -> Vec<NanSearchResult> {
    results
        .into_iter()
        .filter(|result| {
            let Ok(url) = Url::parse(&result.url) else {
                return false;
            };
            if !matches!(url.scheme(), "http" | "https") {
                return false;
            }
            let allowed = allowed_domains.is_empty()
                || allowed_domains
                    .iter()
                    .any(|domain| matches_domain(&url, domain));
            let blocked = blocked_domains
                .iter()
                .any(|domain| matches_domain(&url, domain));
            allowed && !blocked
        })
        .collect()
}

fn matches_domain(url: &Url, domain: &str) -> bool {
    let (hostname, path) = domain
        .split_once('/')
        .map_or((domain, None), |(hostname, path)| (hostname, Some(path)));
    let Some(url_hostname) = url.host_str() else {
        return false;
    };
    let hostname = hostname.to_ascii_lowercase();
    let url_hostname = url_hostname.to_ascii_lowercase();
    let host_matches = url_hostname == hostname || url_hostname.ends_with(&format!(".{hostname}"));
    let path_matches = path.is_none_or(|path| url.path().starts_with(&format!("/{path}")));
    host_matches && path_matches
}

fn error_code(status: StatusCode) -> &'static str {
    match status {
        StatusCode::TOO_MANY_REQUESTS => "too_many_requests",
        status if status.is_client_error() => "invalid_tool_input",
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
        let summary = result_summary(results);
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
        content.push(json!({"type": "text", "text": result_summary(results)}));
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
                        "title": limited(&result.title, 500),
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

fn result_summary(results: &[NanSearchResult]) -> String {
    if results.is_empty() {
        return "No web search results were found.".to_owned();
    }
    results
        .iter()
        .enumerate()
        .map(|(index, result)| {
            format!(
                "{}. {}\nURL: {}\n{}",
                index + 1,
                limited(&result.title, 500),
                result.url,
                limited(&result.snippet, 2_000)
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn usage(outcome: &SearchOutcome) -> Value {
    let count = usize::from(matches!(outcome, SearchOutcome::Success(_)));
    json!({
        "input_tokens": 0,
        "output_tokens": 0,
        "server_tool_use": {"web_search_requests": count}
    })
}

fn limited(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
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
    use super::{NanSearchResult, filter_results, matches_domain, result_summary};
    use reqwest::Url;

    #[test]
    fn enforces_domain_filters_on_nan_results() {
        let results = vec![
            result("Tokio", "https://tokio.rs/tokio/tutorial"),
            result("Rust", "https://www.rust-lang.org/learn"),
        ];

        let filtered = filter_results(results, &["tokio.rs".to_owned()], &[]);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].title, "Tokio");
        assert!(matches_domain(
            &Url::parse("https://docs.rs/tokio/latest").expect("valid URL"),
            "docs.rs/tokio"
        ));
    }

    #[test]
    fn includes_search_snippets_in_the_claude_code_result() {
        let summary = result_summary(&[NanSearchResult {
            title: "Tokio runtime".to_owned(),
            url: "https://tokio.rs".to_owned(),
            snippet: "An asynchronous runtime for Rust.".to_owned(),
        }]);

        assert!(summary.contains("An asynchronous runtime for Rust."));
        assert!(summary.contains("https://tokio.rs"));
    }

    fn result(title: &str, url: &str) -> NanSearchResult {
        NanSearchResult {
            title: title.to_owned(),
            url: url.to_owned(),
            snippet: String::new(),
        }
    }
}
