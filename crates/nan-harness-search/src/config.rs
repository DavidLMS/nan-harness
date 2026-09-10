use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use std::str::FromStr;
use thiserror::Error;
use url::Url;

/// Deployment mode for the `SearXNG` endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SearxngMode {
    /// A separately managed endpoint. Remote endpoints must use HTTPS.
    Remote,
    /// A `SearXNG` process bound to this machine.
    Local,
    /// A `SearXNG` process reached through a container or container network.
    Docker,
}

/// Validated `SearXNG` endpoint configuration.
///
/// The configuration deliberately contains no credential field. `SearXNG` instances
/// commonly use an unauthenticated local endpoint, and any future authentication
/// mechanism must use a secret reference rather than putting a token in this URL.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SearxngConfig {
    mode: SearxngMode,
    base_url: Url,
}

impl SearxngConfig {
    /// Validates a URL for the selected deployment mode.
    ///
    /// # Errors
    ///
    /// Returns [`SearchConfigError`] for unsupported schemes, URL credentials,
    /// query/fragment components, or a URL that does not match the mode.
    pub fn new(mode: SearxngMode, value: &str) -> Result<Self, SearchConfigError> {
        if value.chars().any(char::is_whitespace) {
            return Err(SearchConfigError::Whitespace);
        }
        let mut base_url = Url::parse(value).map_err(|_| SearchConfigError::InvalidUrl)?;
        if !matches!(base_url.scheme(), "http" | "https")
            || base_url.host_str().is_none()
            || !base_url.username().is_empty()
            || base_url.password().is_some()
            || base_url.query().is_some()
            || base_url.fragment().is_some()
        {
            return Err(SearchConfigError::InvalidUrl);
        }

        match mode {
            SearxngMode::Remote if base_url.scheme() != "https" => {
                return Err(SearchConfigError::RemoteRequiresHttps);
            }
            SearxngMode::Local if !is_loopback(base_url.host_str().unwrap_or_default()) => {
                return Err(SearchConfigError::LocalRequiresLoopback);
            }
            SearxngMode::Remote | SearxngMode::Local | SearxngMode::Docker => {}
        }

        // A base path is supported for reverse-proxy deployments. Keeping the
        // trailing slash makes joining `/search` deterministic without exposing
        // credentials or other URL components.
        if !base_url.path().ends_with('/') {
            base_url.set_path(&format!("{}/", base_url.path()));
        }
        Ok(Self { mode, base_url })
    }

    /// # Errors
    ///
    /// Returns [`SearchConfigError`] when `value` is not a valid remote endpoint.
    pub fn remote(value: &str) -> Result<Self, SearchConfigError> {
        Self::new(SearxngMode::Remote, value)
    }

    /// # Errors
    ///
    /// Returns [`SearchConfigError`] when `value` is not a valid local endpoint.
    pub fn local(value: &str) -> Result<Self, SearchConfigError> {
        Self::new(SearxngMode::Local, value)
    }

    /// # Errors
    ///
    /// Returns [`SearchConfigError`] when `value` is not a valid container endpoint.
    pub fn docker(value: &str) -> Result<Self, SearchConfigError> {
        Self::new(SearxngMode::Docker, value)
    }

    #[must_use]
    pub const fn mode(&self) -> SearxngMode {
        self.mode
    }

    #[must_use]
    pub fn base_url(&self) -> &Url {
        &self.base_url
    }

    /// # Panics
    ///
    /// This method does not panic; the endpoint was validated by [`Self::new`].
    #[must_use]
    pub fn search_url(&self) -> Url {
        let mut search_url = self.base_url.clone();
        let path = format!("{}search", self.base_url.path());
        search_url.set_path(&path);
        search_url
    }

    /// Returns the canonical URL without exposing a mutable URL handle.
    #[must_use]
    pub fn base_url_string(&self) -> String {
        self.base_url.as_str().to_owned()
    }
}

fn is_loopback(host: &str) -> bool {
    let host = host.trim_matches(['[', ']']);
    host.eq_ignore_ascii_case("localhost")
        || IpAddr::from_str(host).is_ok_and(|address| address.is_loopback())
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum SearchConfigError {
    #[error("SearXNG URL must not contain whitespace")]
    Whitespace,
    #[error("SearXNG URL must be an HTTP(S) URL without credentials, query, or fragment")]
    InvalidUrl,
    #[error("remote SearXNG endpoints must use HTTPS")]
    RemoteRequiresHttps,
    #[error("local SearXNG endpoints must target localhost or a loopback address")]
    LocalRequiresLoopback,
}

#[cfg(test)]
mod tests {
    use super::{SearchConfigError, SearxngConfig, SearxngMode};

    #[test]
    fn accepts_remote_https_and_canonicalizes_the_base_path() {
        let config = SearxngConfig::remote("https://search.example.test/searxng")
            .expect("remote URL should validate");
        assert_eq!(config.mode(), SearxngMode::Remote);
        assert_eq!(
            config.base_url().as_str(),
            "https://search.example.test/searxng/"
        );
        assert_eq!(
            config.search_url().as_str(),
            "https://search.example.test/searxng/search"
        );
    }

    #[test]
    fn accepts_local_and_docker_endpoints() {
        assert!(SearxngConfig::local("http://127.0.0.1:8080").is_ok());
        assert!(SearxngConfig::local("http://[::1]:8080").is_ok());
        assert!(SearxngConfig::docker("http://searxng:8080").is_ok());
    }

    #[test]
    fn rejects_unsafe_or_mismatched_urls() {
        for url in [
            "http://search.example.test",
            "https://search.example.test/?token=secret",
            "https://user:secret@search.example.test",
            "ftp://search.example.test",
            "https://search.example.test/#fragment",
            "https://search.example.test/with whitespace",
        ] {
            assert!(matches!(
                SearxngConfig::remote(url),
                Err(SearchConfigError::Whitespace
                    | SearchConfigError::InvalidUrl
                    | SearchConfigError::RemoteRequiresHttps,)
            ));
        }
        assert_eq!(
            SearxngConfig::local("http://search.example.test").unwrap_err(),
            SearchConfigError::LocalRequiresLoopback
        );
    }
}
