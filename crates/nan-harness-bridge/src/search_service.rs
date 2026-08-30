use crate::error::ApiError;
use crate::timeouts::map_body_error;
use crate::upstream::NanClient;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::json;

pub(crate) const MAX_QUERY_BYTES: usize = 8 * 1024;
pub(crate) const MAX_RESULTS: usize = 20;
pub(crate) const MAX_URL_BYTES: usize = 8 * 1024;
pub(crate) const MAX_TITLE_CHARS: usize = 500;
pub(crate) const MAX_SNIPPET_CHARS: usize = 2_000;
const MAX_SEARCH_RESPONSE_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SearchRequest {
    pub query: String,
    pub max_results: usize,
    pub allowed_domains: Vec<String>,
    pub blocked_domains: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct SearchResult {
    pub title: String,
    pub url: String,
    #[serde(default)]
    pub snippet: String,
}

#[derive(Debug, Deserialize)]
struct NanSearchResponse {
    #[serde(default)]
    results: Vec<SearchResult>,
}

pub(crate) async fn execute(
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

pub(crate) fn result_summary(results: &[SearchResult]) -> String {
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
                result.title,
                result.url,
                result.snippet
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
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

async fn read_bounded_response(response: &mut reqwest::Response) -> Result<Vec<u8>, ApiError> {
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
    results
        .into_iter()
        .filter_map(|mut result| {
            if result.url.len() > MAX_URL_BYTES {
                return None;
            }
            let url = Url::parse(&result.url).ok()?;
            if !matches!(url.scheme(), "http" | "https") {
                return None;
            }
            let allowed = allowed_domains.is_empty()
                || allowed_domains
                    .iter()
                    .any(|domain| matches_domain(&url, domain));
            let blocked = blocked_domains
                .iter()
                .any(|domain| matches_domain(&url, domain));
            if !allowed || blocked {
                return None;
            }
            result.title = limited(&result.title, MAX_TITLE_CHARS);
            result.snippet = limited(&result.snippet, MAX_SNIPPET_CHARS);
            Some(result)
        })
        .take(max_results)
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

fn ensure_success(response: reqwest::Response) -> Result<reqwest::Response, ApiError> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    Err(ApiError::UpstreamStatus {
        status,
        message: "NaN web search failed".to_owned(),
    })
}

fn limited(value: &str, maximum: usize) -> String {
    value.chars().take(maximum).collect()
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_QUERY_BYTES, MAX_SEARCH_RESPONSE_BYTES, SearchResult, filter_results, matches_domain,
        read_bounded_response, result_summary, validate_query,
    };
    use crate::error::ApiError;
    use axum::body::Bytes;
    use axum::http::Response;
    use futures_util::stream;
    use reqwest::Url;
    use std::convert::Infallible;

    #[test]
    fn enforces_domain_filters_and_result_limits() {
        let results = vec![
            result("Tokio", "https://tokio.rs/tokio/tutorial"),
            result("Rust", "https://www.rust-lang.org/learn"),
        ];

        let filtered = filter_results(results, 20, &["tokio.rs".to_owned()], &[]);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].title, "Tokio");
        assert!(matches_domain(
            &Url::parse("https://docs.rs/tokio/latest").expect("valid URL"),
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
        let mut response = reqwest::Response::from(response);

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
