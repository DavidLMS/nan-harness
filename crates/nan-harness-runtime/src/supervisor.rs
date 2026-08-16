use crate::config::ResolvedConfig;
use crate::prepared::{BridgePreparation, PreparedError, PreparedLaunch};
use crate::process::{ProcessError, spawn_child};
use crate::signals::{CancellationToken, SignalKind};
use nan_harness_bridge::{BridgeConfig, BridgeError, ClaudeModelCatalog, RunningBridge};
use nan_harness_core::launch_plan::{ListenAddress, Transport};
use nan_harness_core::{LaunchPlan, LaunchPlanValidator, PlanError, SecretError, SecretValue};
use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::ExitStatus;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::net::TcpListener;
use tokio::process::Child;

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

    /// Validates, prepares, and supervises one harness launch to completion.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError`] when validation, setup, process control, or cleanup fails.
    pub async fn execute(
        &self,
        plan: &LaunchPlan,
        config: &ResolvedConfig,
        cancellation: &CancellationToken,
    ) -> Result<ExecutionReport, RuntimeError> {
        LaunchPlanValidator::validate(plan).map_err(RuntimeError::InvalidPlan)?;
        match &plan.transport {
            Transport::DirectChat { .. } => execute_direct(plan, config, cancellation).await,
            Transport::AnthropicBridge {
                listen,
                provider_credential_ref,
                session_token_ref,
                ..
            } => {
                execute_bridge(
                    plan,
                    config,
                    cancellation,
                    listen,
                    provider_credential_ref,
                    session_token_ref,
                )
                .await
            }
            Transport::ResponsesBridge { .. } => Err(RuntimeError::UnsupportedBridge),
        }
    }
}

async fn execute_direct(
    plan: &LaunchPlan,
    config: &ResolvedConfig,
    cancellation: &CancellationToken,
) -> Result<ExecutionReport, RuntimeError> {
    let prepared = PreparedLaunch::prepare(plan, None)?;
    let temporary_root = prepared.temporary_root(!plan.temporary_artifacts.is_empty());
    let mut child = spawn_child(plan, &prepared, &config.secrets)?;
    let completion = wait_for_child(&mut child, plan, cancellation).await?;
    Ok(report(plan, completion, temporary_root))
}

async fn execute_bridge(
    plan: &LaunchPlan,
    config: &ResolvedConfig,
    cancellation: &CancellationToken,
    listen: &ListenAddress,
    provider_credential_ref: &nan_harness_core::SecretRef,
    session_token_ref: &nan_harness_core::SecretRef,
) -> Result<ExecutionReport, RuntimeError> {
    let provider_api_key = copy_secret(&config.secrets, provider_credential_ref)?;
    let models = ClaudeModelCatalog::discover(
        &config.provider_base_url,
        Arc::clone(&provider_api_key),
        &plan.model.resolved_id,
    )
    .await?;
    let claude_available_models = models.gateway_ids();
    let listener = TcpListener::bind((listen.host.as_str(), listen.port))
        .await
        .map_err(RuntimeError::BindBridge)?;
    let address = listener.local_addr().map_err(RuntimeError::BindBridge)?;
    let base_url = format!("http://{address}");
    let session_token = Arc::new(generate_session_token()?);
    let prepared = PreparedLaunch::prepare(
        plan,
        Some(BridgePreparation {
            base_url,
            session_token_ref: session_token_ref.clone(),
            session_token: Arc::clone(&session_token),
            claude_available_models,
        }),
    )?;
    let temporary_root = prepared.temporary_root(!plan.temporary_artifacts.is_empty());
    let mut bridge = nan_harness_bridge::spawn(
        listener,
        BridgeConfig {
            provider_base_url: config.provider_base_url.clone(),
            models,
            provider_api_key,
            session_token,
        },
    )?;
    let mut child = match spawn_child(plan, &prepared, &config.secrets) {
        Ok(child) => child,
        Err(error) => {
            bridge.shutdown();
            bridge.wait().await?;
            return Err(RuntimeError::Process(error));
        }
    };

    let completion = supervise_pair(&mut child, &mut bridge, plan, cancellation).await?;
    Ok(report(plan, completion, temporary_root))
}

async fn supervise_pair(
    child: &mut Child,
    bridge: &mut RunningBridge,
    plan: &LaunchPlan,
    cancellation: &CancellationToken,
) -> Result<Completion, RuntimeError> {
    tokio::select! {
        status = child.wait() => {
            let status = status.map_err(RuntimeError::WaitForProcess)?;
            bridge.shutdown();
            bridge.wait().await?;
            Ok(Completion::Exited(status))
        }
        signal = cancellation.cancelled() => {
            terminate_child(child, plan, signal).await?;
            bridge.shutdown();
            bridge.wait().await?;
            Ok(Completion::Cancelled(signal))
        }
        bridge_result = bridge.wait() => {
            let bridge_error = bridge_result.err();
            terminate_child(child, plan, SignalKind::Terminate).await?;
            match bridge_error {
                Some(error) => Err(RuntimeError::Bridge(error)),
                None => Err(RuntimeError::BridgeExited),
            }
        }
    }
}

async fn wait_for_child(
    child: &mut Child,
    plan: &LaunchPlan,
    cancellation: &CancellationToken,
) -> Result<Completion, RuntimeError> {
    tokio::select! {
        status = child.wait() => status
            .map(Completion::Exited)
            .map_err(RuntimeError::WaitForProcess),
        signal = cancellation.cancelled() => {
            terminate_child(child, plan, signal).await?;
            Ok(Completion::Cancelled(signal))
        }
    }
}

async fn terminate_child(
    child: &mut Child,
    plan: &LaunchPlan,
    signal: SignalKind,
) -> Result<(), RuntimeError> {
    if plan.process.forward_signals {
        forward_signal(child, signal)?;
    } else {
        child.start_kill().map_err(RuntimeError::TerminateProcess)?;
    }
    let grace = Duration::from_millis(u64::from(plan.cleanup.grace_period_ms));
    if tokio::time::timeout(grace, child.wait()).await.is_err() {
        child.kill().await.map_err(RuntimeError::TerminateProcess)?;
    }
    Ok(())
}

#[cfg(unix)]
fn forward_signal(child: &mut Child, signal: SignalKind) -> Result<(), RuntimeError> {
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;

    let process_id = child.id().ok_or(RuntimeError::MissingProcessId)?;
    let process_id = i32::try_from(process_id).map_err(|_| RuntimeError::MissingProcessId)?;
    let native_signal = match signal {
        SignalKind::Interrupt => Signal::SIGINT,
        SignalKind::Terminate => Signal::SIGTERM,
    };
    kill(Pid::from_raw(process_id), native_signal).map_err(|error| {
        RuntimeError::TerminateProcess(std::io::Error::from_raw_os_error(error as i32))
    })
}

#[cfg(not(unix))]
fn forward_signal(child: &mut Child, _signal: SignalKind) -> Result<(), RuntimeError> {
    child.start_kill().map_err(RuntimeError::TerminateProcess)
}

fn copy_secret(
    secrets: &nan_harness_core::SecretStore,
    reference: &nan_harness_core::SecretRef,
) -> Result<Arc<SecretValue>, RuntimeError> {
    secrets
        .with_secret(reference, |value| SecretValue::new(value.to_owned()))
        .map_err(RuntimeError::Secret)?
        .map(Arc::new)
        .map_err(RuntimeError::Secret)
}

fn generate_session_token() -> Result<SecretValue, RuntimeError> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes).map_err(RuntimeError::Random)?;
    let mut token = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut token, "{byte:02x}").expect("writing to a String cannot fail");
    }
    SecretValue::new(token).map_err(RuntimeError::Secret)
}

fn report(
    plan: &LaunchPlan,
    completion: Completion,
    temporary_root: Option<PathBuf>,
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
    }
}

#[derive(Clone, Copy)]
enum Completion {
    Exited(ExitStatus),
    Cancelled(SignalKind),
}

fn exit_code_from_status(status: ExitStatus) -> i32 {
    status.code().unwrap_or(1)
}

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("launch plan is invalid: {0}")]
    InvalidPlan(PlanError),
    #[error("the selected bridge is not implemented yet")]
    UnsupportedBridge,
    #[error("could not bind the local bridge: {0}")]
    BindBridge(std::io::Error),
    #[error(transparent)]
    Bridge(#[from] BridgeError),
    #[error("the local bridge stopped before the harness process")]
    BridgeExited,
    #[error(transparent)]
    Prepared(#[from] PreparedError),
    #[error(transparent)]
    Process(#[from] ProcessError),
    #[error(transparent)]
    Secret(SecretError),
    #[error("could not generate a private bridge token: {0}")]
    Random(getrandom::Error),
    #[error("could not wait for the harness process: {0}")]
    WaitForProcess(std::io::Error),
    #[error("could not terminate the harness process: {0}")]
    TerminateProcess(std::io::Error),
    #[error("the harness process ID is unavailable")]
    MissingProcessId,
}

impl RuntimeError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::InvalidPlan(_) => "NH-RUNTIME-001",
            Self::UnsupportedBridge => "NH-RUNTIME-002",
            Self::BindBridge(_) | Self::Bridge(_) | Self::BridgeExited => "NH-RUNTIME-003",
            Self::Prepared(_) => "NH-RUNTIME-004",
            Self::Process(_) => "NH-RUNTIME-005",
            Self::Secret(_) | Self::Random(_) => "NH-RUNTIME-006",
            Self::WaitForProcess(_) | Self::TerminateProcess(_) | Self::MissingProcessId => {
                "NH-RUNTIME-007"
            }
        }
    }
}
