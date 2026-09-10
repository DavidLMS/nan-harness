use nan_harness_core::WebSearchPolicy;
pub use nan_harness_search::{
    SearchAvailability, SearchConfigError, SearchConfigStore, SearchConfigStoreError, SearchError,
    SearchRequest, SearchResult, SearxngClient, SearxngConfig, SearxngMode,
};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchConfiguration {
    None,
    ManagedNan,
    External,
    Unsupported,
}

/// Provider choice used by runtime launch preparation.
///
/// This is separate from [`SearchConfiguration`], which reports whether a
/// harness already owns a search integration. The runtime can carry this
/// provider-neutral choice until a later bridge or supervisor stage resolves
/// the concrete process endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum SearchBackend {
    Disabled,
    #[default]
    Unconfigured,
    Searxng(SearxngConfig),
}

/// Runtime search settings that can be passed across policy and launch layers.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SearchRuntimeConfig {
    pub backend: SearchBackend,
}

impl SearchRuntimeConfig {
    #[must_use]
    pub const fn disabled() -> Self {
        Self {
            backend: SearchBackend::Disabled,
        }
    }

    #[must_use]
    pub fn searxng(config: SearxngConfig) -> Self {
        Self {
            backend: SearchBackend::Searxng(config),
        }
    }
}

/// Resolves the policy portion of search setup without probing a process or network.
#[must_use]
pub fn resolve_search_backend(
    policy: WebSearchPolicy,
    configured: Option<SearxngConfig>,
) -> SearchBackend {
    match policy {
        WebSearchPolicy::Disabled => SearchBackend::Disabled,
        WebSearchPolicy::Auto | WebSearchPolicy::Force => {
            configured.map_or(SearchBackend::Unconfigured, SearchBackend::Searxng)
        }
    }
}

/// Loads the safe, non-secret `SearXNG` endpoint used by later runtime stages.
///
/// # Errors
///
/// Returns [`SearchConfigStoreError`] when the persisted configuration cannot
/// be read, parsed, or validated.
pub fn load_search_config(
    path: impl AsRef<Path>,
) -> Result<Option<SearxngConfig>, SearchConfigStoreError> {
    SearchConfigStore::new(path.as_ref()).load()
}

/// Atomically persists the safe, non-secret `SearXNG` endpoint.
///
/// # Errors
///
/// Returns [`SearchConfigStoreError`] when the configuration directory or
/// replacement file cannot be protected or written.
pub fn save_search_config(
    path: impl AsRef<Path>,
    config: &SearxngConfig,
) -> Result<(), SearchConfigStoreError> {
    SearchConfigStore::new(path.as_ref()).save(config)
}

#[cfg(test)]
mod tests {
    use super::{
        SearchBackend, SearchRuntimeConfig, load_search_config, resolve_search_backend,
        save_search_config,
    };
    use nan_harness_core::WebSearchPolicy;
    use nan_harness_search::SearxngConfig;

    #[test]
    fn policy_resolution_keeps_disabled_and_unconfigured_states_explicit() {
        let config = SearxngConfig::local("http://127.0.0.1:8080").expect("URL should validate");
        assert_eq!(
            resolve_search_backend(WebSearchPolicy::Disabled, Some(config.clone())),
            SearchBackend::Disabled
        );
        assert_eq!(
            resolve_search_backend(WebSearchPolicy::Auto, None),
            SearchBackend::Unconfigured
        );
        assert_eq!(
            resolve_search_backend(WebSearchPolicy::Force, Some(config.clone())),
            SearchBackend::Searxng(config)
        );
        assert_eq!(
            SearchRuntimeConfig::searxng(
                SearxngConfig::local("http://127.0.0.1:8080").expect("URL should validate")
            )
            .backend,
            SearchBackend::Searxng(
                SearxngConfig::local("http://127.0.0.1:8080").expect("URL should validate")
            )
        );
    }

    #[test]
    fn persistence_api_round_trips_a_non_secret_endpoint() {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let path = directory.path().join("search.json");
        let config = SearxngConfig::local("http://127.0.0.1:8080").expect("URL should validate");
        save_search_config(&path, &config).expect("config should save");
        assert_eq!(
            load_search_config(&path)
                .expect("config should load")
                .expect("config should exist"),
            config
        );
    }
}
