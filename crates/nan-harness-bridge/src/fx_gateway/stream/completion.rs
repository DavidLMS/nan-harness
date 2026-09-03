use super::chunk::{FxObject, FxUsage};
use super::events;
use super::search;
use super::state::FxStreamState;
use super::tools::FxTools;
use crate::error::ApiError;
use crate::fx_gateway::request::ProviderSearchTool;
use crate::upstream::NanClient;
use crate::usage::UsageValues;
use axum::response::sse::Event;
use serde_json::{Value, json};

pub(super) enum StreamOutcome {
    Incomplete,
    Failed,
    Complete,
}

#[derive(Debug, Default)]
pub(super) struct FxCompletion {
    finish_reason: Option<String>,
    input_tokens: u64,
    output_tokens: u64,
    usage: Option<UsageValues>,
}

impl FxCompletion {
    pub(super) fn update_finish_reason(&mut self, finish_reason: Option<String>) {
        if finish_reason.is_some() {
            self.finish_reason = finish_reason;
        }
    }

    pub(super) fn update_usage(&mut self, usage: FxUsage) {
        let usage = UsageValues {
            input: usage.prompt_tokens,
            output: usage.completion_tokens,
            reasoning: usage
                .completion_tokens_details
                .map_or(0, |FxObject(details)| details.reasoning_tokens),
        };
        self.input_tokens = usage.input;
        self.output_tokens = usage.output;
        self.usage = Some(usage);
    }

    pub(super) const fn usage(&self) -> Option<UsageValues> {
        self.usage
    }

    fn finish_reason(
        &self,
        tools: &FxTools,
        provider_search: Option<&ProviderSearchTool>,
    ) -> Value {
        let provider_search_name = provider_search.map(|search| search.name.as_str());
        if tools.has_named(provider_search_name) && tools.all_named(provider_search_name) {
            json!({"unified":"stop"})
        } else if tools.is_empty() {
            match self.finish_reason.as_deref() {
                Some("length") => json!({"unified":"length"}),
                _ => json!({"unified":"stop"}),
            }
        } else {
            json!({"unified":"tool-calls"})
        }
    }
}

pub(super) fn truncated_event() -> Event {
    events::error_message("stream ended before the [DONE] marker")
}

pub(super) async fn finish_events(
    state: &FxStreamState,
    upstream: &NanClient,
    provider_search: Option<&ProviderSearchTool>,
    fallback_query: &str,
) -> Result<Vec<Event>, ApiError> {
    let parsed_tools = state.tools().parse()?;
    let mut events = Vec::new();
    if state.reasoning_started() {
        events.push(events::reasoning_end());
    }
    if state.text_started() {
        events.push(events::text_end());
    }
    events
        .extend(search::tool_events(upstream, provider_search, fallback_query, parsed_tools).await);
    let completion = state.completion();
    let finish_reason = completion.finish_reason(state.tools(), provider_search);
    events.push(events::finish(
        state.model_id(),
        &finish_reason,
        completion.input_tokens,
        completion.output_tokens,
    ));
    Ok(events)
}
