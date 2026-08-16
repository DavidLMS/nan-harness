use crate::process::{ProcessError, spawn_child};
use crate::signals::{CancellationToken, SignalKind};
use crate::temporary::{TemporaryError, TemporaryWorkspace};
use nan_harness_core::{LaunchPlan, LaunchPlanValidator, PlanError, SecretStore};
use std::path::PathBuf;
use std::process::{Child, ExitStatus};
use std::thread;
use std::time::Duration;
use thiserror::Error;

const POLL_INTERVAL: Duration = Duration::from_millis(10);

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
}

#[derive(Debug, Default)]
pub struct Supervisor;

impl Supervisor {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Validates and executes one direct launch plan to completion or cancellation.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError`] when validation, setup, process control, or cleanup fails.
    pub fn execute(
        &self,
        plan: &LaunchPlan,
        secrets: &SecretStore,
        cancellation: &CancellationToken,
    ) -> Result<ExecutionReport, RuntimeError> {
        LaunchPlanValidator::validate(plan).map_err(RuntimeError::InvalidPlan)?;
        if plan.transport.is_bridge() {
            return Err(RuntimeError::BridgeUnavailable);
        }

        let temporary_workspace = TemporaryWorkspace::materialize(&plan.temporary_artifacts)
            .map_err(RuntimeError::Temporary)?;
        let temporary_root = (!plan.temporary_artifacts.is_empty())
            .then(|| temporary_workspace.root().to_path_buf());
        let mut child = spawn_child(plan, secrets).map_err(RuntimeError::Process)?;
        let result = wait_for_child(&mut child, cancellation);
        drop(temporary_workspace);

        let completion = result?;
        let (outcome, exit_code) = match completion {
            Completion::Exited(status) => {
                let raw_exit_code = exit_code_from_status(status);
                if status.success() {
                    (ExecutionOutcome::Succeeded, 0)
                } else {
                    let reported = if plan.process.preserve_exit_code {
                        raw_exit_code
                    } else {
                        1
                    };
                    (ExecutionOutcome::Failed, reported)
                }
            }
            Completion::Cancelled(signal) => {
                (ExecutionOutcome::Cancelled(signal), signal.exit_code())
            }
        };

        Ok(ExecutionReport {
            outcome,
            exit_code,
            temporary_root,
        })
    }
}

enum Completion {
    Exited(ExitStatus),
    Cancelled(SignalKind),
}

fn wait_for_child(
    child: &mut Child,
    cancellation: &CancellationToken,
) -> Result<Completion, RuntimeError> {
    loop {
        if let Some(signal) = cancellation.signal() {
            child.kill().map_err(RuntimeError::TerminateProcess)?;
            child.wait().map_err(RuntimeError::WaitForProcess)?;
            return Ok(Completion::Cancelled(signal));
        }
        if let Some(status) = child.try_wait().map_err(RuntimeError::WaitForProcess)? {
            return Ok(Completion::Exited(status));
        }
        thread::sleep(POLL_INTERVAL);
    }
}

fn exit_code_from_status(status: ExitStatus) -> i32 {
    status.code().unwrap_or(1)
}

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("launch plan is invalid: {0}")]
    InvalidPlan(PlanError),
    #[error("bridge execution starts in phase 3; phase 2 only executes direct plans")]
    BridgeUnavailable,
    #[error(transparent)]
    Temporary(TemporaryError),
    #[error(transparent)]
    Process(ProcessError),
    #[error("could not wait for the harness process: {0}")]
    WaitForProcess(std::io::Error),
    #[error("could not terminate the harness process: {0}")]
    TerminateProcess(std::io::Error),
}

impl RuntimeError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::InvalidPlan(_) => "NH-RUNTIME-001",
            Self::BridgeUnavailable => "NH-RUNTIME-002",
            Self::Temporary(_) => "NH-RUNTIME-003",
            Self::Process(_) => "NH-RUNTIME-004",
            Self::WaitForProcess(_) => "NH-RUNTIME-005",
            Self::TerminateProcess(_) => "NH-RUNTIME-006",
        }
    }
}
