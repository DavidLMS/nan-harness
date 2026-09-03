use super::arguments::direct_chat_gateway_notice;
use super::models::*;
use super::resolution::{explicit_model_warning, offline_requested_model, resolve_explicit_model};
use super::*;
use crate::app::Cli;
use crate::commands::persistence::LastSelection;
use nan_harness_core::{
    HarnessKind, KNOWN_CODING_MODELS, ModelAvailability, ProfileSource, QualificationStatus,
    ReasoningEffort, ReasoningSelection, coding_model_profile,
};
use nan_harness_runtime::{
    BridgeError, ExecutionOutcome, ExecutionReport, RuntimeError, SignalKind,
};

fn execution_report(
    outcome: ExecutionOutcome,
    model: Option<&str>,
    reasoning: Option<ReasoningSelection>,
) -> ExecutionReport {
    ExecutionReport {
        outcome,
        exit_code: i32::from(outcome != ExecutionOutcome::Succeeded),
        temporary_root: None,
        selected_model: model.map(str::to_owned),
        selected_reasoning: reasoning,
        bridge_diagnostics: Vec::new(),
        provider_usage: None,
    }
}

mod launch;
mod models;
