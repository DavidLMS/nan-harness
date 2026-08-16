use nan_harness_core::{SecretError, SecretRef, SecretStore, SecretValue};
use std::env;
use thiserror::Error;

pub const DEFAULT_PROVIDER_BASE_URL: &str = "https://api.nan.builders/v1";
pub const NAN_API_KEY_REFERENCE: &str = "nan_api_key";

pub trait EnvironmentSource {
    fn value(&self, name: &str) -> Option<String>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ProcessEnvironment;

impl EnvironmentSource for ProcessEnvironment {
    fn value(&self, name: &str) -> Option<String> {
        env::var(name).ok()
    }
}

#[derive(Default)]
pub struct ConfigOverrides {
    pub provider_base_url: Option<String>,
    pub nan_api_key: Option<SecretValue>,
}

#[derive(Debug)]
pub struct ResolvedConfig {
    pub provider_base_url: String,
    pub provider_credential_ref: SecretRef,
    pub secrets: SecretStore,
}

pub struct ConfigResolver;

impl ConfigResolver {
    /// Resolves runtime configuration using overrides, environment, then defaults.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] when the provider URL or credential is invalid.
    pub fn resolve(
        environment: &impl EnvironmentSource,
        overrides: ConfigOverrides,
    ) -> Result<ResolvedConfig, ConfigError> {
        let provider_base_url = overrides
            .provider_base_url
            .or_else(|| environment.value("NAN_BASE_URL"))
            .unwrap_or_else(|| DEFAULT_PROVIDER_BASE_URL.to_owned());
        validate_provider_base_url(&provider_base_url)?;

        let credential = match overrides.nan_api_key {
            Some(value) => value,
            None => environment
                .value("NAN_API_KEY")
                .ok_or(ConfigError::MissingApiKey)
                .and_then(|value| SecretValue::new(value).map_err(ConfigError::Secret))?,
        };
        let provider_credential_ref =
            SecretRef::new(NAN_API_KEY_REFERENCE).map_err(ConfigError::Secret)?;
        let mut secrets = SecretStore::new();
        secrets.insert(provider_credential_ref.clone(), credential);

        Ok(ResolvedConfig {
            provider_base_url,
            provider_credential_ref,
            secrets,
        })
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error("NAN_API_KEY is required; pass an explicit credential or set the environment variable")]
    MissingApiKey,
    #[error("provider base URL must be an absolute HTTP or HTTPS URL")]
    InvalidProviderBaseUrl,
    #[error(transparent)]
    Secret(#[from] SecretError),
}

impl ConfigError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::MissingApiKey => "NH-CONFIG-001",
            Self::InvalidProviderBaseUrl => "NH-CONFIG-002",
            Self::Secret(_) => "NH-CONFIG-003",
        }
    }
}

fn validate_provider_base_url(value: &str) -> Result<(), ConfigError> {
    if (value.starts_with("http://") || value.starts_with("https://"))
        && value
            .split_once("://")
            .is_some_and(|(_, authority)| !authority.is_empty())
        && !value.chars().any(char::is_whitespace)
    {
        Ok(())
    } else {
        Err(ConfigError::InvalidProviderBaseUrl)
    }
}
