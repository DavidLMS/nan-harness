use nan_harness_core::WebSearchPolicy;
pub use nan_harness_search::{
    SearchAvailability, SearchConfigError, SearchConfigStore, SearchConfigStoreError, SearchError,
    SearchRequest, SearchResult, SearxngClient, SearxngConfig, SearxngMode,
};
use std::env;
use std::path::{Path, PathBuf};

const SEARCH_CONFIG_FILE_NAME: &str = "search.json";
const CONFIG_DIRECTORY_ENVIRONMENT_VARIABLE: &str = "NAN_HARNESS_CONFIG_DIR";

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

/// Loads the persisted search endpoint using the runtime's platform config directory.
///
/// A missing config directory or file means that search is unconfigured. The
/// caller decides whether the configured endpoint is relevant to the launch's
/// resolved search policy.
pub(crate) fn load_persisted_search_config() -> Result<Option<SearxngConfig>, SearchConfigStoreError>
{
    let Some(path) = search_config_path() else {
        return Ok(None);
    };
    load_search_config(path)
}

fn search_config_path() -> Option<PathBuf> {
    config_directory().map(|directory| directory.join(SEARCH_CONFIG_FILE_NAME))
}

fn config_directory() -> Option<PathBuf> {
    if let Some(directory) = env::var_os(CONFIG_DIRECTORY_ENVIRONMENT_VARIABLE) {
        return Some(PathBuf::from(directory));
    }
    #[cfg(target_os = "macos")]
    {
        home_directory().map(|home| home.join("Library/Application Support/nan-harness"))
    }
    #[cfg(target_os = "windows")]
    {
        env::var_os("APPDATA")
            .map(PathBuf::from)
            .map(|directory| directory.join("nan-harness"))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .map(|directory| directory.join("nan-harness"))
            .or_else(|| home_directory().map(|home| home.join(".config/nan-harness")))
    }
}

fn home_directory() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        env::var_os("USERPROFILE").map(PathBuf::from)
    }
    #[cfg(not(windows))]
    {
        env::var_os("HOME").map(PathBuf::from)
    }
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

    #[test]
    fn malformed_persisted_configuration_is_returned_as_a_typed_error() {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let path = directory.path().join("search.json");
        std::fs::write(&path, b"{not-json").expect("fixture should write");

        assert!(matches!(
            load_search_config(path),
            Err(super::SearchConfigStoreError::Parse(_))
        ));
    }
}
