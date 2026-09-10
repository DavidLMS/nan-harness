use crate::error::ApiError;
use crate::timeouts::map_body_error;
use crate::upstream::{NanClient, UpstreamResponse};
use nan_harness_search::{
    SearchError, SearxngClient, filter_results as filter_searxng_results,
    result_summary as summarize_results,
};
use serde::Deserialize;
use serde_json::json;

pub(crate) use nan_harness_search::{SearchRequest, SearchResult};
pub(crate) const MAX_QUERY_BYTES: usize = nan_harness_search::MAX_QUERY_BYTES;
const MAX_RESULTS: usize = nan_harness_search::MAX_RESULTS;
const MAX_SEARCH_RESPONSE_BYTES: usize = nan_harness_search::MAX_RESPONSE_BYTES;

#[derive(Debug, Deserialize)]
struct NanSearchResponse {
    #[serde(default)]
    results: Vec<SearchResult>,
}

/// Compatibility path retained for bridge call sites that still use the NaN
/// upstream client. New wiring should call [`execute`] with a `SearXNG` client.
pub(crate) async fn execute_nan_compat(
    client: &NanClient,
    request: SearchRequest,
) -> Result<Vec<SearchResult>, ApiError> {
    validate_query(&request.query)?;
    let max_results = request.max_results.clamp(1, MAX_RESULTS);
    let response = client
        .search(&json!({
            "query": request.query,
            "count": max_results,
            "fetch_content": false
        }))
        .await?;
    let mut response = ensure_success(response)?;
    let body = read_bounded_response(&mut response).await?;
    let response = serde_json::from_slice::<NanSearchResponse>(&body)
        .map_err(|error| ApiError::InvalidUpstream(format!("invalid web search JSON: {error}")))?;
    Ok(filter_results(
        response.results,
        max_results,
        &request.allowed_domains,
        &request.blocked_domains,
    ))
}

/// Executes a provider-neutral `SearXNG` search.
// This entry point is consumed when the follow-up bridge state wiring selects SearXNG.
#[allow(dead_code)]
pub(crate) async fn execute(
    client: &SearxngClient,
    request: SearchRequest,
) -> Result<Vec<SearchResult>, ApiError> {
    client.search(request).await.map_err(map_search_error)
}

pub(crate) fn result_summary(results: &[SearchResult]) -> String {
    summarize_results(results)
}

fn validate_query(query: &str) -> Result<(), ApiError> {
    if query.trim().is_empty() {
        return Err(ApiError::InvalidRequest(
            "web search query must not be empty".to_owned(),
        ));
    }
    if query.len() > MAX_QUERY_BYTES {
        return Err(ApiError::InvalidRequest(
            "web search query exceeds the supported size".to_owned(),
        ));
    }
    Ok(())
}

async fn read_bounded_response(response: &mut UpstreamResponse) -> Result<Vec<u8>, ApiError> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_SEARCH_RESPONSE_BYTES as u64)
    {
        return Err(search_response_too_large());
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(map_body_error)? {
        if body.len().saturating_add(chunk.len()) > MAX_SEARCH_RESPONSE_BYTES {
            return Err(search_response_too_large());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn search_response_too_large() -> ApiError {
    ApiError::InvalidUpstream("web search response exceeds the 1 MiB limit".to_owned())
}

fn filter_results(
    results: Vec<SearchResult>,
    max_results: usize,
    allowed_domains: &[String],
    blocked_domains: &[String],
) -> Vec<SearchResult> {
    filter_searxng_results(results, max_results, allowed_domains, blocked_domains)
}

fn ensure_success(response: UpstreamResponse) -> Result<UpstreamResponse, ApiError> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    Err(ApiError::UpstreamStatus {
        status,
        message: "NaN web search failed".to_owned(),
    })
}

// Used by the provider-neutral bridge entry point in the follow-up wiring slice.
#[allow(dead_code)]
fn map_search_error(error: SearchError) -> ApiError {
    match error {
        SearchError::InvalidQuery
        | SearchError::QueryTooLarge
        | SearchError::InvalidDomainFilter => ApiError::InvalidRequest(error.to_string()),
        SearchError::HttpStatus(status) => ApiError::UpstreamStatus {
            status: reqwest::StatusCode::from_u16(status)
                .unwrap_or(reqwest::StatusCode::BAD_GATEWAY),
            message: "SearXNG search failed".to_owned(),
        },
        SearchError::Timeout => ApiError::InvalidUpstream("SearXNG search timed out".to_owned()),
        SearchError::ClientBuild
        | SearchError::Transport
        | SearchError::ResponseTooLarge
        | SearchError::InvalidResponse => {
            ApiError::InvalidUpstream(format!("SearXNG search failed [{}]", error.code()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_QUERY_BYTES, MAX_SEARCH_RESPONSE_BYTES, SearchResult, filter_results,
        read_bounded_response, result_summary, validate_query,
    };
    use crate::error::ApiError;
    use crate::upstream::UpstreamResponse;
    use axum::Router;
    use axum::body::Bytes;
    use axum::http::Response;
    use axum::routing::get;
    use futures_util::stream;
    use nan_harness_search::matches_domain as matches_searxng_domain;
    use nan_harness_search::{SearchRequest, SearxngClient, SearxngConfig};
    use std::convert::Infallible;
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn executes_provider_neutral_searxng_json() {
        let app = Router::new().route(
            "/search",
            get(|| async {
                axum::Json(serde_json::json!({
                    "results": [{
                        "title": "Rust",
                        "url": "https://www.rust-lang.org/",
                        "content": "A language empowering everyone."
                    }]
                }))
            }),
        );
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
        let client = SearxngClient::new(
            SearxngConfig::local(&format!("http://{address}"))
                .expect("test endpoint should validate"),
        )
        .expect("client should build");
        let results = super::execute(
            &client,
            SearchRequest {
                query: "rust".to_owned(),
                max_results: 5,
                allowed_domains: Vec::new(),
                blocked_domains: Vec::new(),
            },
        )
        .await
        .expect("SearXNG response should parse");
        assert_eq!(results[0].title, "Rust");
    }

    #[test]
    fn enforces_domain_filters_and_result_limits() {
        let results = vec![
            result("Tokio", "https://tokio.rs/tokio/tutorial"),
            result("Rust", "https://www.rust-lang.org/learn"),
        ];

        let filtered = filter_results(results, 20, &["tokio.rs".to_owned()], &[]);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].title, "Tokio");
        assert!(matches_searxng_domain(
            &reqwest::Url::parse("https://docs.rs/tokio/latest").expect("valid URL"),
            "docs.rs/tokio"
        ));
    }

    #[test]
    fn rejects_oversized_queries() {
        assert!(validate_query(&"x".repeat(MAX_QUERY_BYTES)).is_ok());
        assert!(validate_query(&"x".repeat(MAX_QUERY_BYTES + 1)).is_err());
        assert!(validate_query(" \n\t").is_err());
    }

    #[tokio::test]
    async fn rejects_a_chunked_response_before_buffering_past_the_limit() {
        let stream = stream::iter([
            Ok::<Bytes, Infallible>(Bytes::from(vec![b' '; MAX_SEARCH_RESPONSE_BYTES])),
            Ok(Bytes::from_static(b"x")),
        ]);
        let response = Response::builder()
            .body(reqwest::Body::wrap_stream(stream))
            .expect("test response should build");
        let mut response = UpstreamResponse::uncoordinated(reqwest::Response::from(response));

        assert!(matches!(
            read_bounded_response(&mut response).await,
            Err(ApiError::InvalidUpstream(message)) if message.contains("1 MiB")
        ));
    }

    #[test]
    fn rejects_invalid_urls_and_limits_rendered_fields() {
        let results = vec![
            SearchResult {
                title: "x".repeat(600),
                url: "https://example.test/result".to_owned(),
                snippet: "y".repeat(2_100),
            },
            result("local", "file:///tmp/result"),
        ];

        let filtered = filter_results(results, 1, &[], &[]);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].title.chars().count(), 500);
        assert_eq!(filtered[0].snippet.chars().count(), 2_000);
    }

    #[test]
    fn renders_a_shared_result_summary() {
        let summary = result_summary(&[SearchResult {
            title: "Tokio runtime".to_owned(),
            url: "https://tokio.rs".to_owned(),
            snippet: "An asynchronous runtime for Rust.".to_owned(),
        }]);

        assert!(summary.contains("An asynchronous runtime for Rust."));
        assert!(summary.contains("https://tokio.rs"));
    }

    fn result(title: &str, url: &str) -> SearchResult {
        SearchResult {
            title: title.to_owned(),
            url: url.to_owned(),
            snippet: String::new(),
        }
    }
}
