use super::candidates::candidate_paths;
use super::configuration::{SearchBackend, SearchRuntimeConfig, resolve_search_backend};
use super::environment::{detect_environment, home_directory};
use super::errors::SearchPolicyError;
use super::inspection::detect;
use super::load_persisted_search_config;
use super::signal::DetectionSignal;
use nan_harness_core::launch_plan::Transport;
use nan_harness_core::{HarnessKind, LaunchPlan, WebSearchPolicy};
#[cfg(test)]
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SearchResolution {
    Nan,
    Existing,
    Disabled,
    Unsupported,
}

impl SearchResolution {
    pub(crate) const fn uses_nan(self) -> bool {
        matches!(self, Self::Nan)
    }
}

pub(super) const fn supports_nan_search(harness: HarnessKind) -> bool {
    !matches!(harness, HarnessKind::Aider)
}

pub(crate) fn resolve(
    plan: &LaunchPlan,
    direct_chat_gateway: bool,
) -> Result<SearchResolution, SearchPolicyError> {
    if plan.web_search_policy == WebSearchPolicy::Disabled {
        return Ok(SearchResolution::Disabled);
    }
    if !supports_nan_search(plan.harness.kind) {
        return if plan.web_search_policy == WebSearchPolicy::Force {
            Err(SearchPolicyError::UnsupportedHarness(plan.harness.kind))
        } else {
            Ok(SearchResolution::Unsupported)
        };
    }
    if plan.harness.kind == HarnessKind::Omp
        && matches!(&plan.transport, Transport::DirectChat { .. })
        && direct_chat_gateway
    {
        return Ok(SearchResolution::Nan);
    }
    let home = home_directory().ok_or(SearchPolicyError::MissingHomeDirectory)?;
    let candidates = candidate_paths(plan, &home);
    let signal = detect_environment(plan.harness.kind, &home)?.combine(detect(&candidates)?);
    if matches!(&plan.transport, Transport::DirectChat { .. }) && !direct_chat_gateway {
        return match (plan.web_search_policy, signal) {
            (_, DetectionSignal::Collision(path)) => Err(SearchPolicyError::McpNameCollision(path)),
            (_, DetectionSignal::ManagedNan)
            | (WebSearchPolicy::Auto, DetectionSignal::External) => Ok(SearchResolution::Existing),
            (WebSearchPolicy::Auto, DetectionSignal::None) => Ok(SearchResolution::Unsupported),
            (WebSearchPolicy::Force, DetectionSignal::External | DetectionSignal::None) => {
                Err(SearchPolicyError::RequiresDirectGateway)
            }
            (WebSearchPolicy::Disabled, _) => unreachable!("disabled returns before detection"),
        };
    }
    resolve_signal(plan.web_search_policy, signal)
}

/// Resolves the launch search backend and loads the persisted SearXNG endpoint
/// only when policy selected NaN search for this launch.
pub(crate) fn resolve_runtime_config(
    plan: &LaunchPlan,
    direct_chat_gateway: bool,
) -> Result<SearchRuntimeConfig, SearchPolicyError> {
    let resolution = resolve(plan, direct_chat_gateway)?;
    if !resolution.uses_nan() {
        return Ok(SearchRuntimeConfig::disabled());
    }
    let configured = load_persisted_search_config()?;
    Ok(SearchRuntimeConfig {
        backend: resolve_search_backend(plan.web_search_policy, configured),
    })
}

pub(crate) fn bridge_search_values(
    configuration: &SearchRuntimeConfig,
) -> (bool, Option<nan_harness_search::SearxngConfig>) {
    match &configuration.backend {
        SearchBackend::Disabled => (false, None),
        SearchBackend::Unconfigured => (true, None),
        SearchBackend::Searxng(config) => (true, Some(config.clone())),
    }
}

#[cfg(test)]
mod runtime_tests {
    use super::bridge_search_values;
    use crate::search_policy::{SearchBackend, SearchRuntimeConfig};
    use nan_harness_search::SearxngConfig;

    #[test]
    fn bridge_values_keep_disabled_unconfigured_and_configured_states_distinct() {
        let endpoint = SearxngConfig::local("http://127.0.0.1:8080").expect("valid endpoint");
        assert_eq!(
            bridge_search_values(&SearchRuntimeConfig::disabled()),
            (false, None)
        );
        assert_eq!(
            bridge_search_values(&SearchRuntimeConfig::default()),
            (true, None)
        );
        assert_eq!(
            bridge_search_values(&SearchRuntimeConfig {
                backend: SearchBackend::Searxng(endpoint.clone()),
            }),
            (true, Some(endpoint))
        );
    }
}

#[cfg(test)]
pub(super) fn resolve_from_candidates(
    policy: WebSearchPolicy,
    candidates: &[PathBuf],
) -> Result<SearchResolution, SearchPolicyError> {
    resolve_signal(policy, detect(candidates)?)
}

fn resolve_signal(
    policy: WebSearchPolicy,
    signal: DetectionSignal,
) -> Result<SearchResolution, SearchPolicyError> {
    if let DetectionSignal::Collision(path) = signal {
        return Err(SearchPolicyError::McpNameCollision(path));
    }
    if policy == WebSearchPolicy::Force {
        return Ok(if signal == DetectionSignal::ManagedNan {
            SearchResolution::Existing
        } else {
            SearchResolution::Nan
        });
    }
    Ok(match signal {
        DetectionSignal::External | DetectionSignal::ManagedNan => SearchResolution::Existing,
        DetectionSignal::None => SearchResolution::Nan,
        DetectionSignal::Collision(_) => unreachable!("collision is returned above"),
    })
}
