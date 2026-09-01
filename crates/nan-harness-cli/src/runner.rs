use crate::app::{Cli, Command, HarnessRunArgs};
use crate::commands;
use crate::commands::install::{
    InstallDecision, check_required_runtime, executable_from_known_locations, install_spec,
    offer_install,
};
use crate::commands::persistence::{LastSelection, PersistenceManager};
use crate::error::CliError;
use crate::usage_evidence;
use crate::usage_summary;
use nan_harness_adapters::{
    AiderAdapter, ClaudeCodeAdapter, ClineAdapter, CodexAdapter, DeepSeekHarnessAdapter, FxAdapter,
    GooseAdapter, HermesAdapter, KimiCodeAdapter, OmpAdapter, OpenClawAdapter, OpenCodeAdapter,
    PiAdapter, PrimeAgentAdapter, QwenCodeAdapter,
};
use nan_harness_core::launch_plan::{LaunchId, ObservabilityFormat};
use nan_harness_core::model::{
    ModelAvailability, ProfileSource, QualificationStatus, ReasoningEffort, ReasoningSelection,
};
use nan_harness_core::{
    CodingModelProfile, DetectedHarness, HarnessAdapter, HarnessKind, LaunchPlan, PlanContext,
    PlanError, ResolvedModel, WebSearchPolicy, build_validated_plan, coding_model_profile,
    is_valid_provider_model_id, known_coding_model,
};
use nan_harness_runtime::BridgeDiagnostic;
use nan_harness_runtime::{
    CancellationToken, DiscoveryError, DiscoveryOptions, DiscoveryReport, ExecutionOutcome,
    LaunchSession, RuntimeError, SignalKind, Supervisor, discover_harness,
};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

const DEFAULT_MODEL_ID: &str = "qwen3.6";

mod arguments;
mod discovery;
mod dispatch;
mod harness;
mod models;
mod personality;
mod resolution;
mod signals;
#[cfg(test)]
mod tests;

pub(crate) use arguments::{direct_chat_gateway_disabled, harness_run_arguments};
pub(crate) use discovery::discover_or_install_harness;
pub(crate) use harness::web_search_policy;
pub(crate) use resolution::near_model_match;
pub(crate) use signals::install_signal_handlers;

#[derive(Debug)]
pub(crate) struct RunError {
    error: CliError,
    harness: Option<DetectedHarness>,
}

impl RunError {
    fn after_discovery(error: CliError, harness: DetectedHarness) -> Self {
        Self {
            error,
            harness: Some(harness),
        }
    }

    pub(crate) const fn error(&self) -> &CliError {
        &self.error
    }

    pub(crate) const fn harness(&self) -> Option<&DetectedHarness> {
        self.harness.as_ref()
    }
}

impl From<CliError> for RunError {
    fn from(error: CliError) -> Self {
        Self {
            error,
            harness: None,
        }
    }
}

pub(crate) async fn run(
    cli: &Cli,
    interactive: bool,
    bridge_diagnostics: &mut Vec<BridgeDiagnostic>,
) -> Result<i32, RunError> {
    dispatch::dispatch(cli, interactive, bridge_diagnostics).await
}
