mod chunk;
mod completion;
mod events;
mod search;
mod state;
#[cfg(test)]
mod tests;
mod tools;
mod translation;

use super::request::ProviderSearchTool;
use crate::upstream::NanClient;
use crate::usage::RequestUsageGuard;
use axum::response::sse::Event;
use futures_util::Stream;
use std::convert::Infallible;

pub(super) fn translate(
    response: reqwest::Response,
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
