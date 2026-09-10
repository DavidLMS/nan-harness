use crate::config::SearxngConfig;
use crate::types::{
    MAX_QUERY_BYTES, MAX_RESPONSE_BYTES, SearchRequest, SearchResult, filter_results,
};
use reqwest::Client;
use serde_json::Value;
use std::time::Duration;
use thiserror::Error;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone)]
pub struct SearxngClient {
    client: Client,
    config: SearxngConfig,
}

impl std::fmt::Debug for SearxngClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SearxngClient")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl SearxngClient {
    /// Builds a client with bounded connection and response waits.
    ///
    /// # Errors
    ///
    /// Returns [`SearchError::ClientBuild`] if the HTTP client cannot be built.
    pub fn new(config: SearxngConfig) -> Result<Self, SearchError> {
        let client = Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(|_| SearchError::ClientBuild)?;
        Ok(Self { client, config })
    }

    #[must_use]
    pub const fn config(&self) -> &SearxngConfig {
        &self.config
    }

    /// Queries `SearXNG`'s JSON endpoint and applies local result validation and filters.
    ///
    /// Invalid individual results are ignored so one malformed engine result does
    /// not discard usable results from the rest of the response.
    ///
    /// # Errors
    ///
    /// Returns [`SearchError`] for invalid request data, transport failures,
    /// non-success HTTP statuses, oversized bodies, or invalid response data.
    pub async fn search(&self, request: SearchRequest) -> Result<Vec<SearchResult>, SearchError> {
        validate_request(&request)?;
        let max_results = request.max_results.clamp(1, crate::types::MAX_RESULTS);
        let mut search_url = self.config.search_url();
        search_url
            .query_pairs_mut()
            .append_pair("q", &request.query)
            .append_pair("format", "json")
            .append_pair("number_of_results", &max_results.to_string());
        let response = self
            .client
            .get(search_url)
            .send()
            .await
            .map_err(|error| map_transport_error(&error))?;
        let status = response.status();
        if !status.is_success() {
            return Err(SearchError::HttpStatus(status.as_u16()));
        }
        let body = read_bounded_response(response).await?;
        let value: Value =
            serde_json::from_slice(&body).map_err(|_| SearchError::InvalidResponse)?;
        let results = value
            .get("results")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(parse_result)
            .collect::<Vec<_>>();
        Ok(filter_results(
            results,
            max_results,
            &request.allowed_domains,
            &request.blocked_domains,
        ))
    }
}

fn validate_request(request: &SearchRequest) -> Result<(), SearchError> {
    if request.query.trim().is_empty() {
        return Err(SearchError::InvalidQuery);
    }
    if request.query.len() > MAX_QUERY_BYTES {
        return Err(SearchError::QueryTooLarge);
    }
    for domain in request
        .allowed_domains
        .iter()
        .chain(request.blocked_domains.iter())
    {
        if !is_valid_domain_filter(domain) {
            return Err(SearchError::InvalidDomainFilter);
        }
    }
    Ok(())
}

fn is_valid_domain_filter(domain: &str) -> bool {
    if domain.is_empty() || domain.chars().any(char::is_whitespace) || domain.contains("//") {
        return false;
    }
    let (host, path) = domain
        .split_once('/')
        .map_or((domain, None), |(host, path)| (host, Some(path)));
    !host.is_empty()
        && !host.starts_with('.')
        && !host.contains([':', '@', '?', '#'])
        && !path.is_some_and(str::is_empty)
        && path.is_none_or(|value| !value.contains(['?', '#']))
}

fn parse_result(value: &Value) -> Option<SearchResult> {
    let title = value.get("title").and_then(Value::as_str)?.trim();
    let url = value.get("url").and_then(Value::as_str)?.trim();
    if title.is_empty() || url.is_empty() {
        return None;
    }
    let snippet = value
        .get("content")
        .or_else(|| value.get("snippet"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    Some(SearchResult {
        title: title.to_owned(),
        url: url.to_owned(),
        snippet: snippet.to_owned(),
    })
}

async fn read_bounded_response(response: reqwest::Response) -> Result<Vec<u8>, SearchError> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
    {
        return Err(SearchError::ResponseTooLarge);
    }
    let mut response = response;
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| map_transport_error(&error))?
    {
        if body.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
            return Err(SearchError::ResponseTooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn map_transport_error(error: &reqwest::Error) -> SearchError {
    if error.is_timeout() {
        SearchError::Timeout
    } else {
        SearchError::Transport
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchAvailability {
    Available,
    Unconfigured,
    Unavailable,
    Disabled,
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum SearchError {
    #[error("could not build the SearXNG HTTP client")]
    ClientBuild,
    #[error("web search query must not be empty")]
    InvalidQuery,
    #[error("web search query exceeds the supported size")]
    QueryTooLarge,
    #[error("web search domain filters are invalid")]
    InvalidDomainFilter,
    #[error("SearXNG request timed out")]
    Timeout,
    #[error("SearXNG request failed before a response was received")]
    Transport,
    #[error("SearXNG returned HTTP status {0}")]
    HttpStatus(u16),
    #[error("SearXNG response exceeds the 1 MiB limit")]
    ResponseTooLarge,
    #[error("SearXNG returned invalid JSON or result data")]
    InvalidResponse,
}

impl SearchError {
    #[must_use]
    pub const fn availability(self) -> SearchAvailability {
        match self {
            Self::InvalidQuery | Self::QueryTooLarge | Self::InvalidDomainFilter => {
                SearchAvailability::Available
            }
            Self::ClientBuild
            | Self::Timeout
            | Self::Transport
            | Self::HttpStatus(_)
            | Self::ResponseTooLarge
            | Self::InvalidResponse => SearchAvailability::Unavailable,
        }
    }

    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::ClientBuild => "client_build",
            Self::InvalidQuery => "invalid_query",
            Self::QueryTooLarge => "query_too_large",
            Self::InvalidDomainFilter => "invalid_domain_filter",
            Self::Timeout => "timeout",
            Self::Transport => "transport",
            Self::HttpStatus(_) => "http_status",
            Self::ResponseTooLarge => "response_too_large",
            Self::InvalidResponse => "invalid_response",
        }
    }

    #[must_use]
    pub const fn is_retryable(self) -> bool {
        matches!(
            self,
            Self::Timeout | Self::Transport | Self::HttpStatus(429 | 500..=599)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{SearchError, SearxngClient};
    use crate::{SearchRequest, SearxngConfig};
    use axum::Router;
    use axum::body::Body;
    use axum::http::{Response, StatusCode};
    use axum::routing::get;
    use std::convert::Infallible;
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn parses_searxng_json_and_skips_partial_results() {
        let app = Router::new().route(
            "/search",
            get(|| async {
                axum::Json(serde_json::json!({
                    "results": [
                        {"title":"Rust","url":"https://www.rust-lang.org/","content":"Rust language"},
                        {"title":"missing url","content":"ignored"},
                        {"url":"https://example.test/","content":"missing title"},
                        {"title":"bad scheme","url":"file:///tmp/no","content":"ignored"}
                    ]
                }))
            }),
        );
        let client = test_client(app).await;
        let results = client
            .search(SearchRequest {
                query: "rust".to_owned(),
                max_results: 10,
                allowed_domains: Vec::new(),
                blocked_domains: Vec::new(),
            })
            .await
            .expect("valid response should parse");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].snippet, "Rust language");
    }

    #[tokio::test]
    async fn clamps_result_limit_and_filters_domains() {
        let app = Router::new().route(
            "/search",
            get(|| async {
                axum::Json(serde_json::json!({
                    "results": [
                        {"title":"one","url":"https://allowed.test/one"},
                        {"title":"two","url":"https://blocked.test/two"},
                        {"title":"three","url":"https://allowed.test/three"}
                    ]
                }))
            }),
        );
        let client = test_client(app).await;
        let results = client
            .search(SearchRequest {
                query: "limit".to_owned(),
                max_results: usize::MAX,
                allowed_domains: vec!["allowed.test".to_owned()],
                blocked_domains: vec!["allowed.test/three".to_owned()],
            })
            .await
            .expect("valid response should parse");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "one");
    }

    #[tokio::test]
    async fn rejects_oversized_response_and_invalid_json() {
        let app = Router::new().route(
            "/search",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .body(Body::from("not json"))
                    .expect("response should build")
            }),
        );
        let client = test_client(app).await;
        let error = client
            .search(request())
            .await
            .expect_err("invalid JSON should fail");
        assert_eq!(error, SearchError::InvalidResponse);
    }

    #[tokio::test]
    async fn rejects_a_response_larger_than_the_body_limit() {
        let app = Router::new().route(
            "/search",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .body(Body::from(vec![b'x'; crate::MAX_RESPONSE_BYTES + 1]))
                    .expect("response should build")
            }),
        );
        let client = test_client(app).await;
        let error = client
            .search(request())
            .await
            .expect_err("oversized body should fail");
        assert_eq!(error, SearchError::ResponseTooLarge);
    }

    #[tokio::test]
    async fn never_includes_credentials_in_error_text_or_debug() {
        let config = SearxngConfig::local("http://127.0.0.1:1").expect("URL should validate");
        let client = SearxngClient::new(config).expect("client should build");
        let error = client
            .search(SearchRequest {
                query: "api-key=super-secret".to_owned(),
                ..request()
            })
            .await
            .expect_err("unreachable endpoint should fail");
        assert!(!error.to_string().contains("super-secret"));
        assert!(!format!("{client:?}").contains("super-secret"));
    }

    fn request() -> SearchRequest {
        SearchRequest {
            query: "query".to_owned(),
            max_results: 10,
            allowed_domains: Vec::new(),
            blocked_domains: Vec::new(),
        }
    }

    async fn test_client(app: Router) -> SearxngClient {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener should bind");
        let address = listener
            .local_addr()
            .expect("listener should have an address");
        tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("test server should run");
        });
        SearxngClient::new(
            SearxngConfig::local(&format!("http://{address}"))
                .expect("test endpoint should validate"),
        )
        .expect("client should build")
    }

    #[allow(dead_code)]
    fn _infallible(_: Infallible) {}
}
