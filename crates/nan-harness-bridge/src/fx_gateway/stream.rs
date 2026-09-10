mod chunk;
mod completion;
mod events;
#[cfg(test)]
mod framing_tests;
mod search;
mod state;
#[cfg(test)]
mod tests;
mod tools;
mod translation;

use super::request::ProviderSearchTool;
use crate::upstream::{NanClient, UpstreamResponse};
use crate::usage::RequestUsageGuard;
use axum::response::sse::Event;
use futures_util::Stream;
use std::convert::Infallible;

pub(super) fn translate(
    response: UpstreamResponse,
    model_id: String,
    upstream: NanClient,
    provider_search: Option<ProviderSearchTool>,
    fallback_query: String,
    usage_guard: RequestUsageGuard,
) -> impl Stream<Item = Result<Event, Infallible>> {
    translation::translate(
        response,
        model_id,
        upstream,
        provider_search,
        fallback_query,
        usage_guard,
    )
}

pub(crate) fn budget_notice(
    stop: crate::session_budget::SessionBudgetReached,
    model: &str,
) -> Vec<Event> {
    vec![
        events::response_metadata(model),
        events::text_start(),
        events::text_delta(&stop.to_string()),
        events::text_end(),
        events::finish(model, &serde_json::json!({"unified":"stop"}), 0, 0),
    ]
}
