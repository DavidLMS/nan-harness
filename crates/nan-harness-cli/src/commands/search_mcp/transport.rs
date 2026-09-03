use super::error::SearchMcpError;
use super::response_limits::read_response_body;
use nan_harness_core::SecretValue;
use reqwest::Url;
use serde_json::Value;
use std::net::IpAddr;
use std::time::Duration;

pub(super) struct SearchTransport {
    endpoint: Url,
    token: SecretValue,
    client: reqwest::Client,
}

impl SearchTransport {
    pub(super) fn new(endpoint: Url, token_environment: String) -> Result<Self, SearchMcpError> {
        let token = std::env::var(&token_environment)
            .map_err(|_| SearchMcpError::MissingToken(token_environment))?;
        let token = SecretValue::new(token).map_err(SearchMcpError::InvalidToken)?;
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_mins(1))
            .build()
            .map_err(SearchMcpError::BuildClient)?;
        Ok(Self {
            endpoint,
            token,
            client,
        })
    }

    pub(super) async fn search(&self, body: &Value) -> Result<Value, &'static str> {
        let request = self.token.with_secret(|token| {
            self.client
                .post(self.endpoint.clone())
                .bearer_auth(token)
                .json(body)
        });
        let mut response = request.send().await.map_err(|_| "NH-SEARCH-MCP-006")?;
        if !response.status().is_success() {
            return Err("NH-SEARCH-MCP-007");
        }
        let body = read_response_body(&mut response).await?;
        serde_json::from_slice(&body).map_err(|_| "NH-SEARCH-MCP-009")
    }
}

pub(super) fn provider_search_endpoint(mut base_url: Url) -> Result<Url, SearchMcpError> {
    if !base_url.username().is_empty()
        || base_url.password().is_some()
        || base_url.query().is_some()
        || base_url.fragment().is_some()
    {
        return Err(SearchMcpError::UnsafeEndpoint);
    }
    let local_http = base_url.scheme() == "http"
        && base_url.host().is_some_and(|host| match host {
            url::Host::Domain(domain) => domain.eq_ignore_ascii_case("localhost"),
            url::Host::Ipv4(address) => IpAddr::V4(address).is_loopback(),
            url::Host::Ipv6(address) => IpAddr::V6(address).is_loopback(),
        });
    if base_url.scheme() != "https" && !local_http {
        return Err(SearchMcpError::UnsafeEndpoint);
    }
    let path = format!("{}/search", base_url.path().trim_end_matches('/'));
    base_url.set_path(&path);
    Ok(base_url)
}

pub(super) fn validate_endpoint(endpoint: &Url) -> Result<(), SearchMcpError> {
    if endpoint.scheme() != "http"
        || !endpoint.username().is_empty()
        || endpoint.password().is_some()
        || endpoint.query().is_some()
        || endpoint.fragment().is_some()
        || endpoint.path() != "/v1/search"
    {
        return Err(SearchMcpError::UnsafeEndpoint);
    }
    let loopback = endpoint.host().is_some_and(|host| match host {
        url::Host::Domain(domain) => domain.eq_ignore_ascii_case("localhost"),
        url::Host::Ipv4(address) => IpAddr::V4(address).is_loopback(),
        url::Host::Ipv6(address) => IpAddr::V6(address).is_loopback(),
    });
    if loopback {
        Ok(())
    } else {
        Err(SearchMcpError::UnsafeEndpoint)
    }
}

#[cfg(test)]
mod tests {
    use super::{provider_search_endpoint, validate_endpoint};
    use reqwest::Url;

    #[test]
    fn local_endpoint_accepts_only_authenticated_loopback_search_routes() {
        for endpoint in [
            "http://127.0.0.1:4312/v1/search",
            "http://127.255.255.254:4312/v1/search",
            "http://[::1]:4312/v1/search",
            "http://localhost:4312/v1/search",
            "http://LOCALHOST:4312/v1/search",
        ] {
            validate_endpoint(&Url::parse(endpoint).expect("URL")).expect("loopback endpoint");
        }
    }

    #[test]
    fn local_endpoint_rejects_non_loopback_or_decorated_urls() {
        for endpoint in [
            "https://127.0.0.1:4312/v1/search",
            "http://example.com/v1/search",
            "http://127.0.0.1:4312/v1/models",
            "http://127.0.0.1:4312/v1/search?target=other",
            "http://127.0.0.1:4312/v1/search#fragment",
            "http://user:secret@127.0.0.1:4312/v1/search",
        ] {
            assert!(
                validate_endpoint(&Url::parse(endpoint).expect("URL")).is_err(),
                "{endpoint}"
            );
        }
    }

    #[test]
    fn provider_mode_builds_a_search_endpoint_for_safe_base_urls() {
        assert_eq!(
            provider_search_endpoint(Url::parse("https://api.nan.builders/v1").expect("URL"))
                .expect("provider URL")
                .as_str(),
            "https://api.nan.builders/v1/search"
        );
        assert_eq!(
            provider_search_endpoint(Url::parse("http://localhost:8080/v1/").expect("URL"))
                .expect("loopback URL")
                .as_str(),
            "http://localhost:8080/v1/search"
        );
    }

    #[test]
    fn provider_mode_rejects_unsafe_base_urls() {
        for base_url in [
            "http://api.nan.builders/v1",
            "https://user:secret@api.nan.builders/v1",
            "https://api.nan.builders/v1?target=other",
            "https://api.nan.builders/v1#fragment",
        ] {
            assert!(
                provider_search_endpoint(Url::parse(base_url).expect("URL")).is_err(),
                "{base_url}"
            );
        }
    }
}
