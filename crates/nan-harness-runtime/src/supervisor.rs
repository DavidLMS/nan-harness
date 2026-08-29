use crate::config::ResolvedConfig;
use crate::prepared::{BridgePreparation, PreparedError, PreparedLaunch, requires_model_catalog};
use crate::process::{ProcessError, spawn_child};
use crate::signals::{CancellationToken, SignalKind};
use nan_harness_bridge::{
    BridgeConfig, BridgeDiagnostic, BridgeError, ChatCompletionsBridgeConfig, ClaudeModelCatalog,
    CodexModelCatalog, FxGatewayConfig, FxModelCatalog, ProviderUsageSnapshot,
    ResponsesBridgeConfig, RunningBridge, discover_coding_models,
};
use nan_harness_core::launch_plan::{
    CODEX_HOME_OVERLAY_ID, CODEX_PROFILE_ARTIFACT_ID, ListenAddress, Transport,
};
use nan_harness_core::{
    LaunchPlan, LaunchPlanValidator, PlanError, ReasoningHint, ReasoningPolicy, ReasoningSelection,
    SecretError, SecretValue,
};
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
    pub selected_model: Option<String>,
    pub selected_reasoning: Option<ReasoningSelection>,
    pub bridge_diagnostics: Vec<BridgeDiagnostic>,
    pub provider_usage: Option<ProviderUsageSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CodexSelection {
    model: String,
    reasoning: Option<ReasoningSelection>,
}

#[derive(Debug)]
pub struct Supervisor {
    direct_chat_gateway: bool,
}

impl Default for Supervisor {
    fn default() -> Self {
        Self::new()
    }
}

impl Supervisor {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            direct_chat_gateway: true,
        }
    }

    #[must_use]
    pub const fn without_direct_chat_gateway(mut self) -> Self {
        self.direct_chat_gateway = false;
        self
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
            Transport::DirectChat { .. } if self.direct_chat_gateway => {
                execute_direct_with_gateway(plan, config, cancellation).await
            }
            Transport::DirectChat { .. } => {
                execute_direct_without_gateway(plan, config, cancellation).await
            }
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
            Transport::ResponsesBridge {
                listen,
                provider_credential_ref,
                session_token_ref,
                ..
            } => {
                execute_responses_bridge(
                    plan,
                    config,
                    cancellation,
                    listen,
                    provider_credential_ref,
                    session_token_ref,
                )
                .await
            }
            Transport::FxGatewayBridge {
                listen,
                provider_credential_ref,
                session_token_ref,
            } => {
                execute_fx_gateway(
                    plan,
                    config,
                    cancellation,
                    listen,
                    provider_credential_ref,
                    session_token_ref,
                )
                .await
            }
        }
    }
}

async fn execute_responses_bridge(
    plan: &LaunchPlan,
    config: &ResolvedConfig,
    cancellation: &CancellationToken,
    listen: &ListenAddress,
    provider_credential_ref: &nan_harness_core::SecretRef,
    session_token_ref: &nan_harness_core::SecretRef,
) -> Result<ExecutionReport, RuntimeError> {
    let provider_api_key = copy_secret(&config.secrets, provider_credential_ref)?;
    let listener = TcpListener::bind((listen.host.as_str(), listen.port))
        .await
        .map_err(RuntimeError::BindBridge)?;
    let address = listener.local_addr().map_err(RuntimeError::BindBridge)?;
    let base_url = format!("http://{address}");
    let session_token = Arc::new(generate_session_token()?);
    let discovered_models =
        discover_coding_models(&config.provider_base_url, Arc::clone(&provider_api_key)).await?;
    validate_selected_model(&discovered_models, &plan.model.resolved_id)?;
    let models =
        CodexModelCatalog::from_models(discovered_models.clone(), &plan.model.resolved_id)?;
    let prepared = PreparedLaunch::prepare(
        plan,
        &config.provider_base_url,
        Some(BridgePreparation {
            base_url,
            client_base_url: None,
            chat_url: None,
            session_token_ref: session_token_ref.clone(),
            session_token: Arc::clone(&session_token),
            claude_available_models: Vec::new(),
            codex_model_catalog: Some(models.api_response().to_string()),
        }),
        Some(&discovered_models),
    )?;
    let temporary_root = prepared.temporary_root(has_temporary_resources(plan));
    let mut bridge = nan_harness_bridge::spawn_responses(
        listener,
        ResponsesBridgeConfig {
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

    let mut bridge_diagnostics = Vec::new();
    let completion = supervise_pair(
        &mut child,
        &mut bridge,
        plan,
        cancellation,
        &mut bridge_diagnostics,
    )
    .await?;
    let selected = matches!(completion, Completion::Exited(status) if status.success())
        .then(|| prepared_codex_selection(&prepared, &discovered_models))
        .flatten();
    let provider_usage = Some(bridge.usage());
    Ok(report(
        plan,
        completion,
        temporary_root,
        selected,
        bridge_diagnostics,
        provider_usage,
    ))
}

async fn execute_fx_gateway(
    plan: &LaunchPlan,
    config: &ResolvedConfig,
    cancellation: &CancellationToken,
    listen: &ListenAddress,
    provider_credential_ref: &nan_harness_core::SecretRef,
    session_token_ref: &nan_harness_core::SecretRef,
) -> Result<ExecutionReport, RuntimeError> {
    let provider_api_key = copy_secret(&config.secrets, provider_credential_ref)?;
    let listener = TcpListener::bind((listen.host.as_str(), listen.port))
        .await
        .map_err(RuntimeError::BindBridge)?;
    let address = listener.local_addr().map_err(RuntimeError::BindBridge)?;
    let base_url = format!("http://{address}");
    let chat_url = format!("{base_url}/v3/ai/language-model");
    let session_token = Arc::new(generate_session_token()?);
    let discovered_models =
        discover_coding_models(&config.provider_base_url, Arc::clone(&provider_api_key)).await?;
    validate_selected_model(&discovered_models, &plan.model.resolved_id)?;
    let models = FxModelCatalog::from_models(discovered_models.clone())?;
    let prepared = PreparedLaunch::prepare(
        plan,
        &config.provider_base_url,
        Some(BridgePreparation {
            base_url,
            client_base_url: None,
            chat_url: Some(chat_url),
            session_token_ref: session_token_ref.clone(),
            session_token: Arc::clone(&session_token),
            claude_available_models: Vec::new(),
            codex_model_catalog: None,
        }),
        Some(&discovered_models),
    )?;
    let temporary_root = prepared.temporary_root(has_temporary_resources(plan));
    let mut bridge = nan_harness_bridge::spawn_fx_gateway(
        listener,
        FxGatewayConfig {
            provider_base_url: config.provider_base_url.clone(),
            models,
            selected_model_id: plan.model.resolved_id.clone(),
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
    let mut bridge_diagnostics = Vec::new();
    let completion = supervise_pair(
        &mut child,
        &mut bridge,
        plan,
        cancellation,
        &mut bridge_diagnostics,
    )
    .await?;
    let provider_usage = Some(bridge.usage());
    Ok(report(
        plan,
        completion,
        temporary_root,
        None,
        bridge_diagnostics,
        provider_usage,
    ))
}

async fn execute_direct_with_gateway(
    plan: &LaunchPlan,
    config: &ResolvedConfig,
    cancellation: &CancellationToken,
) -> Result<ExecutionReport, RuntimeError> {
    let provider_api_key = copy_secret(&config.secrets, &config.provider_credential_ref)?;
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(RuntimeError::BindBridge)?;
    let address = listener.local_addr().map_err(RuntimeError::BindBridge)?;
    let base_url = format!("http://{address}");
    let client_base_url = format!("{}/v1", base_url.trim_end_matches('/'));
    let session_token = Arc::new(generate_session_token()?);
    let session_token_ref = match &plan.transport {
        Transport::DirectChat {
            credential_target, ..
        } => plan
            .environment
            .secrets
            .get(credential_target)
            .cloned()
            .ok_or_else(|| {
                RuntimeError::InvalidPlan(PlanError::MissingSecretReference {
                    reference: credential_target.clone(),
                })
            })?,
        _ => unreachable!("execute_direct requires DirectChat"),
    };
    let discovered_models = if requires_model_catalog(plan) {
        let models =
            discover_coding_models(&config.provider_base_url, Arc::clone(&provider_api_key))
                .await?;
        validate_selected_model(&models, &plan.model.resolved_id)?;
        Some(models)
    } else {
        None
    };
    let prepared = PreparedLaunch::prepare(
        plan,
        &config.provider_base_url,
        Some(BridgePreparation {
            base_url: base_url.clone(),
            client_base_url: Some(client_base_url),
            chat_url: None,
            session_token_ref,
            session_token: Arc::clone(&session_token),
            claude_available_models: Vec::new(),
            codex_model_catalog: None,
        }),
        discovered_models.as_deref(),
    )?;
    let temporary_root = prepared.temporary_root(has_temporary_resources(plan));
    let mut bridge = nan_harness_bridge::spawn_chat_completions(
        listener,
        ChatCompletionsBridgeConfig {
            provider_base_url: config.provider_base_url.clone(),
            model_id: plan.model.resolved_id.clone(),
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
    let mut bridge_diagnostics = Vec::new();
    let completion = supervise_pair(
        &mut child,
        &mut bridge,
        plan,
        cancellation,
        &mut bridge_diagnostics,
    )
    .await?;
    let provider_usage = Some(bridge.usage());
    Ok(report(
        plan,
        completion,
        temporary_root,
        None,
        bridge_diagnostics,
        provider_usage,
    ))
}

async fn execute_direct_without_gateway(
    plan: &LaunchPlan,
    config: &ResolvedConfig,
    cancellation: &CancellationToken,
) -> Result<ExecutionReport, RuntimeError> {
    let discovered_models = if requires_model_catalog(plan) {
        let provider_api_key = copy_secret(&config.secrets, &config.provider_credential_ref)?;
        let models = discover_coding_models(&config.provider_base_url, provider_api_key).await?;
        validate_selected_model(&models, &plan.model.resolved_id)?;
        Some(models)
    } else {
        None
    };
    let prepared = PreparedLaunch::prepare(
        plan,
        &config.provider_base_url,
        None,
        discovered_models.as_deref(),
    )?;
    let temporary_root = prepared.temporary_root(has_temporary_resources(plan));
    let mut child = spawn_child(plan, &prepared, &config.secrets)?;
    let completion = wait_for_child(&mut child, plan, cancellation).await?;
    Ok(report(
        plan,
        completion,
        temporary_root,
        None,
        Vec::new(),
        None,
    ))
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
    let discovered_models =
        discover_coding_models(&config.provider_base_url, Arc::clone(&provider_api_key)).await?;
    validate_selected_model(&discovered_models, &plan.model.resolved_id)?;
    let models =
        ClaudeModelCatalog::from_models(discovered_models.clone(), &plan.model.resolved_id)?;
    let claude_available_models = models.gateway_ids();
    let listener = TcpListener::bind((listen.host.as_str(), listen.port))
        .await
        .map_err(RuntimeError::BindBridge)?;
    let address = listener.local_addr().map_err(RuntimeError::BindBridge)?;
    let base_url = format!("http://{address}");
    let session_token = Arc::new(generate_session_token()?);
    let prepared = PreparedLaunch::prepare(
        plan,
        &config.provider_base_url,
        Some(BridgePreparation {
            base_url,
            client_base_url: None,
            chat_url: None,
            session_token_ref: session_token_ref.clone(),
            session_token: Arc::clone(&session_token),
            claude_available_models,
            codex_model_catalog: None,
        }),
        Some(&discovered_models),
    )?;
    let temporary_root = prepared.temporary_root(has_temporary_resources(plan));
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

    let mut bridge_diagnostics = Vec::new();
    let completion = supervise_pair(
        &mut child,
        &mut bridge,
        plan,
        cancellation,
        &mut bridge_diagnostics,
    )
    .await?;
    let provider_usage = Some(bridge.usage());
    Ok(report(
        plan,
        completion,
        temporary_root,
        None,
        bridge_diagnostics,
        provider_usage,
    ))
}

async fn supervise_pair(
    child: &mut Child,
    bridge: &mut RunningBridge,
    plan: &LaunchPlan,
    cancellation: &CancellationToken,
    bridge_diagnostics: &mut Vec<BridgeDiagnostic>,
) -> Result<Completion, RuntimeError> {
    let mut diagnostics_rx = bridge.take_diagnostics();
    loop {
        tokio::select! {
            status = child.wait() => {
                let status = status.map_err(RuntimeError::WaitForProcess)?;
                bridge.shutdown();
                bridge.wait().await?;
                drain_bridge_diagnostics(&mut diagnostics_rx, bridge_diagnostics);
                return Ok(Completion::Exited(status));
            }
            signal = cancellation.cancelled() => {
                terminate_child(child, plan, signal, cancellation).await?;
                bridge.shutdown();
                bridge.wait().await?;
                drain_bridge_diagnostics(&mut diagnostics_rx, bridge_diagnostics);
                return Ok(Completion::Cancelled(signal));
            }
            bridge_result = bridge.wait() => {
                let bridge_error = bridge_result.err();
                terminate_child(child, plan, SignalKind::Terminate, cancellation).await?;
                return match bridge_error {
                    Some(error) => Err(RuntimeError::Bridge(error)),
                    None => Err(RuntimeError::BridgeExited),
                };
            }
            diagnostic = diagnostics_rx.recv() => {
                if let Some(diagnostic) = diagnostic {
                    push_bridge_diagnostic(bridge_diagnostics, diagnostic);
                }
            }
        }
    }
}

fn drain_bridge_diagnostics(
    receiver: &mut tokio::sync::mpsc::UnboundedReceiver<BridgeDiagnostic>,
    diagnostics: &mut Vec<BridgeDiagnostic>,
) {
    while let Ok(diagnostic) = receiver.try_recv() {
        push_bridge_diagnostic(diagnostics, diagnostic);
    }
}

fn push_bridge_diagnostic(diagnostics: &mut Vec<BridgeDiagnostic>, diagnostic: BridgeDiagnostic) {
    if !diagnostics.contains(&diagnostic) {
        diagnostics.push(diagnostic);
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
            terminate_child(child, plan, signal, cancellation).await?;
            Ok(Completion::Cancelled(signal))
        }
    }
}

async fn terminate_child(
    child: &mut Child,
    plan: &LaunchPlan,
    signal: SignalKind,
    cancellation: &CancellationToken,
) -> Result<(), RuntimeError> {
    if plan.process.forward_signals {
        forward_signal(child, signal)?;
    } else if let Err(error) = child.start_kill()
        && !is_process_gone_error(&error)
    {
        return Err(RuntimeError::TerminateProcess(error));
    }
    let grace = Duration::from_millis(u64::from(plan.cleanup.grace_period_ms));
    tokio::select! {
        result = child.wait() => reap_result(result),
        () = cancellation.force_cancelled() => kill_and_reap(child).await,
        () = tokio::time::sleep(grace) => kill_and_reap(child).await,
    }
}

fn reap_result(result: std::io::Result<ExitStatus>) -> Result<(), RuntimeError> {
    match result {
        Ok(_) => Ok(()),
        Err(error) if is_process_gone_error(&error) => Ok(()),
        Err(error) => Err(RuntimeError::WaitForProcess(error)),
    }
}

async fn kill_and_reap(child: &mut Child) -> Result<(), RuntimeError> {
    match child.kill().await {
        Ok(()) => Ok(()),
        Err(error) if is_process_gone_error(&error) => reap_child(child).await,
        Err(error) => Err(RuntimeError::TerminateProcess(error)),
    }
}

async fn reap_child(child: &mut Child) -> Result<(), RuntimeError> {
    match child.wait().await {
        Ok(_) => Ok(()),
        Err(error) if is_process_gone_error(&error) => Ok(()),
        Err(error) => Err(RuntimeError::WaitForProcess(error)),
    }
}

fn is_process_gone_error(error: &std::io::Error) -> bool {
    if error.kind() == std::io::ErrorKind::InvalidInput {
        return true;
    }

    #[cfg(unix)]
    {
        matches!(error.raw_os_error(), Some(code) if
            code == nix::libc::ECHILD || code == nix::libc::ESRCH
        )
    }

    #[cfg(not(unix))]
    {
        false
    }
}

#[cfg(unix)]
fn forward_signal(child: &mut Child, signal: SignalKind) -> Result<(), RuntimeError> {
    use nix::errno::Errno;
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;

    let Some(process_id) = child.id() else {
        return Ok(());
    };
    let process_id = i32::try_from(process_id).map_err(|_| RuntimeError::MissingProcessId)?;
    let native_signal = match signal {
        SignalKind::Interrupt => Signal::SIGINT,
        SignalKind::Terminate => Signal::SIGTERM,
    };
    match kill(Pid::from_raw(process_id), native_signal) {
        Ok(()) | Err(Errno::ESRCH) => Ok(()),
        Err(error) => Err(RuntimeError::TerminateProcess(
            std::io::Error::from_raw_os_error(error as i32),
        )),
    }
}

#[cfg(not(unix))]
fn forward_signal(child: &mut Child, _signal: SignalKind) -> Result<(), RuntimeError> {
    child
        .start_kill()
        .or_else(|error| {
            if is_process_gone_error(&error) {
                Ok(())
            } else {
                Err(error)
            }
        })
        .map_err(RuntimeError::TerminateProcess)
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

fn validate_selected_model(
    models: &[nan_harness_core::CodingModelProfile],
    selected_model: &str,
) -> Result<(), BridgeError> {
    if models.is_empty() {
        return Err(BridgeError::NoCompatibleModels);
    }
    if models.iter().any(|model| model.id == selected_model) {
        Ok(())
    } else {
        Err(BridgeError::SelectedModelUnavailable {
            model: selected_model.to_owned(),
            available: models.iter().map(|model| model.id.clone()).collect(),
        })
    }
}

fn generate_session_token() -> Result<SecretValue, RuntimeError> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes).map_err(RuntimeError::Random)?;
    let mut token = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(&mut token, "{byte:02x}");
    }
    SecretValue::new(token).map_err(RuntimeError::Secret)
}

fn report(
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

fn prepared_codex_selection(
    prepared: &PreparedLaunch,
    models: &[nan_harness_core::CodingModelProfile],
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

fn parse_codex_reasoning(value: &str, policy: ReasoningPolicy) -> Option<ReasoningSelection> {
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

fn has_temporary_resources(plan: &LaunchPlan) -> bool {
    !plan.temporary_artifacts.is_empty()
        || !plan.configuration_overlays.is_empty()
        || !plan.launch_scoped_files.is_empty()
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
            Self::BindBridge(_) | Self::Bridge(_) | Self::BridgeExited => "NH-RUNTIME-003",
            Self::Prepared(_) => "NH-RUNTIME-004",
            Self::Process(_) => "NH-RUNTIME-005",
            Self::Secret(_) | Self::Random(_) => "NH-RUNTIME-006",
            Self::WaitForProcess(_) | Self::TerminateProcess(_) | Self::MissingProcessId => {
                "NH-RUNTIME-007"
            }
        }
    }

    #[must_use]
    pub fn unavailable_model(&self) -> Option<(&str, &[String])> {
        match self {
            Self::Bridge(BridgeError::SelectedModelUnavailable { model, available }) => {
                Some((model, available))
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CancellationToken, SignalKind, is_process_gone_error, parse_codex_reasoning,
        terminate_child,
    };
    use nan_harness_core::{LaunchPlan, ReasoningEffort, ReasoningPolicy, ReasoningSelection};

    #[cfg(unix)]
    #[tokio::test]
    async fn terminate_child_treats_an_already_reaped_process_as_success() {
        let mut child = tokio::process::Command::new("/bin/sh")
            .args(["-c", "exit 0"])
            .spawn()
            .expect("child should spawn");
        child.wait().await.expect("child should be reaped");
        let plan: LaunchPlan = serde_json::from_str(include_str!(
            "../../nan-harness-core/tests/fixtures/launch-plan.direct.json"
        ))
        .expect("fixture should be valid");

        terminate_child(
            &mut child,
            &plan,
            SignalKind::Interrupt,
            &CancellationToken::new(),
        )
        .await
        .expect("cancellation should tolerate a reaped child");
    }

    #[cfg(unix)]
    #[test]
    fn process_gone_errors_are_recognized() {
        assert!(is_process_gone_error(&std::io::Error::from_raw_os_error(
            nix::libc::ESRCH
        )));
        assert!(is_process_gone_error(&std::io::Error::from_raw_os_error(
            nix::libc::ECHILD
        )));
        assert!(!is_process_gone_error(&std::io::Error::from_raw_os_error(
            nix::libc::EPERM
        )));
    }

    #[test]
    fn codex_reasoning_state_uses_shared_policy_resolution() {
        assert_eq!(
            parse_codex_reasoning("medium", ReasoningPolicy::AlwaysOn),
            Some(ReasoningSelection::Toggle(true))
        );
        assert_eq!(
            parse_codex_reasoning(
                "medium",
                ReasoningPolicy::Toggle {
                    default_enabled: false,
                }
            ),
            Some(ReasoningSelection::Toggle(true))
        );
        assert_eq!(
            parse_codex_reasoning(
                "medium",
                ReasoningPolicy::Effort {
                    supported: [
                        ReasoningEffort::Low,
                        ReasoningEffort::Medium,
                        ReasoningEffort::High,
                    ],
                    default: ReasoningEffort::Medium,
                }
            ),
            Some(ReasoningSelection::Effort(ReasoningEffort::Medium))
        );
        assert_eq!(
            parse_codex_reasoning("medium", ReasoningPolicy::Unknown),
            Some(ReasoningSelection::Auto)
        );
    }
}
