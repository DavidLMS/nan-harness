use crate::app::{Cli, Command, HarnessRunArgs};
use crate::commands;
use crate::commands::install::{
    InstallDecision, check_required_runtime, executable_from_known_locations, install_spec,
    offer_install,
};
use crate::commands::persistence::PersistenceManager;
use crate::error::CliError;
use nan_harness_adapters::{
    AiderAdapter, ClaudeCodeAdapter, ClineAdapter, CodexAdapter, DeepSeekHarnessAdapter, FxAdapter,
    GooseAdapter, HermesAdapter, KimiCodeAdapter, OpenClawAdapter, OpenCodeAdapter, PiAdapter,
    PrimeAgentAdapter, QwenCodeAdapter,
};
use nan_harness_core::launch_plan::{LaunchId, ObservabilityFormat};
use nan_harness_core::model::{
    ModelAvailability, ProfileSource, QualificationStatus, ReasoningSelection,
};
use nan_harness_core::{
    HarnessAdapter, HarnessKind, LaunchPlan, PlanContext, ResolvedModel, build_validated_plan,
};
use nan_harness_runtime::{
    CancellationToken, DiscoveryError, DiscoveryOptions, ResolvedConfig, RuntimeError, SignalKind,
    Supervisor, discover_harness,
};
use std::fmt::Write as _;

const DEFAULT_MODEL_ID: &str = "qwen3.6";

pub(crate) async fn run(cli: &Cli, interactive: bool) -> Result<i32, CliError> {
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
    if let Some(result) = run_simple_harness(cli, config.as_ref()).await {
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
    config: Option<&ResolvedConfig>,
) -> Option<Result<i32, CliError>> {
    let (kind, arguments, adapter): (HarnessKind, &HarnessRunArgs, &dyn HarnessAdapter) =
        match &cli.command {
            Command::Claude(arguments) => (HarnessKind::ClaudeCode, arguments, &ClaudeCodeAdapter),
            Command::Codex(arguments) => (HarnessKind::Codex, arguments, &CodexAdapter),
            Command::OpenCode(arguments) => (HarnessKind::OpenCode, arguments, &OpenCodeAdapter),
            Command::Hermes(arguments) => (HarnessKind::Hermes, arguments, &HermesAdapter),
            Command::Pi(arguments) => (HarnessKind::Pi, arguments, &PiAdapter),
            Command::Prime(arguments) => (HarnessKind::PrimeAgent, arguments, &PrimeAgentAdapter),
            Command::DeepSeek(arguments) => (
                HarnessKind::DeepSeekHarness,
                arguments,
                &DeepSeekHarnessAdapter,
            ),
            Command::OpenClaw(arguments) => (HarnessKind::OpenClaw, arguments, &OpenClawAdapter),
            Command::Cline(arguments) => (HarnessKind::Cline, arguments, &ClineAdapter),
            Command::Qwen(arguments) => (HarnessKind::QwenCode, arguments, &QwenCodeAdapter),
            Command::Kimi(arguments) => (HarnessKind::KimiCode, arguments, &KimiCodeAdapter),
            Command::Aider(arguments) => (HarnessKind::Aider, arguments, &AiderAdapter),
            Command::Goose(arguments) => (HarnessKind::Goose, arguments, &GooseAdapter),
            Command::Fx(arguments) => (HarnessKind::Fx, arguments, &FxAdapter),
            _ => return None,
        };
    Some(run_harness(kind, arguments, adapter, config).await)
}

async fn run_harness(
    kind: HarnessKind,
    arguments: &HarnessRunArgs,
    adapter: &dyn HarnessAdapter,
    config: Option<&ResolvedConfig>,
) -> Result<i32, CliError> {
    let Some(discovery) = discover_or_install_harness(kind, arguments)? else {
        return Ok(0);
    };
    for warning in &discovery.warnings {
        eprintln!("warning: {warning}");
    }
    let working_directory = std::env::current_dir().map_err(CliError::CurrentDirectory)?;
    let working_directory = working_directory.to_string_lossy().into_owned();
    let launch_id = generate_launch_id()?;
    let launch_model = model_for_launch(kind, arguments);
    let build_plan = |model_id: &str| -> Result<LaunchPlan, CliError> {
        let reasoning = (model_id == launch_model.id)
            .then_some(launch_model.reasoning)
            .flatten();
        let context = PlanContext {
            launch_id: launch_id.clone(),
            harness: discovery.harness.clone(),
            model: requested_model(model_id, reasoning),
            working_directory: working_directory.clone(),
            user_arguments: arguments.arguments.clone(),
            observability_format: ObservabilityFormat::Human,
        };
        build_validated_plan(adapter, &context).map_err(CliError::InvalidPlan)
    };
    let plan = build_plan(&launch_model.id)?;
    if arguments.dry_run {
        let normalized = serde_json::to_string_pretty(&plan).map_err(CliError::SerializePlan)?;
        println!("{normalized}");
        return Ok(0);
    }

    check_required_runtime(kind)?;
    let config = required_config(config)?;
    let cancellation = CancellationToken::new();
    let signal_task = install_signal_handlers(cancellation.clone());
    let supervisor = Supervisor::new();
    let result = supervisor.execute(&plan, config, &cancellation).await;
    let result = match result {
        Err(error) => {
            let fallback = fallback_codex_model(kind, &launch_model, &error);
            if let Some(fallback) = fallback {
                eprintln!(
                    "warning: Codex model '{}' is no longer available; using '{fallback}'.",
                    launch_model.id
                );
                let fallback_plan = match build_plan(&fallback) {
                    Ok(plan) => plan,
                    Err(error) => {
                        signal_task.abort();
                        return Err(error);
                    }
                };
                supervisor
                    .execute(&fallback_plan, config, &cancellation)
                    .await
            } else {
                Err(error)
            }
        }
        result => result,
    };
    signal_task.abort();
    let report = result?;
    if kind == HarnessKind::Codex
        && let Some(model) = report.selected_model.as_deref()
        && let Ok(manager) = PersistenceManager::from_environment()
        && let Err(error) = manager.save_last_codex_selection(model, report.selected_reasoning)
    {
        eprintln!("warning: could not save the last Codex model: {error}");
    }
    Ok(report.exit_code)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LaunchModelSource {
    Explicit,
    Remembered,
    Default,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LaunchModel {
    id: String,
    source: LaunchModelSource,
    reasoning: Option<ReasoningSelection>,
}

fn model_for_launch(kind: HarnessKind, arguments: &HarnessRunArgs) -> LaunchModel {
    if let Some(model) = &arguments.model {
        return LaunchModel {
            id: model.clone(),
            source: LaunchModelSource::Explicit,
            reasoning: None,
        };
    }
    if kind == HarnessKind::Codex
        && let Ok(manager) = PersistenceManager::from_environment()
        && let Ok(Some(selection)) = manager.last_codex_selection()
    {
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

fn fallback_codex_model(
    kind: HarnessKind,
    selected: &LaunchModel,
    error: &RuntimeError,
) -> Option<String> {
    if kind != HarnessKind::Codex || selected.source == LaunchModelSource::Explicit {
        return None;
    }
    let (unavailable, available) = error.unavailable_model()?;
    if unavailable != selected.id {
        return None;
    }
    available
        .iter()
        .find(|model| model.as_str() == DEFAULT_MODEL_ID)
        .or_else(|| available.first())
        .filter(|model| model.as_str() != selected.id)
        .cloned()
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

fn required_config(config: Option<&ResolvedConfig>) -> Result<&ResolvedConfig, CliError> {
    config.ok_or(CliError::CredentialInvariant)
}

fn generate_launch_id() -> Result<LaunchId, CliError> {
    let mut bytes = [0_u8; 12];
    getrandom::fill(&mut bytes).map_err(CliError::Random)?;
    let mut suffix = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut suffix, "{byte:02x}").expect("writing to a String cannot fail");
    }
    LaunchId::new(format!("launch_{suffix}")).map_err(CliError::InvalidPlan)
}

fn requested_model(model: &str, reasoning_selection: Option<ReasoningSelection>) -> ResolvedModel {
    let bundled = matches!(
        model,
        "qwen3.6" | "deepseek-v4-flash" | "mimo-v2.5" | "gemma4"
    );
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
            let Ok(mut terminate) =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            else {
                if tokio::signal::ctrl_c().await.is_ok() {
                    cancellation.cancel(SignalKind::Interrupt);
                }
                return;
            };
            tokio::select! {
                result = tokio::signal::ctrl_c() => {
                    if result.is_ok() {
                        cancellation.cancel(SignalKind::Interrupt);
                    }
                }
                value = terminate.recv() => {
                    if value.is_some() {
                        cancellation.cancel(SignalKind::Terminate);
                    }
                }
            }
        }
        #[cfg(not(unix))]
        if tokio::signal::ctrl_c().await.is_ok() {
            cancellation.cancel(SignalKind::Interrupt);
        }
    })
}

pub(crate) fn harness_run_arguments(cli: &Cli) -> Option<(HarnessKind, &HarnessRunArgs)> {
    match &cli.command {
        Command::Claude(arguments) => Some((HarnessKind::ClaudeCode, arguments)),
        Command::Codex(arguments) => Some((HarnessKind::Codex, arguments)),
        Command::OpenCode(arguments) => Some((HarnessKind::OpenCode, arguments)),
        Command::Hermes(arguments) => Some((HarnessKind::Hermes, arguments)),
        Command::Pi(arguments) => Some((HarnessKind::Pi, arguments)),
        Command::Prime(arguments) => Some((HarnessKind::PrimeAgent, arguments)),
        Command::DeepSeek(arguments) => Some((HarnessKind::DeepSeekHarness, arguments)),
        Command::OpenClaw(arguments) => Some((HarnessKind::OpenClaw, arguments)),
        Command::Cline(arguments) => Some((HarnessKind::Cline, arguments)),
        Command::Qwen(arguments) => Some((HarnessKind::QwenCode, arguments)),
        Command::Kimi(arguments) => Some((HarnessKind::KimiCode, arguments)),
        Command::Aider(arguments) => Some((HarnessKind::Aider, arguments)),
        Command::Goose(arguments) => Some((HarnessKind::Goose, arguments)),
        Command::Fx(arguments) => Some((HarnessKind::Fx, arguments)),
        Command::Doctor(_)
        | Command::Auth { .. }
        | Command::Config(_)
        | Command::Update
        | Command::Uninstall(_)
        | Command::Telemetry { .. }
        | Command::RecordInstallation(_) => None,
    }
}

fn credential_arguments(cli: &Cli) -> Option<&HarnessRunArgs> {
    harness_run_arguments(cli)
        .map(|(_, arguments)| arguments)
        .filter(|arguments| !arguments.dry_run)
}
