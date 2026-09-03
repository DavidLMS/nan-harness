use super::events;
use super::tools::ParsedFxTool;
use crate::fx_gateway::request::ProviderSearchTool;
use crate::search_service::{self, SearchRequest};
use crate::upstream::NanClient;
use axum::response::sse::Event;
use serde_json::{Value, json};

pub(super) async fn tool_events(
    upstream: &NanClient,
    provider_search: Option<&ProviderSearchTool>,
    fallback_query: &str,
    parsed_tools: Vec<ParsedFxTool>,
) -> Vec<Event> {
    let mut output = Vec::new();
    for tool in parsed_tools {
        let matching_search = provider_search.filter(|search| search.name == tool.name);
        let mut tool_event = json!({
            "type":"tool-call",
            "toolCallId":tool.id,
            "toolName":tool.name,
            "input":tool.input
        });
        if matching_search.is_some() {
            tool_event["providerExecuted"] = json!(true);
        }
        output.push(events::event(&tool_event));
        if let Some(search) = matching_search {
            let query = provider_search_query(&tool_event["input"], fallback_query);
            let result = execute_provider_search(upstream, search, query).await;
            output.push(events::event(&json!({
                "type":"tool-result",
                "toolCallId":tool_event["toolCallId"],
                "result":result
            })));
        }
    }
    output
}

fn provider_search_query<'a>(input: &'a Value, fallback_query: &'a str) -> &'a str {
    input
        .get("query")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(fallback_query)
}

async fn execute_provider_search(
    upstream: &NanClient,
    provider: &ProviderSearchTool,
    query: &str,
) -> Value {
    match search_service::execute(
        upstream,
        SearchRequest {
            query: query.to_owned(),
            max_results: provider.max_results,
            allowed_domains: provider.allowed_domains.clone(),
            blocked_domains: provider.blocked_domains.clone(),
        },
    )
    .await
    {
        Ok(results) => json!({"results": results}),
        Err(error) => {
            json!({
                "error": {
                    "type": "search_failed",
                    "message": format!("web search request failed [{}]", error.code())
                }
            })
        }
    }
}
