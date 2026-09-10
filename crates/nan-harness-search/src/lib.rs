#![forbid(unsafe_code)]
#![cfg_attr(not(test), warn(clippy::expect_used, clippy::unwrap_used))]

mod client;
mod config;
mod persistence;
mod types;

pub use client::{SearchAvailability, SearchError, SearxngClient};
pub use config::{SearchConfigError, SearxngConfig, SearxngMode};
pub use persistence::{SearchConfigStore, SearchConfigStoreError};
pub use types::{
    MAX_QUERY_BYTES, MAX_RESPONSE_BYTES, MAX_RESULTS, MAX_SNIPPET_CHARS, MAX_TITLE_CHARS,
    MAX_URL_BYTES, SearchRequest, SearchResult, filter_results, matches_domain, result_summary,
};
