use super::lifecycle::BridgeExecution;
use crate::prepared::PreparedLaunch;
use crate::signals::SignalKind;
use nan_harness_bridge::{BridgeDiagnostic, ProviderUsageSnapshot};
use nan_harness_core::launch_plan::{CODEX_HOME_OVERLAY_ID, CODEX_PROFILE_ARTIFACT_ID};
use nan_harness_core::{
    CodingModelProfile, LaunchPlan, ReasoningHint, ReasoningPolicy, ReasoningSelection,
};
use std::path::PathBuf;
use std::process::ExitStatus;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionOutcome {
    Succeeded,
    Failed,
    Cancelled(SignalKind),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionReport {
    pub outcome: ExecutionOutcome,
    pub exit_code: i32,
    pub temporary_root: Option<PathBuf>,
    pub selected_model: Option<String>,
    pub selected_reasoning: Option<ReasoningSelection>,
    pub bridge_diagnostics: Vec<BridgeDiagnostic>,
    pub provider_usage: Option<ProviderUsageSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CodexSelection {
    model: String,
    reasoning: Option<ReasoningSelection>,
}

#[derive(Clone, Copy)]
pub(super) enum Completion {
    Exited(ExitStatus),
    Cancelled(SignalKind),
}

pub(super) fn report(
    plan: &LaunchPlan,
    completion: Completion,
    temporary_root: Option<PathBuf>,
    selected: Option<CodexSelection>,
    bridge_diagnostics: Vec<BridgeDiagnostic>,
    provider_usage: Option<ProviderUsageSnapshot>,
) -> ExecutionReport {
    let (outcome, exit_code) = match completion {
        Completion::Exited(status) if status.success() => (ExecutionOutcome::Succeeded, 0),
        Completion::Exited(status) => {
            let exit_code = if plan.process.preserve_exit_code {
                exit_code_from_status(status)
            } else {
                1
            };
            (ExecutionOutcome::Failed, exit_code)
        }
        Completion::Cancelled(signal) => (ExecutionOutcome::Cancelled(signal), signal.exit_code()),
    };
    ExecutionReport {
        outcome,
        exit_code,
        temporary_root,
        selected_model: selected.as_ref().map(|selection| selection.model.clone()),
        selected_reasoning: selected.and_then(|selection| selection.reasoning),
        bridge_diagnostics,
        provider_usage,
    }
}

pub(super) fn bridged_report(
    plan: &LaunchPlan,
    execution: BridgeExecution,
    temporary_root: Option<PathBuf>,
    selected: Option<CodexSelection>,
) -> ExecutionReport {
    report(
        plan,
        execution.completion,
        temporary_root,
        selected,
        execution.diagnostics,
        Some(execution.provider_usage),
    )
}

pub(super) fn prepared_codex_selection(
    prepared: &PreparedLaunch,
    models: &[CodingModelProfile],
) -> Option<CodexSelection> {
    let path = prepared
        .artifact_path(CODEX_PROFILE_ARTIFACT_ID)
        .or_else(|| {
            prepared
                .artifact_path(CODEX_HOME_OVERLAY_ID)
                .map(|path| path.join("config.toml"))
        })?;
    let content = std::fs::read_to_string(path).ok()?;
    let config = toml::from_str::<toml::Table>(&content).ok()?;
    let selected = config
        .get("model")
        .and_then(toml::Value::as_str)
        .filter(|model| !model.is_empty())
        .and_then(|selected| models.iter().find(|model| model.id == selected))?;
    let reasoning = config
        .get("model_reasoning_effort")
        .and_then(toml::Value::as_str)
        .and_then(|value| parse_codex_reasoning(value, selected.reasoning));
    Some(CodexSelection {
        model: selected.id.clone(),
        reasoning,
    })
}

pub(super) fn parse_codex_reasoning(
    value: &str,
    policy: ReasoningPolicy,
) -> Option<ReasoningSelection> {
    let hint = match value {
        "none" => ReasoningHint::Disabled,
        "low" => ReasoningHint::Low,
        "medium" => ReasoningHint::Medium,
        "high" => ReasoningHint::High,
        "xhigh" => ReasoningHint::ExtraHigh,
        _ => return None,
    };
    policy.resolve_hint(hint)
}

fn exit_code_from_status(status: ExitStatus) -> i32 {
    status.code().unwrap_or(1)
}
