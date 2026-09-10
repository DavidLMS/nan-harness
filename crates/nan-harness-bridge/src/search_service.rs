use crate::error::ApiError;
use nan_harness_search::{
    SearchError, SearxngClient, SearxngConfig, result_summary as summarize_results,
};

pub(crate) use nan_harness_search::{SearchRequest, SearchResult};
#[cfg(test)]
pub(crate) const MAX_QUERY_BYTES: usize = nan_harness_search::MAX_QUERY_BYTES;
/// Builds the launch-scoped `SearXNG` client when managed search is configured.
/// No request is made during bridge construction.
pub(crate) fn build_client(
    config: Option<SearxngConfig>,
) -> Result<Option<SearxngClient>, SearchError> {
    config.map(SearxngClient::new).transpose()
}

pub(crate) fn require_client(
    enabled: bool,
    client: Option<&SearxngClient>,
) -> Result<&SearxngClient, ApiError> {
    if !enabled {
        return Err(ApiError::SearchDisabled);
    }
    client.ok_or(ApiError::SearchUnconfigured)
}

/// Executes a provider-neutral `SearXNG` search.
pub(crate) async fn execute(
    client: &SearxngClient,
    request: SearchRequest,
) -> Result<Vec<SearchResult>, ApiError> {
    client.search(request).await.map_err(map_search_error)
}

pub(crate) fn result_summary(results: &[SearchResult]) -> String {
    summarize_results(results)
}

#[cfg(test)]
fn filter_results(
    results: Vec<SearchResult>,
    max_results: usize,
    allowed_domains: &[String],
    blocked_domains: &[String],
) -> Vec<SearchResult> {
    nan_harness_search::filter_results(results, max_results, allowed_domains, blocked_domains)
}

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
        SearchError::Timeout => {
            ApiError::UpstreamTimeout(crate::error::UpstreamTimeoutPhase::InitialResponse)
        }
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
    use super::{MAX_QUERY_BYTES, SearchResult, filter_results, result_summary};
    use crate::error::ApiError;
    use axum::Router;
    use axum::routing::get;
    use nan_harness_search::{SearchRequest, SearxngClient, SearxngConfig};
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
    }

    #[tokio::test]
    async fn rejects_oversized_queries() {
        let client =
            SearxngClient::new(SearxngConfig::local("http://127.0.0.1:1").expect("valid URL"))
                .expect("client should build");
        let error = super::execute(
            &client,
            SearchRequest {
                query: "x".repeat(MAX_QUERY_BYTES + 1),
                max_results: 1,
                allowed_domains: Vec::new(),
                blocked_domains: Vec::new(),
            },
        )
        .await
        .expect_err("oversized query should fail");
        assert!(matches!(error, ApiError::InvalidRequest(message) if message.contains("exceeds")));
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
