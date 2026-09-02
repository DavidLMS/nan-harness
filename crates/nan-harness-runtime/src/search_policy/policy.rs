use super::candidates::candidate_paths;
use super::environment::{detect_environment, home_directory};
use super::errors::SearchPolicyError;
use super::inspection::detect;
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
