//! Live model discovery with a credential-bound, last-known-good fallback.
mod storage;

use crate::BridgeError;
use nan_harness_core::{CodingModelProfile, SecretValue};
use std::io::Write as _;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use storage::ModelCache;

/// Safe reason for using a previously discovered catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelFallbackReason {
    Transport,
    Timeout,
    HttpStatus(u16),
    InvalidResponse,
    NoModels,
}

impl ModelFallbackReason {
    /// Classifies HTTP failures eligible for fallback, excluding authentication failures.
    #[must_use]
    pub const fn from_status(status: u16) -> Option<Self> {
        match status {
            408 | 429 | 500..=599 => Some(Self::HttpStatus(status)),
            _ => None,
        }
    }
}

/// Source of the models returned by discovery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelDiscoverySource {
    Live,
    Cache {
        fetched_at_unix_seconds: u64,
        reason: ModelFallbackReason,
    },
}

#[derive(Debug)]
pub struct ModelDiscovery {
    pub models: Vec<CodingModelProfile>,
    pub source: ModelDiscoverySource,
}

impl ModelDiscovery {
    /// Returns the models and emits one safe stderr notice when using cached data.
    #[must_use]
    pub fn into_models_with_notice(self) -> Vec<CodingModelProfile> {
        if let Some(notice) = self.notice(unix_seconds()) {
            let _ = writeln!(std::io::stderr().lock(), "{notice}");
        }
        self.models
    }

    fn notice(&self, now: u64) -> Option<String> {
        let ModelDiscoverySource::Cache {
            fetched_at_unix_seconds,
            reason,
        } = self.source
        else {
            return None;
        };
        let cause = match reason {
            ModelFallbackReason::Transport => "connection failed".to_owned(),
            ModelFallbackReason::Timeout => "request timed out".to_owned(),
            ModelFallbackReason::HttpStatus(status) => format!("HTTP {status}"),
            ModelFallbackReason::InvalidResponse => "invalid response".to_owned(),
            ModelFallbackReason::NoModels => "no usable models returned".to_owned(),
        };
        let age = now.saturating_sub(fetched_at_unix_seconds);
        Some(format!(
            "Warning: NaN model discovery failed ({cause}). Using cached models from {age} seconds ago. Availability may have changed."
        ))
    }
}

/// Saves successful discovery or recovers a catalog after an eligible failure.
///
/// The caller must perform live discovery first. Cache I/O is best effort and never
/// changes the live error when fallback is unavailable. Empty successes are not saved;
/// callers must classify them as their existing no-models error before calling this.
///
/// # Errors
/// Returns the original discovery error when it is ineligible or no valid cache exists.
pub fn resolve_model_discovery<E>(
    provider_base_url: &str,
    api_key: &str,
    result: Result<Vec<CodingModelProfile>, E>,
    classify: impl FnOnce(&E) -> Option<ModelFallbackReason>,
) -> Result<ModelDiscovery, E> {
    let cache = ModelCache::default_directory()
        .and_then(|directory| ModelCache::open(&directory, provider_base_url, api_key).ok());
    resolve(cache.as_ref(), result, classify)
}

fn resolve<E>(
    cache: Option<&ModelCache>,
    result: Result<Vec<CodingModelProfile>, E>,
    classify: impl FnOnce(&E) -> Option<ModelFallbackReason>,
) -> Result<ModelDiscovery, E> {
    match result {
        Ok(models) => {
            if let Some(cache) = cache {
                let _ = cache.save(&models, unix_seconds());
            }
            Ok(ModelDiscovery {
                models,
                source: ModelDiscoverySource::Live,
            })
        }
        Err(error) => {
            if let Some(reason) = classify(&error)
                && let Some((models, fetched_at_unix_seconds)) = cache.and_then(ModelCache::load)
            {
                return Ok(ModelDiscovery {
                    models,
                    source: ModelDiscoverySource::Cache {
                        fetched_at_unix_seconds,
                        reason,
                    },
                });
            }
            Err(error)
        }
    }
}

pub(crate) async fn discover_coding_models(
    provider_base_url: &str,
    api_key: Arc<SecretValue>,
) -> Result<Vec<CodingModelProfile>, BridgeError> {
    let result =
        nan_harness_bridge::discover_coding_models(provider_base_url, Arc::clone(&api_key))
            .await
            .and_then(|models| {
                if models.is_empty() {
                    Err(BridgeError::NoCompatibleModels)
                } else {
                    Ok(models)
                }
            });
    api_key
        .with_secret(|secret| {
            resolve_model_discovery(provider_base_url, secret, result, bridge_fallback_reason)
        })
        .map(ModelDiscovery::into_models_with_notice)
}

fn bridge_fallback_reason(error: &BridgeError) -> Option<ModelFallbackReason> {
    match error {
        BridgeError::ModelDiscoveryTransport(error) if error.is_timeout() => {
            Some(ModelFallbackReason::Timeout)
        }
        BridgeError::ModelDiscoveryTransport(_) => Some(ModelFallbackReason::Transport),
        BridgeError::ModelDiscoveryStatus { status, .. } => {
            ModelFallbackReason::from_status(status.as_u16())
        }
        BridgeError::ModelDiscoveryTooLarge | BridgeError::InvalidModelDiscoveryResponse(_) => {
            Some(ModelFallbackReason::InvalidResponse)
        }
        BridgeError::NoCompatibleModels => Some(ModelFallbackReason::NoModels),
        _ => None,
    }
}

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests;
