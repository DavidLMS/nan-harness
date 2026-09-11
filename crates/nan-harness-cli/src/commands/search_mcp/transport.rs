use super::error::SearchMcpError;
use crate::commands::persistence::config_directory;
use nan_harness_runtime::search_docker::DockerSearchPaths;
use nan_harness_runtime::{
    SearchError, SearchInterest, SearchLease, SearchRequest, SearchResult, SearchSupervisor,
    SearxngClient, SearxngMode, load_search_config,
};
use reqwest::Url;
use std::net::IpAddr;
use std::path::PathBuf;
use tokio::sync::Mutex;

pub(super) struct SearchTransport {
    client: Option<SearxngClient>,
    local_supervisor: Option<SearchSupervisor>,
    local_lease: Mutex<Option<SearchLease>>,
    docker_interest_directory: Option<PathBuf>,
    docker_interest: Mutex<Option<SearchInterest>>,
}

impl SearchTransport {
    /// Builds a `SearXNG` client from the persisted search configuration.
    ///
    /// The endpoint and token arguments are retained for compatibility with existing
    /// managed MCP documents. They are intentionally ignored: managed search must not
    /// send a NaN credential to the `SearXNG` endpoint.
    pub(super) fn new(
        _endpoint: Option<Url>,
        _token_environment: Option<String>,
    ) -> Result<Self, SearchMcpError> {
        let config = search_config_path()
            .map(load_search_config)
            .transpose()
            .map_err(SearchMcpError::LoadConfig)?
            .flatten();
        let client = config
            .as_ref()
            .map(|config| SearxngClient::new(config.clone()))
            .transpose()
            .map_err(|_| SearchMcpError::BuildSearchClient)?;
        let local_supervisor = config
            .as_ref()
            .filter(|config| config.mode() == SearxngMode::Local)
            .map(|config| {
                let home = home_directory();
                SearchSupervisor::from_standalone_install(config, home.as_deref())
                    .map_err(SearchMcpError::SearchLifecycle)
            })
            .transpose()?
            .flatten();
        let docker_interest_directory = if config
            .as_ref()
            .is_some_and(|config| config.mode() == SearxngMode::Docker)
        {
            home_directory()
                .map(|home| DockerSearchPaths::for_user_home(&home))
                .map(|paths| {
                    paths
                        .is_owned_root()
                        .map_err(SearchMcpError::DockerLifecycle)
                        .map(|owned| owned.then(|| paths.root().to_path_buf()))
                })
                .transpose()?
                .flatten()
        } else {
            None
        };
        Ok(Self {
            client,
            local_supervisor,
            local_lease: Mutex::new(None),
            docker_interest_directory,
            docker_interest: Mutex::new(None),
        })
    }

    pub(super) async fn search(
        &self,
        request: SearchRequest,
    ) -> Result<Vec<SearchResult>, &'static str> {
        let client = self
            .client
            .as_ref()
            .ok_or("NaN web search is not configured; run `nanh search setup`")?;
        if let Some(supervisor) = &self.local_supervisor {
            let mut lease = self.local_lease.lock().await;
            if lease.is_none() {
                *lease = supervisor
                    .acquire()
                    .await
                    .map_err(|_| "NH-SEARCH-MCP-006")?;
            }
        }
        if let Some(directory) = &self.docker_interest_directory {
            let mut interest = self.docker_interest.lock().await;
            if interest.is_none() {
                *interest =
                    Some(SearchInterest::acquire(directory).map_err(|_| "NH-SEARCH-MCP-006")?);
            }
        }
        client.search(request).await.map_err(map_search_error)
    }
}

fn search_config_path() -> Option<PathBuf> {
    config_directory().map(|directory| directory.join("search.json"))
}

fn home_directory() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("USERPROFILE").map(PathBuf::from)
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("HOME").map(PathBuf::from)
    }
}

fn map_search_error(error: SearchError) -> &'static str {
    match error {
        SearchError::InvalidQuery
        | SearchError::QueryTooLarge
        | SearchError::InvalidDomainFilter => "NH-SEARCH-MCP-005",
        SearchError::Timeout | SearchError::Transport => "NH-SEARCH-MCP-006",
        SearchError::HttpStatus(_) => "NH-SEARCH-MCP-007",
        SearchError::ResponseTooLarge => "NH-SEARCH-MCP-008",
        SearchError::InvalidResponse => "NH-SEARCH-MCP-009",
        SearchError::ClientBuild => "NH-SEARCH-MCP-003",
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
