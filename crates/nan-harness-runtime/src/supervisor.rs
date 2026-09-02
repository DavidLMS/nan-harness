mod lifecycle;

use crate::config::ResolvedConfig;
use crate::prepared::{BridgePreparation, PreparedError, PreparedLaunch, requires_model_catalog};
use crate::process::{ProcessError, spawn_child};
use crate::search_policy::{SearchPolicyError, resolve as resolve_search_policy};
use crate::signals::{CancellationToken, SignalKind};
use lifecycle::{BridgeExecution, run_bridged_child, wait_for_child};
use nan_harness_bridge::{
    BridgeConfig, BridgeDiagnostic, BridgeError, ChatCompletionsBridgeConfig, ClaudeModelCatalog,
    CodexModelCatalog, FxGatewayConfig, FxModelCatalog, ProviderUsageSnapshot,
    ResponsesBridgeConfig, discover_coding_models,
};
use nan_harness_core::launch_plan::{
    CODEX_HOME_OVERLAY_ID, CODEX_PROFILE_ARTIFACT_ID, ListenAddress, Transport,
};
use nan_harness_core::{
    CodingModelProfile, LaunchPlan, LaunchPlanValidator, PlanError, ReasoningHint, ReasoningPolicy,
    ReasoningSelection, SecretError, SecretValue,
};
use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::ExitStatus;
use std::sync::Arc;
use thiserror::Error;
use tokio::net::TcpListener;
use tokio::sync::OnceCell;

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

#[derive(Debug)]
pub struct LaunchSession<'a> {
    config: &'a ResolvedConfig,
    model_catalog: OnceCell<Vec<CodingModelProfile>>,
}

#[derive(Clone, Copy)]
struct BridgeLaunchOptions<'a> {
    discovered_models: &'a [CodingModelProfile],
    web_search_enabled: bool,
}

struct BoundBridgeEndpoint {
    listener: TcpListener,
    base_url: String,
}

impl BoundBridgeEndpoint {
    async fn bind_transport(listen: &ListenAddress) -> Result<Self, RuntimeError> {
        let listener = TcpListener::bind((listen.host.as_str(), listen.port))
            .await
            .map_err(RuntimeError::BindBridge)?;
        Self::from_listener(listener)
    }

    async fn bind_direct_chat_gateway() -> Result<Self, RuntimeError> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(RuntimeError::BindBridge)?;
        Self::from_listener(listener)
    }

    fn from_listener(listener: TcpListener) -> Result<Self, RuntimeError> {
        let address = listener.local_addr().map_err(RuntimeError::BindBridge)?;
        Ok(Self {
            listener,
            base_url: format!("http://{address}"),
        })
    }
}

struct PreparedHarnessLaunch {
    prepared: PreparedLaunch,
    temporary_root: Option<PathBuf>,
}

impl PreparedHarnessLaunch {
    fn prepare(
        plan: &LaunchPlan,
        provider_base_url: &str,
        bridge: Option<BridgePreparation>,
        model_catalog: Option<&[CodingModelProfile]>,
    ) -> Result<Self, PreparedError> {
        let prepared = PreparedLaunch::prepare(plan, provider_base_url, bridge, model_catalog)?;
        let temporary_root = prepared.temporary_root(has_temporary_resources(plan));
        Ok(Self {
            prepared,
            temporary_root,
        })
    }
}

impl<'a> LaunchSession<'a> {
    #[must_use]
    pub const fn new(config: &'a ResolvedConfig) -> Self {
        Self {
            config,
            model_catalog: OnceCell::const_new(),
        }
    }

    #[must_use]
    pub fn with_model_catalog(
        config: &'a ResolvedConfig,
        model_catalog: Vec<CodingModelProfile>,
    ) -> Self {
        Self {
            config,
            model_catalog: OnceCell::new_with(Some(model_catalog)),
        }
    }

    /// Returns the credential-bound catalog snapshot for this launch session.
    ///
    /// Repeated calls reuse the same bounded discovery result.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError`] when the provider credential cannot be resolved or model
    /// discovery fails.
    pub async fn model_catalog(&self) -> Result<&[CodingModelProfile], RuntimeError> {
        let models = self
            .model_catalog
            .get_or_try_init(|| async {
                let provider_api_key =
                    copy_secret(&self.config.secrets, &self.config.provider_credential_ref)?;
                discover_coding_models(&self.config.provider_base_url, provider_api_key)
                    .await
                    .map_err(RuntimeError::Bridge)
            })
            .await?;
        Ok(models.as_slice())
    }
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
        let session = LaunchSession::new(config);
        self.execute_in_session(plan, &session, cancellation).await
    }

    /// Validates, prepares, and supervises one launch while reusing its model catalog.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError`] when validation, model discovery, setup, process control, or
    /// cleanup fails.
    pub async fn execute_in_session(
        &self,
        plan: &LaunchPlan,
        session: &LaunchSession<'_>,
        cancellation: &CancellationToken,
    ) -> Result<ExecutionReport, RuntimeError> {
        LaunchPlanValidator::validate(plan).map_err(RuntimeError::InvalidPlan)?;
        let web_search_enabled = resolve_search_policy(plan, self.direct_chat_gateway)?.uses_nan();
        let model_catalog_required = match &plan.transport {
            Transport::DirectChat { .. } => requires_model_catalog(plan),
            Transport::AnthropicBridge { .. }
            | Transport::ResponsesBridge { .. }
            | Transport::FxGatewayBridge { .. } => true,
        };
        let model_catalog = if model_catalog_required {
            let models = session.model_catalog().await?;
            validate_selected_model(models, &plan.model.resolved_id)?;
            Some(models)
        } else {
            None
        };
        let config = session.config;
        match &plan.transport {
            Transport::DirectChat { .. } if self.direct_chat_gateway => {
                execute_direct_with_gateway(
                    plan,
                    config,
                    cancellation,
                    model_catalog,
                    web_search_enabled,
                )
                .await
            }
            Transport::DirectChat { .. } => {
                execute_direct_without_gateway(plan, config, cancellation, model_catalog).await
            }
            Transport::AnthropicBridge {
                listen,
                provider_credential_ref,
                session_token_ref,
                ..
            } => {
                execute_anthropic_bridge(
                    plan,
                    config,
                    cancellation,
                    listen,
                    provider_credential_ref,
                    session_token_ref,
                    BridgeLaunchOptions {
                        discovered_models: model_catalog.unwrap_or_default(),
                        web_search_enabled,
                    },
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
                    BridgeLaunchOptions {
                        discovered_models: model_catalog.unwrap_or_default(),
                        web_search_enabled,
                    },
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
                    BridgeLaunchOptions {
                        discovered_models: model_catalog.unwrap_or_default(),
                        web_search_enabled,
                    },
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
    options: BridgeLaunchOptions<'_>,
) -> Result<ExecutionReport, RuntimeError> {
    let BridgeLaunchOptions {
        discovered_models,
        web_search_enabled,
    } = options;
    let provider_api_key = copy_secret(&config.secrets, provider_credential_ref)?;
    let BoundBridgeEndpoint { listener, base_url } =
        BoundBridgeEndpoint::bind_transport(listen).await?;
    let session_token = Arc::new(generate_session_token()?);
    let models =
        CodexModelCatalog::from_models(discovered_models.to_vec(), &plan.model.resolved_id)?;
    let launch = PreparedHarnessLaunch::prepare(
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
            web_search_enabled,
        }),
        Some(discovered_models),
    )?;
    let mut bridge = nan_harness_bridge::spawn_responses(
        listener,
        ResponsesBridgeConfig {
            provider_base_url: config.provider_base_url.clone(),
            models,
            provider_api_key,
            session_token,
            web_search_enabled,
        },
    )?;
    let execution = run_bridged_child(
        plan,
        &launch.prepared,
        &config.secrets,
        cancellation,
        &mut bridge,
    )
    .await?;
    let selected = matches!(execution.completion, Completion::Exited(status) if status.success())
        .then(|| prepared_codex_selection(&launch.prepared, discovered_models))
        .flatten();
    Ok(bridged_report(
        plan,
        execution,
        launch.temporary_root,
        selected,
    ))
}

async fn execute_fx_gateway(
    plan: &LaunchPlan,
    config: &ResolvedConfig,
    cancellation: &CancellationToken,
    listen: &ListenAddress,
    provider_credential_ref: &nan_harness_core::SecretRef,
    session_token_ref: &nan_harness_core::SecretRef,
    options: BridgeLaunchOptions<'_>,
) -> Result<ExecutionReport, RuntimeError> {
    let BridgeLaunchOptions {
        discovered_models,
        web_search_enabled,
    } = options;
    let provider_api_key = copy_secret(&config.secrets, provider_credential_ref)?;
    let BoundBridgeEndpoint { listener, base_url } =
        BoundBridgeEndpoint::bind_transport(listen).await?;
    let chat_url = format!("{base_url}/v3/ai/language-model");
    let session_token = Arc::new(generate_session_token()?);
    let models = FxModelCatalog::from_models(discovered_models.to_vec())?;
    let launch = PreparedHarnessLaunch::prepare(
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
            web_search_enabled,
        }),
        Some(discovered_models),
    )?;
    let mut bridge = nan_harness_bridge::spawn_fx_gateway(
        listener,
        FxGatewayConfig {
            provider_base_url: config.provider_base_url.clone(),
            models,
            selected_model_id: plan.model.resolved_id.clone(),
            provider_api_key,
            session_token,
            web_search_enabled,
        },
    )?;
    let execution = run_bridged_child(
        plan,
        &launch.prepared,
        &config.secrets,
        cancellation,
        &mut bridge,
    )
    .await?;
    Ok(bridged_report(plan, execution, launch.temporary_root, None))
}

async fn execute_direct_with_gateway(
    plan: &LaunchPlan,
    config: &ResolvedConfig,
    cancellation: &CancellationToken,
    discovered_models: Option<&[CodingModelProfile]>,
    web_search_enabled: bool,
) -> Result<ExecutionReport, RuntimeError> {
    let provider_api_key = copy_secret(&config.secrets, &config.provider_credential_ref)?;
    let BoundBridgeEndpoint { listener, base_url } =
        BoundBridgeEndpoint::bind_direct_chat_gateway().await?;
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
    let launch = PreparedHarnessLaunch::prepare(
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
            web_search_enabled,
        }),
        discovered_models,
    )?;
    let mut bridge = nan_harness_bridge::spawn_chat_completions(
        listener,
        ChatCompletionsBridgeConfig {
            provider_base_url: config.provider_base_url.clone(),
            model_id: plan.model.resolved_id.clone(),
            provider_api_key,
            session_token,
            web_search_enabled,
        },
    )?;
    let execution = run_bridged_child(
        plan,
        &launch.prepared,
        &config.secrets,
        cancellation,
        &mut bridge,
    )
    .await?;
    Ok(bridged_report(plan, execution, launch.temporary_root, None))
}

async fn execute_direct_without_gateway(
    plan: &LaunchPlan,
    config: &ResolvedConfig,
    cancellation: &CancellationToken,
    discovered_models: Option<&[CodingModelProfile]>,
) -> Result<ExecutionReport, RuntimeError> {
    let launch =
        PreparedHarnessLaunch::prepare(plan, &config.provider_base_url, None, discovered_models)?;
    let mut child = spawn_child(plan, &launch.prepared, &config.secrets)?;
    let completion = wait_for_child(&mut child, plan, cancellation).await?;
    Ok(report(
        plan,
        completion,
        launch.temporary_root,
        None,
        Vec::new(),
        None,
    ))
}

async fn execute_anthropic_bridge(
    plan: &LaunchPlan,
    config: &ResolvedConfig,
    cancellation: &CancellationToken,
    listen: &ListenAddress,
    provider_credential_ref: &nan_harness_core::SecretRef,
    session_token_ref: &nan_harness_core::SecretRef,
    options: BridgeLaunchOptions<'_>,
) -> Result<ExecutionReport, RuntimeError> {
    let BridgeLaunchOptions {
        discovered_models,
        web_search_enabled,
    } = options;
    let provider_api_key = copy_secret(&config.secrets, provider_credential_ref)?;
    let models =
        ClaudeModelCatalog::from_models(discovered_models.to_vec(), &plan.model.resolved_id)?;
    let claude_available_models = models.gateway_ids();
    let BoundBridgeEndpoint { listener, base_url } =
        BoundBridgeEndpoint::bind_transport(listen).await?;
    let session_token = Arc::new(generate_session_token()?);
    let launch = PreparedHarnessLaunch::prepare(
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
            web_search_enabled,
        }),
        Some(discovered_models),
    )?;
    let mut bridge = nan_harness_bridge::spawn(
        listener,
        BridgeConfig {
            provider_base_url: config.provider_base_url.clone(),
            models,
            provider_api_key,
            session_token,
            web_search_enabled,
            auto_mode_traces: false,
        },
    )?;
    let execution = run_bridged_child(
        plan,
        &launch.prepared,
        &config.secrets,
        cancellation,
        &mut bridge,
    )
    .await?;
    Ok(bridged_report(plan, execution, launch.temporary_root, None))
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
    models: &[CodingModelProfile],
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

fn bridged_report(
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

fn prepared_codex_selection(
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
    #[error(transparent)]
    SearchPolicy(#[from] SearchPolicyError),
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
            Self::SearchPolicy(_) => "NH-RUNTIME-008",
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
    use super::parse_codex_reasoning;
    #[cfg(unix)]
    use super::{ExecutionOutcome, LaunchSession, Supervisor};
    #[cfg(unix)]
    use crate::config::ResolvedConfig;
    #[cfg(unix)]
    use crate::signals::CancellationToken;
    #[cfg(unix)]
    use nan_harness_bridge::ProviderUsageSnapshot;
    #[cfg(unix)]
    use nan_harness_core::launch_plan::{
        BRIDGE_BASE_URL_PLACEHOLDER, FX_GATEWAY_CHAT_URL_PLACEHOLDER, ListenAddress, TerminalMode,
        Transport,
    };
    #[cfg(unix)]
    use nan_harness_core::{
        HarnessKind, LaunchPlan, SecretRef, SecretStore, SecretValue,
        coding_models_from_provider_ids,
    };
    use nan_harness_core::{ReasoningEffort, ReasoningPolicy, ReasoningSelection};

    #[cfg(unix)]
    #[tokio::test]
    async fn fx_gateway_launch_uses_scoped_credentials_and_explicit_routes() {
        let working_directory = tempfile::tempdir().expect("working directory should exist");
        let mut plan: LaunchPlan = serde_json::from_str(include_str!(
            "../../nan-harness-core/tests/fixtures/launch-plan.bridge.json"
        ))
        .expect("fixture should be valid");
        plan.harness.kind = HarnessKind::Fx;
        "/bin/sh".clone_into(&mut plan.harness.executable);
        let provider_credential_ref =
            SecretRef::new("nan_api_key").expect("valid provider credential reference");
        let session_token_ref =
            SecretRef::new("fx_gateway_session_token").expect("valid session token reference");
        plan.transport = Transport::FxGatewayBridge {
            listen: ListenAddress {
                host: "127.0.0.1".to_owned(),
                port: 0,
            },
            provider_credential_ref: provider_credential_ref.clone(),
            session_token_ref: session_token_ref.clone(),
        };
        plan.environment.public.clear();
        plan.environment.public.insert(
            "FX_GATEWAY_BASE_URL".to_owned(),
            BRIDGE_BASE_URL_PLACEHOLDER.to_owned(),
        );
        plan.environment.public.insert(
            "FX_GATEWAY_CHAT_URL".to_owned(),
            FX_GATEWAY_CHAT_URL_PLACEHOLDER.to_owned(),
        );
        plan.environment.secrets.clear();
        plan.environment
            .secrets
            .insert("AI_GATEWAY_API_KEY".to_owned(), session_token_ref);
        plan.environment.remove.clear();
        plan.temporary_artifacts.clear();
        plan.configuration_overlays.clear();
        plan.launch_scoped_files.clear();
        plan.observability.redact_environment_names.clear();
        plan.observability
            .redact_environment_names
            .insert("AI_GATEWAY_API_KEY".to_owned());
        plan.process.arguments = vec![
            "-c".to_owned(),
            concat!(
                "test \"${#AI_GATEWAY_API_KEY}\" -eq 64 && ",
                "test \"$AI_GATEWAY_API_KEY\" != test-key && ",
                "case \"$AI_GATEWAY_API_KEY\" in *[!0-9a-f]*) exit 9;; esac && ",
                "case \"$FX_GATEWAY_BASE_URL\" in http://127.0.0.1:*) ;; *) exit 8;; esac && ",
                "test \"$FX_GATEWAY_CHAT_URL\" = ",
                "\"$FX_GATEWAY_BASE_URL/v3/ai/language-model\""
            )
            .to_owned(),
        ];
        plan.process.working_directory = working_directory.path().to_string_lossy().into_owned();
        plan.process.terminal = TerminalMode::Captured;

        let mut secrets = SecretStore::new();
        secrets.insert(
            provider_credential_ref.clone(),
            SecretValue::new("test-key").expect("valid secret value"),
        );
        let config = ResolvedConfig {
            provider_base_url: "http://127.0.0.1:9/v1".to_owned(),
            provider_credential_ref,
            secrets,
        };
        let session = LaunchSession::with_model_catalog(
            &config,
            coding_models_from_provider_ids(["qwen3.6".to_owned()]),
        );

        let report = Supervisor::new()
            .execute_in_session(&plan, &session, &CancellationToken::new())
            .await
            .expect("fx gateway launch should complete");

        assert_eq!(report.outcome, ExecutionOutcome::Succeeded);
        assert_eq!(
            report.provider_usage,
            Some(ProviderUsageSnapshot::default())
        );
        assert!(report.bridge_diagnostics.is_empty());
        assert_eq!(report.temporary_root, None);
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
