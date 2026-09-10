use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use url::Url;

pub const MAX_QUERY_BYTES: usize = 8 * 1024;
pub const MAX_RESULTS: usize = 20;
pub const MAX_URL_BYTES: usize = 8 * 1024;
pub const MAX_TITLE_CHARS: usize = 500;
pub const MAX_SNIPPET_CHARS: usize = 2_000;
pub const MAX_RESPONSE_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchRequest {
    pub query: String,
    pub max_results: usize,
    #[serde(default)]
    pub allowed_domains: Vec<String>,
    #[serde(default)]
    pub blocked_domains: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    #[serde(default)]
    pub snippet: String,
}

pub fn filter_results(
    results: impl IntoIterator<Item = SearchResult>,
    max_results: usize,
    allowed_domains: &[String],
    blocked_domains: &[String],
) -> Vec<SearchResult> {
    let max_results = max_results.clamp(1, MAX_RESULTS);
    results
        .into_iter()
        .filter_map(|mut result| {
            if result.url.len() > MAX_URL_BYTES {
                return None;
            }
            let url = Url::parse(&result.url).ok()?;
            if !matches!(url.scheme(), "http" | "https")
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
            {
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

#[must_use]
pub fn matches_domain(url: &Url, domain: &str) -> bool {
    let Some((hostname, path)) = parse_domain(domain) else {
        return false;
    };
    let Some(url_hostname) = url.host_str() else {
        return false;
    };
    let hostname = hostname.to_ascii_lowercase();
    let url_hostname = url_hostname.to_ascii_lowercase();
    let host_matches = url_hostname == hostname || url_hostname.ends_with(&format!(".{hostname}"));
    let path_matches = path.is_none_or(|path| {
        let path = format!("/{path}");
        url.path() == path || url.path().starts_with(&format!("{path}/"))
    });
    host_matches && path_matches
}

fn parse_domain(domain: &str) -> Option<(Cow<'_, str>, Option<&str>)> {
    if domain.is_empty() || domain.chars().any(char::is_whitespace) || domain.contains("//") {
        return None;
    }
    let (hostname, path) = domain
        .split_once('/')
        .map_or((domain, None), |(host, path)| {
            (host, Some(path.trim_end_matches('/')))
        });
    if hostname.is_empty()
        || hostname.starts_with('.')
        || hostname.contains(':')
        || hostname.contains('@')
        || hostname.starts_with('.') && hostname.len() == 1
        || path.is_some_and(str::is_empty)
    {
        return None;
    }
    Some((Cow::Owned(hostname.to_ascii_lowercase()), path))
}

fn limited(value: &str, maximum: usize) -> String {
    value.chars().take(maximum).collect()
}

#[must_use]
pub fn result_summary(results: &[SearchResult]) -> String {
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

#[cfg(test)]
mod tests {
    use super::{MAX_SNIPPET_CHARS, MAX_TITLE_CHARS, SearchResult, filter_results, matches_domain};
    use url::Url;

    #[test]
    fn filters_domains_and_preserves_result_limits() {
        let results = vec![
            SearchResult {
                title: "Tokio".to_owned(),
                url: "https://tokio.rs/tokio/tutorial".to_owned(),
                snippet: String::new(),
            },
            SearchResult {
                title: "Rust".to_owned(),
                url: "https://www.rust-lang.org/learn".to_owned(),
                snippet: String::new(),
            },
        ];
        let filtered = filter_results(results, 1, &["tokio.rs/tokio".to_owned()], &[]);
        assert_eq!(filtered.len(), 1);
        assert!(matches_domain(
            &Url::parse("https://docs.rs/tokio/latest").expect("valid URL"),
            "docs.rs/tokio"
        ));
        assert!(!matches_domain(
            &Url::parse("https://notdocs.rs/tokio").expect("valid URL"),
            "docs.rs"
        ));
    }

    #[test]
    fn truncates_rendered_fields_and_skips_invalid_urls() {
        let results = vec![
            SearchResult {
                title: "x".repeat(600),
                url: "https://example.test/result".to_owned(),
                snippet: "y".repeat(2_100),
            },
            SearchResult {
                title: "local".to_owned(),
                url: "file:///tmp/result".to_owned(),
                snippet: String::new(),
            },
            SearchResult {
                title: "credential URL".to_owned(),
                url: "https://user:secret@example.test/result".to_owned(),
                snippet: String::new(),
            },
        ];
        let filtered = filter_results(results, 20, &[], &[]);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].title.chars().count(), MAX_TITLE_CHARS);
        assert_eq!(filtered[0].snippet.chars().count(), MAX_SNIPPET_CHARS);
    }
}
