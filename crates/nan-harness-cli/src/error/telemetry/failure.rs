use super::super::CliError;
use nan_harness_runtime::RuntimeError;
use nan_harness_telemetry::event::{FailureCategory, FailureStage};

pub(super) const fn classify(error: &CliError) -> (FailureCategory, FailureStage, bool) {
    match error {
        CliError::Discovery(_) => (
            FailureCategory::Discovery,
            FailureStage::HarnessDetection,
            false,
        ),
        CliError::Install(_) => (
            FailureCategory::Discovery,
            FailureStage::HarnessDetection,
            true,
        ),
        CliError::Credential(_) => (
            FailureCategory::Configuration,
            FailureStage::CredentialResolution,
            false,
        ),
        CliError::Configuration(_) | CliError::TelemetrySettings(_) => {
            (FailureCategory::Configuration, FailureStage::Startup, false)
        }
        CliError::ChatGptDesktop(_)
        | CliError::ClaudeDesktop(_)
        | CliError::HermesDesktop(_)
        | CliError::PenDesktop(_)
        | CliError::ZedDesktop(_) => (
            FailureCategory::Configuration,
            FailureStage::HarnessExecution,
            false,
        ),
        CliError::Runtime(error) => classify_runtime(error),
        CliError::InvalidPlan(_) => (
            FailureCategory::Planning,
            FailureStage::LaunchValidation,
            false,
        ),
        CliError::SerializePlan(_) => (
            FailureCategory::Internal,
            FailureStage::LaunchValidation,
            false,
        ),
        CliError::CurrentDirectory(_)
        | CliError::Random(_)
        | CliError::CredentialInvariant
        | CliError::PreflightTaskFailed(_) => {
            (FailureCategory::Internal, FailureStage::Startup, false)
        }
        CliError::Update(_) => (FailureCategory::Internal, FailureStage::Startup, true),
        CliError::Persistence(_) => (FailureCategory::Configuration, FailureStage::Startup, false),
        CliError::Uninstall(_) => (
            FailureCategory::Configuration,
            FailureStage::Shutdown,
            false,
        ),
        CliError::UsageEvidence(_) => (FailureCategory::Internal, FailureStage::Shutdown, false),
    }
}

const fn classify_runtime(error: &RuntimeError) -> (FailureCategory, FailureStage, bool) {
    match error {
        RuntimeError::InvalidPlan(_) => (
            FailureCategory::Planning,
            FailureStage::LaunchValidation,
            false,
        ),
        RuntimeError::BindBridge(_) => {
            (FailureCategory::Bridge, FailureStage::BridgeStartup, false)
        }
        RuntimeError::Bridge(_) | RuntimeError::BridgeExited => {
            (FailureCategory::Bridge, FailureStage::BridgeStartup, true)
        }
        RuntimeError::Prepared(_) | RuntimeError::Process(_) => (
            FailureCategory::Process,
            FailureStage::HarnessExecution,
            false,
        ),
        RuntimeError::Secret(_) | RuntimeError::Random(_) => {
            (FailureCategory::Internal, FailureStage::Startup, false)
        }
        RuntimeError::WaitForProcess(_)
        | RuntimeError::TerminateProcess(_)
        | RuntimeError::MissingProcessId => {
            (FailureCategory::Process, FailureStage::Shutdown, true)
        }
        RuntimeError::SearchPolicy(_) => (
            FailureCategory::Configuration,
            FailureStage::LaunchValidation,
            false,
        ),
    }
}
