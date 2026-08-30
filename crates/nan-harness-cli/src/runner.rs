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
    GooseAdapter, HermesAdapter, KimiCodeAdapter, OpenClawAdapter, OpenCodeAdapter, PiAdapter,
    PrimeAgentAdapter, QwenCodeAdapter,
};
use nan_harness_core::launch_plan::{LaunchId, ObservabilityFormat};
use nan_harness_core::model::{
    ModelAvailability, ProfileSource, QualificationStatus, ReasoningEffort, ReasoningSelection,
};
use nan_harness_core::{
    HarnessAdapter, HarnessKind, LaunchPlan, PlanContext, ResolvedModel, WebSearchPolicy,
    build_validated_plan,
};
use nan_harness_runtime::BridgeDiagnostic;
use nan_harness_runtime::{
    CancellationToken, DiscoveryError, DiscoveryOptions, ExecutionOutcome, LaunchSession,
    RuntimeError, SignalKind, Supervisor, discover_harness,
};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

const DEFAULT_MODEL_ID: &str = "qwen3.6";

pub(crate) async fn run(
    cli: &Cli,
    interactive: bool,
    bridge_diagnostics: &mut Vec<BridgeDiagnostic>,
) -> Result<i32, CliError> {
    let working_directory = command_working_directory(cli)?;
    let config = if let Some(arguments) = credential_arguments(cli) {
        Some(
            commands::credentials::resolve_or_onboard(
                arguments.provider_base_url.clone(),
                interactive,
            )
            .await?,
        )
    } else {
        None
    };
    if let Some(working_directory) = working_directory.as_deref()
        && let Some(result) =
            run_simple_harness(cli, config.as_ref(), working_directory, bridge_diagnostics).await
    {
        return result;
    }
    match &cli.command {
        Command::Doctor(arguments) => commands::doctor::run(arguments).await.map_err(Into::into),
        Command::Auth { command } => {
            commands::credentials::run(command, interactive).await?;
            Ok(0)
        }
        Command::Config(arguments) => {
            commands::configuration::run(arguments, interactive).await?;
            Ok(0)
        }
        Command::Update => {
            commands::update::run_manual().await?;
            Ok(0)
        }
        Command::Uninstall(arguments) => {
            commands::uninstall::run(arguments, interactive)?;
            Ok(0)
        }
        Command::Telemetry { command } => {
            commands::telemetry::run(*command)?;
            Ok(0)
        }
        Command::RecordInstallation(arguments) => {
            commands::uninstall::record_installation(arguments)?;
            Ok(0)
        }
        Command::Claude(_)
        | Command::Codex(_)
        | Command::OpenCode(_)
        | Command::Hermes(_)
        | Command::Pi(_)
        | Command::Prime(_)
        | Command::DeepSeek(_)
        | Command::OpenClaw(_)
        | Command::Cline(_)
        | Command::Qwen(_)
        | Command::Kimi(_)
        | Command::Aider(_)
        | Command::Goose(_)
        | Command::Fx(_) => unreachable!("simple harness commands are dispatched first"),
    }
}

async fn run_simple_harness(
    cli: &Cli,
    config: Option<&commands::credentials::ResolvedLaunchConfig>,
    working_directory: &Path,
    bridge_diagnostics: &mut Vec<BridgeDiagnostic>,
) -> Option<Result<i32, CliError>> {
    let (kind, arguments, adapter): (HarnessKind, &HarnessRunArgs, &dyn HarnessAdapter) = match &cli
        .command
    {
        Command::Claude(arguments) => (HarnessKind::ClaudeCode, &arguments.run, &ClaudeCodeAdapter),
        Command::Codex(arguments) => (HarnessKind::Codex, &arguments.run, &CodexAdapter),
        Command::OpenCode(arguments) => (HarnessKind::OpenCode, &arguments.run, &OpenCodeAdapter),
        Command::Hermes(arguments) => (HarnessKind::Hermes, &arguments.run, &HermesAdapter),
        Command::Pi(arguments) => (HarnessKind::Pi, &arguments.run, &PiAdapter),
        Command::Prime(arguments) => (HarnessKind::PrimeAgent, &arguments.run, &PrimeAgentAdapter),
        Command::DeepSeek(arguments) => (
            HarnessKind::DeepSeekHarness,
            &arguments.run,
            &DeepSeekHarnessAdapter,
        ),
        Command::OpenClaw(arguments) => (HarnessKind::OpenClaw, &arguments.run, &OpenClawAdapter),
        Command::Cline(arguments) => (HarnessKind::Cline, &arguments.run, &ClineAdapter),
        Command::Qwen(arguments) => (HarnessKind::QwenCode, &arguments.run, &QwenCodeAdapter),
        Command::Kimi(arguments) => (HarnessKind::KimiCode, &arguments.run, &KimiCodeAdapter),
        Command::Aider(arguments) => (HarnessKind::Aider, &arguments.run, &AiderAdapter),
        Command::Goose(arguments) => (HarnessKind::Goose, &arguments.run, &GooseAdapter),
        Command::Fx(arguments) => (HarnessKind::Fx, &arguments.run, &FxAdapter),
        _ => return None,
    };
    Some(
        run_harness(
            kind,
            arguments,
            adapter,
            direct_chat_gateway_disabled(cli),
            config,
            working_directory,
            bridge_diagnostics,
        )
        .await,
    )
}

async fn run_harness(
    kind: HarnessKind,
    arguments: &HarnessRunArgs,
    adapter: &dyn HarnessAdapter,
    disable_direct_chat_gateway: bool,
    config: Option<&commands::credentials::ResolvedLaunchConfig>,
    working_directory: &Path,
    bridge_diagnostics: &mut Vec<BridgeDiagnostic>,
) -> Result<i32, CliError> {
    let Some(discovery) = discover_or_install_harness(kind, arguments)? else {
        return Ok(0);
    };
    for warning in &discovery.warnings {
        eprintln!("warning: {warning}");
    }
    let working_directory = working_directory.to_string_lossy().into_owned();
    let launch_id = generate_launch_id()?;
    let launch_model = model_for_launch(kind, arguments);
    let build_plan = |model: &LaunchModel| -> Result<LaunchPlan, CliError> {
        let context = PlanContext {
            launch_id: launch_id.clone(),
            harness: discovery.harness.clone(),
            model: requested_model(&model.id, model.reasoning),
            working_directory: working_directory.clone(),
            user_arguments: arguments.arguments.clone(),
            web_search_policy: web_search_policy(arguments),
            observability_format: ObservabilityFormat::Human,
        };
        build_validated_plan(adapter, &context).map_err(CliError::InvalidPlan)
    };
    let plan = build_plan(&launch_model)?;
    if let Some(notice) = direct_chat_gateway_notice(disable_direct_chat_gateway, arguments.dry_run)
    {
        eprintln!("{notice}");
    }
    if arguments.dry_run {
        let normalized = serde_json::to_string_pretty(&plan).map_err(CliError::SerializePlan)?;
        println!("{normalized}");
        return Ok(0);
    }

    check_required_runtime(kind)?;
    let launch_config = required_config(config)?;
    let config = &launch_config.config;
    let session = launch_config.model_catalog.as_ref().map_or_else(
        || LaunchSession::new(config),
        |models| LaunchSession::with_model_catalog(config, models.clone()),
    );
    let cancellation = CancellationToken::new();
    let signal_task = install_signal_handlers(cancellation.clone());
    let supervisor = if disable_direct_chat_gateway {
        Supervisor::new().without_direct_chat_gateway()
    } else {
        Supervisor::new()
    };
    eprintln!("{}", format_launch_announcement(kind, &launch_model));
    let mut effective_launch_model = launch_model.clone();
    let result = supervisor
        .execute_in_session(&plan, &session, &cancellation)
        .await;
    let result = match result {
        Err(error) => {
            let fallback = fallback_codex_model(kind, &launch_model, &error);
            if let Some(fallback) = fallback {
                eprintln!(
                    "warning: Codex model '{}' is no longer available; using '{fallback}'.",
                    launch_model.id,
                    fallback = fallback.id
                );
                let fallback_plan = match build_plan(&fallback) {
                    Ok(plan) => plan,
                    Err(error) => {
                        signal_task.abort();
                        return Err(error);
                    }
                };
                eprintln!("{}", format_launch_announcement(kind, &fallback));
                effective_launch_model = fallback;
                supervisor
                    .execute_in_session(&fallback_plan, &session, &cancellation)
                    .await
            } else {
                Err(error)
            }
        }
        result => result,
    };
    signal_task.abort();
    let report = result?;
    usage_evidence::write_if_configured(&report).map_err(CliError::UsageEvidence)?;
    let usage_summary = usage_summary::render(&report);
    if let Some((exit_line, doctor_line)) =
        format_exit_bookend(kind, report.outcome, report.exit_code)
    {
        eprintln!("{exit_line}");
        eprintln!("{doctor_line}");
    }
    if let Some(selection) = successful_selection(kind, &effective_launch_model, &report)
        && let Ok(manager) = PersistenceManager::from_environment()
        && let Err(error) = manager.save_last_selection(kind, &selection.model, selection.reasoning)
    {
        eprintln!("warning: could not save the last {kind} model: {error}");
    }
    bridge_diagnostics.extend(report.bridge_diagnostics);
    if let Some(usage_summary) = usage_summary {
        eprintln!("{usage_summary}");
    }
    Ok(report.exit_code)
}

const fn web_search_policy(arguments: &HarnessRunArgs) -> WebSearchPolicy {
    if arguments.search.no_search {
        WebSearchPolicy::Disabled
    } else if arguments.search.force_search {
        WebSearchPolicy::Force
    } else {
        WebSearchPolicy::Auto
    }
}

fn command_working_directory(cli: &Cli) -> Result<Option<PathBuf>, CliError> {
    if harness_run_arguments(cli).is_some() || matches!(cli.command, Command::Doctor(_)) {
        return std::env::current_dir()
            .map(Some)
            .map_err(CliError::CurrentDirectory);
    }
    Ok(None)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LaunchModelSource {
    Explicit,
    Remembered,
    Default,
    Fallback,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LaunchModel {
    id: String,
    source: LaunchModelSource,
    reasoning: Option<ReasoningSelection>,
}

fn model_for_launch(kind: HarnessKind, arguments: &HarnessRunArgs) -> LaunchModel {
    let remembered = PersistenceManager::from_environment()
        .ok()
        .and_then(|manager| manager.last_selection(kind).ok())
        .flatten();
    choose_launch_model(arguments.model.as_deref(), remembered)
}

fn choose_launch_model(explicit: Option<&str>, remembered: Option<LastSelection>) -> LaunchModel {
    if let Some(model) = explicit {
        return LaunchModel {
            id: model.to_owned(),
            source: LaunchModelSource::Explicit,
            reasoning: None,
        };
    }
    if let Some(selection) = remembered {
        return LaunchModel {
            id: selection.model,
            source: LaunchModelSource::Remembered,
            reasoning: selection.reasoning,
        };
    }
    LaunchModel {
        id: DEFAULT_MODEL_ID.to_owned(),
        source: LaunchModelSource::Default,
        reasoning: None,
    }
}

fn successful_selection(
    kind: HarnessKind,
    launched: &LaunchModel,
    report: &nan_harness_runtime::ExecutionReport,
) -> Option<LastSelection> {
    if report.outcome != ExecutionOutcome::Succeeded {
        return None;
    }
    if kind == HarnessKind::Codex
        && let Some(model) = report.selected_model.as_deref()
        && (matches!(
            launched.source,
            LaunchModelSource::Explicit
                | LaunchModelSource::Remembered
                | LaunchModelSource::Fallback
        ) || model != launched.id
            || report.selected_reasoning != launched.reasoning)
    {
        return Some(LastSelection {
            model: model.to_owned(),
            reasoning: report.selected_reasoning,
        });
    }
    matches!(
        launched.source,
        LaunchModelSource::Explicit | LaunchModelSource::Fallback
    )
    .then(|| LastSelection {
        model: launched.id.clone(),
        reasoning: launched.reasoning,
    })
}

fn fallback_codex_model(
    kind: HarnessKind,
    selected: &LaunchModel,
    error: &RuntimeError,
) -> Option<LaunchModel> {
    if kind != HarnessKind::Codex || selected.source == LaunchModelSource::Explicit {
        return None;
    }
    let (unavailable, available) = error.unavailable_model()?;
    if unavailable != selected.id {
        return None;
    }
    let id = available
        .iter()
        .find(|model| model.as_str() == DEFAULT_MODEL_ID)
        .or_else(|| available.first())
        .filter(|model| model.as_str() != selected.id)
        .cloned()?;
    Some(LaunchModel {
        id,
        source: LaunchModelSource::Fallback,
        reasoning: None,
    })
}

fn format_launch_announcement(kind: HarnessKind, model: &LaunchModel) -> String {
    let qualifier = match model.source {
        LaunchModelSource::Explicit => None,
        LaunchModelSource::Remembered => {
            Some("(remembered from your last session; override with --model)")
        }
        LaunchModelSource::Default => Some("(default; override with --model)"),
        LaunchModelSource::Fallback => Some("(provider-selected fallback)"),
    };
    let reasoning = format_reasoning_state(model.reasoning);
    match qualifier {
        Some(qualifier) => format!(
            "Starting {kind} with model '{}' {qualifier}. Reasoning: {reasoning}.",
            model.id
        ),
        None => format!(
            "Starting {kind} with model '{}'. Reasoning: {reasoning}.",
            model.id
        ),
    }
}

fn format_reasoning_state(reasoning: Option<ReasoningSelection>) -> &'static str {
    match reasoning {
        None => "not specified",
        Some(ReasoningSelection::Auto) => "auto",
        Some(ReasoningSelection::Toggle(true)) => "enabled",
        Some(ReasoningSelection::Toggle(false)) => "disabled",
        Some(ReasoningSelection::Effort(ReasoningEffort::Low)) => "low",
        Some(ReasoningSelection::Effort(ReasoningEffort::Medium)) => "medium",
        Some(ReasoningSelection::Effort(ReasoningEffort::High)) => "high",
    }
}

fn format_exit_bookend(
    kind: HarnessKind,
    outcome: ExecutionOutcome,
    exit_code: i32,
) -> Option<(String, String)> {
    if exit_code == 0 || matches!(outcome, ExecutionOutcome::Cancelled(_)) {
        return None;
    }
    Some((
        format!("{kind} exited with code {exit_code}."),
        format!("If this looks like a setup problem, run `nan doctor {kind}`."),
    ))
}

fn discover_or_install_harness(
    kind: HarnessKind,
    arguments: &HarnessRunArgs,
) -> Result<Option<nan_harness_runtime::DiscoveryReport>, CliError> {
    let options = DiscoveryOptions {
        allow_unsupported: arguments.allow_unsupported,
        allow_untested: arguments.allow_untested,
    };
    match discover_harness(kind, arguments.executable.as_deref(), options) {
        Ok(report) => Ok(Some(report)),
        Err(DiscoveryError::ExecutableNotFound(_))
            if install_spec(kind).is_some() && arguments.executable.is_none() =>
        {
            if let Some(executable) = executable_from_known_locations(kind) {
                return discover_harness(kind, Some(&executable), options)
                    .map(Some)
                    .map_err(CliError::from);
            }
            if arguments.dry_run {
                eprintln!("{kind} was not found on PATH; dry-run does not install harnesses.");
                eprintln!("Run `nan doctor {kind}` after installing the official release.");
                return Ok(None);
            }
            match offer_install(kind)? {
                InstallDecision::NotInteractive => {
                    report_install_skipped(kind, "installation requires an interactive terminal");
                    Err(DiscoveryError::ExecutableNotFound(kind.binary_name().to_owned()).into())
                }
                InstallDecision::Declined => {
                    report_install_skipped(kind, "installation was declined");
                    Ok(None)
                }
                InstallDecision::Installed => {
                    let executable = executable_from_known_locations(kind);
                    match discover_harness(kind, executable.as_deref(), options) {
                        Ok(report) => Ok(Some(report)),
                        Err(error @ DiscoveryError::ExecutableNotFound(_)) => {
                            eprintln!(
                                "{kind} was installed, but its executable is not visible on PATH."
                            );
                            Err(error.into())
                        }
                        Err(error) => Err(error.into()),
                    }
                }
            }
        }
        Err(error) => Err(error.into()),
    }
}

fn report_install_skipped(kind: HarnessKind, reason: &str) {
    eprintln!("{kind} was not found; {reason}.");
    eprintln!(
        "Install the official release, or pass --executable /path/to/{}.",
        kind.binary_name()
    );
}

fn required_config(
    config: Option<&commands::credentials::ResolvedLaunchConfig>,
) -> Result<&commands::credentials::ResolvedLaunchConfig, CliError> {
    config.ok_or(CliError::CredentialInvariant)
}

fn generate_launch_id() -> Result<LaunchId, CliError> {
    let mut bytes = [0_u8; 12];
    getrandom::fill(&mut bytes).map_err(CliError::Random)?;
    let mut suffix = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(&mut suffix, "{byte:02x}");
    }
    LaunchId::new(format!("launch_{suffix}")).map_err(CliError::InvalidPlan)
}

fn requested_model(model: &str, reasoning_selection: Option<ReasoningSelection>) -> ResolvedModel {
    let bundled = nan_harness_core::known_coding_model(model).is_some();
    ResolvedModel {
        requested_id: model.to_owned(),
        resolved_id: model.to_owned(),
        reasoning_selection,
        availability: ModelAvailability::Discovered,
        profile_source: if bundled {
            ProfileSource::Bundled
        } else {
            ProfileSource::Generic
        },
        qualification: if bundled {
            QualificationStatus::Qualified
        } else {
            QualificationStatus::Unknown
        },
        warnings: Vec::new(),
    }
}

fn install_signal_handlers(cancellation: CancellationToken) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        #[cfg(unix)]
        {
            let Ok(mut interrupt) =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
            else {
                loop {
                    if tokio::signal::ctrl_c().await.is_err() {
                        break;
                    }
                    cancellation.cancel(SignalKind::Interrupt);
                }
                return;
            };
            let Ok(mut terminate) =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            else {
                loop {
                    if interrupt.recv().await.is_none() {
                        break;
                    }
                    cancellation.cancel(SignalKind::Interrupt);
                }
                return;
            };
            loop {
                tokio::select! {
                    value = interrupt.recv() => {
                        if value.is_some() {
                            cancellation.cancel(SignalKind::Interrupt);
                        } else {
                            break;
                        }
                    }
                    value = terminate.recv() => {
                        if value.is_some() {
                            cancellation.cancel(SignalKind::Terminate);
                        } else {
                            break;
                        }
                    }
                }
            }
        }
        #[cfg(not(unix))]
        loop {
            if tokio::signal::ctrl_c().await.is_err() {
                break;
            }
            cancellation.cancel(SignalKind::Interrupt);
        }
    })
}

pub(crate) fn harness_run_arguments(cli: &Cli) -> Option<(HarnessKind, &HarnessRunArgs)> {
    match &cli.command {
        Command::Claude(arguments) => Some((HarnessKind::ClaudeCode, &arguments.run)),
        Command::Codex(arguments) => Some((HarnessKind::Codex, &arguments.run)),
        Command::OpenCode(arguments) => Some((HarnessKind::OpenCode, &arguments.run)),
        Command::Hermes(arguments) => Some((HarnessKind::Hermes, &arguments.run)),
        Command::Pi(arguments) => Some((HarnessKind::Pi, &arguments.run)),
        Command::Prime(arguments) => Some((HarnessKind::PrimeAgent, &arguments.run)),
        Command::DeepSeek(arguments) => Some((HarnessKind::DeepSeekHarness, &arguments.run)),
        Command::OpenClaw(arguments) => Some((HarnessKind::OpenClaw, &arguments.run)),
        Command::Cline(arguments) => Some((HarnessKind::Cline, &arguments.run)),
        Command::Qwen(arguments) => Some((HarnessKind::QwenCode, &arguments.run)),
        Command::Kimi(arguments) => Some((HarnessKind::KimiCode, &arguments.run)),
        Command::Aider(arguments) => Some((HarnessKind::Aider, &arguments.run)),
        Command::Goose(arguments) => Some((HarnessKind::Goose, &arguments.run)),
        Command::Fx(arguments) => Some((HarnessKind::Fx, &arguments.run)),
        Command::Doctor(_)
        | Command::Auth { .. }
        | Command::Config(_)
        | Command::Update
        | Command::Uninstall(_)
        | Command::Telemetry { .. }
        | Command::RecordInstallation(_) => None,
    }
}

pub(crate) const fn direct_chat_gateway_disabled(cli: &Cli) -> bool {
    match &cli.command {
        Command::OpenCode(arguments)
        | Command::Hermes(arguments)
        | Command::Pi(arguments)
        | Command::Prime(arguments)
        | Command::DeepSeek(arguments)
        | Command::OpenClaw(arguments)
        | Command::Cline(arguments)
        | Command::Qwen(arguments)
        | Command::Kimi(arguments)
        | Command::Aider(arguments)
        | Command::Goose(arguments) => arguments.no_chat_gateway,
        Command::Claude(_)
        | Command::Codex(_)
        | Command::Fx(_)
        | Command::Doctor(_)
        | Command::Auth { .. }
        | Command::Config(_)
        | Command::Update
        | Command::Uninstall(_)
        | Command::Telemetry { .. }
        | Command::RecordInstallation(_) => false,
    }
}

const fn direct_chat_gateway_notice(disabled: bool, dry_run: bool) -> Option<&'static str> {
    if !disabled {
        None
    } else if dry_run {
        Some(
            "note: Chat Completions gateway would be disabled for this launch. The harness would receive the provider credential directly; usage accounting and gateway-dependent features would be unavailable.",
        )
    } else {
        Some(
            "warning: Chat Completions gateway disabled for this launch. The harness will receive the provider credential directly; usage accounting and gateway-dependent features are unavailable.",
        )
    }
}

fn credential_arguments(cli: &Cli) -> Option<&HarnessRunArgs> {
    harness_run_arguments(cli)
        .map(|(_, arguments)| arguments)
        .filter(|arguments| !arguments.dry_run)
}

#[cfg(test)]
mod tests {
    use super::{
        LaunchModel, LaunchModelSource, choose_launch_model, direct_chat_gateway_notice,
        format_exit_bookend, format_launch_announcement, format_reasoning_state, requested_model,
        successful_selection,
    };
    use crate::commands::persistence::LastSelection;
    use nan_harness_core::{
        HarnessKind, KNOWN_CODING_MODELS, ProfileSource, QualificationStatus, ReasoningEffort,
        ReasoningSelection,
    };
    use nan_harness_runtime::{ExecutionOutcome, ExecutionReport, SignalKind};

    fn execution_report(
        outcome: ExecutionOutcome,
        model: Option<&str>,
        reasoning: Option<ReasoningSelection>,
    ) -> ExecutionReport {
        ExecutionReport {
            outcome,
            exit_code: if outcome == ExecutionOutcome::Succeeded {
                0
            } else {
                1
            },
            temporary_root: None,
            selected_model: model.map(str::to_owned),
            selected_reasoning: reasoning,
            bridge_diagnostics: Vec::new(),
            provider_usage: None,
        }
    }

    #[test]
    fn model_selection_precedence_is_explicit_then_remembered_then_default() {
        let remembered = LastSelection {
            model: "remembered-model".to_owned(),
            reasoning: Some(ReasoningSelection::Toggle(true)),
        };
        assert_eq!(
            choose_launch_model(Some("explicit-model"), Some(remembered.clone())),
            LaunchModel {
                id: "explicit-model".to_owned(),
                source: LaunchModelSource::Explicit,
                reasoning: None,
            }
        );
        assert_eq!(
            choose_launch_model(None, Some(remembered)),
            LaunchModel {
                id: "remembered-model".to_owned(),
                source: LaunchModelSource::Remembered,
                reasoning: Some(ReasoningSelection::Toggle(true)),
            }
        );
        assert_eq!(
            choose_launch_model(None, None),
            LaunchModel {
                id: "qwen3.6".to_owned(),
                source: LaunchModelSource::Default,
                reasoning: None,
            }
        );
    }

    #[test]
    fn selections_are_remembered_only_after_eligible_successes() {
        let explicit = LaunchModel {
            id: "explicit-model".to_owned(),
            source: LaunchModelSource::Explicit,
            reasoning: Some(ReasoningSelection::Toggle(true)),
        };
        let fallback = LaunchModel {
            id: "fallback-model".to_owned(),
            source: LaunchModelSource::Fallback,
            reasoning: None,
        };
        let default = LaunchModel {
            id: "qwen3.6".to_owned(),
            source: LaunchModelSource::Default,
            reasoning: None,
        };

        assert_eq!(
            successful_selection(
                HarnessKind::Fx,
                &explicit,
                &execution_report(ExecutionOutcome::Succeeded, None, None),
            ),
            Some(LastSelection {
                model: "explicit-model".to_owned(),
                reasoning: Some(ReasoningSelection::Toggle(true)),
            })
        );
        assert_eq!(
            successful_selection(
                HarnessKind::ClaudeCode,
                &fallback,
                &execution_report(ExecutionOutcome::Succeeded, None, None),
            ),
            Some(LastSelection {
                model: "fallback-model".to_owned(),
                reasoning: None,
            })
        );
        assert_eq!(
            successful_selection(
                HarnessKind::Fx,
                &explicit,
                &execution_report(ExecutionOutcome::Failed, None, None),
            ),
            None
        );
        assert_eq!(
            successful_selection(
                HarnessKind::Fx,
                &explicit,
                &execution_report(
                    ExecutionOutcome::Cancelled(SignalKind::Interrupt),
                    None,
                    None,
                ),
            ),
            None
        );
        assert_eq!(
            successful_selection(
                HarnessKind::Fx,
                &default,
                &execution_report(ExecutionOutcome::Succeeded, None, None),
            ),
            None,
            "an implicit default must not be remembered"
        );
    }

    #[test]
    fn codex_remembers_the_observable_actual_selection() {
        let remembered = LaunchModel {
            id: "remembered-model".to_owned(),
            source: LaunchModelSource::Remembered,
            reasoning: Some(ReasoningSelection::Toggle(false)),
        };
        let actual_reasoning = Some(ReasoningSelection::Effort(ReasoningEffort::High));
        assert_eq!(
            successful_selection(
                HarnessKind::Codex,
                &remembered,
                &execution_report(
                    ExecutionOutcome::Succeeded,
                    Some("picker-selected-model"),
                    actual_reasoning,
                ),
            ),
            Some(LastSelection {
                model: "picker-selected-model".to_owned(),
                reasoning: actual_reasoning,
            })
        );
    }

    #[test]
    fn requested_model_stays_in_sync_with_the_shared_catalog() {
        for model in KNOWN_CODING_MODELS {
            let resolved = requested_model(model.id, None);

            assert_eq!(
                resolved.profile_source,
                ProfileSource::Bundled,
                "known model {} should use bundled metadata",
                model.id
            );
            assert_eq!(
                resolved.qualification,
                QualificationStatus::Qualified,
                "known model {} should be qualified",
                model.id
            );
        }
    }

    #[test]
    fn gateway_escape_hatch_explains_the_security_and_feature_tradeoff() {
        assert_eq!(direct_chat_gateway_notice(false, false), None);
        assert_eq!(
            direct_chat_gateway_notice(true, false),
            Some(
                "warning: Chat Completions gateway disabled for this launch. The harness will receive the provider credential directly; usage accounting and gateway-dependent features are unavailable."
            )
        );
        assert_eq!(
            direct_chat_gateway_notice(true, true),
            Some(
                "note: Chat Completions gateway would be disabled for this launch. The harness would receive the provider credential directly; usage accounting and gateway-dependent features would be unavailable."
            )
        );
    }

    #[test]
    fn requested_model_keeps_unknown_models_generic_and_unknown() {
        let resolved = requested_model("future-text-model", None);

        assert_eq!(resolved.profile_source, ProfileSource::Generic);
        assert_eq!(resolved.qualification, QualificationStatus::Unknown);
    }

    #[test]
    fn launch_announcement_describes_each_model_source() {
        let cases = [
            (
                LaunchModel {
                    id: "glm5.2".to_owned(),
                    source: LaunchModelSource::Explicit,
                    reasoning: Some(ReasoningSelection::Toggle(false)),
                },
                "Starting codex with model 'glm5.2'. Reasoning: disabled.",
            ),
            (
                LaunchModel {
                    id: "glm5.2".to_owned(),
                    source: LaunchModelSource::Remembered,
                    reasoning: Some(ReasoningSelection::Effort(ReasoningEffort::High)),
                },
                "Starting codex with model 'glm5.2' (remembered from your last session; override with --model). Reasoning: high.",
            ),
            (
                LaunchModel {
                    id: "qwen3.6".to_owned(),
                    source: LaunchModelSource::Default,
                    reasoning: None,
                },
                "Starting codex with model 'qwen3.6' (default; override with --model). Reasoning: not specified.",
            ),
            (
                LaunchModel {
                    id: "glm5.2-flash".to_owned(),
                    source: LaunchModelSource::Fallback,
                    reasoning: None,
                },
                "Starting codex with model 'glm5.2-flash' (provider-selected fallback). Reasoning: not specified.",
            ),
        ];

        for (model, expected) in cases {
            assert_eq!(
                format_launch_announcement(HarnessKind::Codex, &model),
                expected
            );
        }
    }

    #[test]
    fn reasoning_state_has_stable_text_for_every_selection() {
        let cases = [
            (None, "not specified"),
            (Some(ReasoningSelection::Auto), "auto"),
            (Some(ReasoningSelection::Toggle(true)), "enabled"),
            (Some(ReasoningSelection::Toggle(false)), "disabled"),
            (
                Some(ReasoningSelection::Effort(ReasoningEffort::Low)),
                "low",
            ),
            (
                Some(ReasoningSelection::Effort(ReasoningEffort::Medium)),
                "medium",
            ),
            (
                Some(ReasoningSelection::Effort(ReasoningEffort::High)),
                "high",
            ),
        ];

        for (selection, expected) in cases {
            assert_eq!(format_reasoning_state(selection), expected);
        }
    }

    #[test]
    fn non_zero_exit_bookend_explains_failures_only() {
        assert_eq!(
            format_exit_bookend(HarnessKind::Codex, ExecutionOutcome::Failed, 7),
            Some((
                "codex exited with code 7.".to_owned(),
                "If this looks like a setup problem, run `nan doctor codex`.".to_owned(),
            ))
        );
        assert_eq!(
            format_exit_bookend(HarnessKind::Codex, ExecutionOutcome::Succeeded, 0),
            None
        );
        assert_eq!(
            format_exit_bookend(
                HarnessKind::Codex,
                ExecutionOutcome::Cancelled(SignalKind::Interrupt),
                130
            ),
            None
        );
        assert_eq!(
            format_exit_bookend(
                HarnessKind::Codex,
                ExecutionOutcome::Cancelled(SignalKind::Terminate),
                143
            ),
            None
        );
    }
}
