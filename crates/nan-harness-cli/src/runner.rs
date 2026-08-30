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
    CodingModelProfile, DetectedHarness, HarnessAdapter, HarnessKind, LaunchPlan, PlanContext,
    PlanError, ResolvedModel, WebSearchPolicy, build_validated_plan, coding_model_profile,
    is_valid_provider_model_id, known_coding_model,
};
use nan_harness_runtime::BridgeDiagnostic;
use nan_harness_runtime::{
    CancellationToken, DiscoveryError, DiscoveryOptions, ExecutionOutcome, LaunchSession,
    RuntimeError, SignalKind, Supervisor, discover_harness,
};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

const DEFAULT_MODEL_ID: &str = "qwen3.6";

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
    let working_directory = command_working_directory(cli)?;
    let config = if let Some(arguments) = credential_arguments(cli) {
        Some(
            commands::credentials::resolve_or_onboard(
                arguments.provider_base_url.clone(),
                interactive,
            )
            .await
            .map_err(CliError::from)?,
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
        Command::Doctor(arguments) => commands::doctor::run(arguments)
            .await
            .map_err(CliError::from)
            .map_err(Into::into),
        Command::Auth { command } => {
            commands::credentials::run(command, interactive)
                .await
                .map_err(CliError::from)?;
            Ok(0)
        }
        Command::Config(arguments) => {
            commands::configuration::run(arguments, interactive)
                .await
                .map_err(CliError::from)?;
            Ok(0)
        }
        Command::Update => {
            commands::update::run_manual()
                .await
                .map_err(CliError::from)?;
            Ok(0)
        }
        Command::Uninstall(arguments) => {
            commands::uninstall::run(arguments, interactive).map_err(CliError::from)?;
            Ok(0)
        }
        Command::Telemetry { command } => {
            commands::telemetry::run(*command).map_err(CliError::from)?;
            Ok(0)
        }
        Command::RecordInstallation(arguments) => {
            commands::uninstall::record_installation(arguments).map_err(CliError::from)?;
            Ok(0)
        }
        Command::Completions { .. } => {
            unreachable!("completion generation returns before runner dispatch")
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
) -> Option<Result<i32, RunError>> {
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
) -> Result<i32, RunError> {
    let Some(discovery) = discover_or_install_harness(kind, arguments)? else {
        return Ok(0);
    };
    for warning in &discovery.warnings {
        eprintln!("warning: {warning}");
    }
    let result = async {
        let working_directory = working_directory.to_string_lossy().into_owned();
        let launch_id = generate_launch_id()?;
        let mut launch_model = model_for_launch(kind, arguments);
        let build_plan = |model: ResolvedModel| -> Result<LaunchPlan, CliError> {
            let context = PlanContext {
                launch_id: launch_id.clone(),
                harness: discovery.harness.clone(),
                model,
                working_directory: working_directory.clone(),
                user_arguments: arguments.arguments.clone(),
                web_search_policy: web_search_policy(arguments),
                observability_format: ObservabilityFormat::Human,
            };
            build_validated_plan(adapter, &context).map_err(CliError::InvalidPlan)
        };
        if let Some(notice) =
            direct_chat_gateway_notice(disable_direct_chat_gateway, arguments.dry_run)
        {
            eprintln!("{notice}");
        }
        if arguments.dry_run {
            let plan = build_plan(offline_requested_model(&launch_model)?)?;
            let normalized =
                serde_json::to_string_pretty(&plan).map_err(CliError::SerializePlan)?;
            println!("{normalized}");
            return Ok(0);
        }

        check_required_runtime(kind)?;
        let launch_config = required_config(config)?;
        let (session, resolved_model) =
            prepare_launch_session(kind, &mut launch_model, launch_config).await?;
        let plan = build_plan(resolved_model)?;
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
                let fallback = if should_attempt_fallback(&launch_model, &error) {
                    match session.model_catalog().await {
                        Ok(models) => fallback_model(&launch_model, &error, models),
                        Err(_) => None,
                    }
                } else {
                    None
                };
                if let Some(fallback) = fallback {
                    eprintln!(
                        "warning: model '{}' is no longer available for this credential; using '{fallback}'.",
                        launch_model.id,
                        fallback = fallback.id
                    );
                    let fallback_plan =
                        match offline_requested_model(&fallback).and_then(&build_plan) {
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
        finish_harness_run(kind, &effective_launch_model, report, bridge_diagnostics)
    }
    .await;
    result.map_err(|error| RunError::after_discovery(error, discovery.harness))
}

async fn prepare_launch_session<'a>(
    kind: HarnessKind,
    launch_model: &mut LaunchModel,
    launch_config: &'a commands::credentials::ResolvedLaunchConfig,
) -> Result<(LaunchSession<'a>, ResolvedModel), CliError> {
    let config = &launch_config.config;
    let initial_session = launch_config.model_catalog.as_ref().map_or_else(
        || LaunchSession::new(config),
        |models| LaunchSession::with_model_catalog(config, models.clone()),
    );
    if launch_model.source != LaunchModelSource::Explicit {
        return Ok((initial_session, offline_requested_model(launch_model)?));
    }

    let _ = valid_model_profile(&launch_model.id)?;
    let resolution =
        resolve_explicit_model(kind, launch_model, initial_session.model_catalog().await?)?;
    if let Some(warning) = resolution.warning {
        eprintln!("{warning}");
    }
    if resolution.undiscovered {
        launch_model.source = LaunchModelSource::ExplicitUndiscovered;
    }
    Ok((
        LaunchSession::with_model_catalog(config, resolution.catalog),
        resolution.model,
    ))
}

fn finish_harness_run(
    kind: HarnessKind,
    effective_launch_model: &LaunchModel,
    report: nan_harness_runtime::ExecutionReport,
    bridge_diagnostics: &mut Vec<BridgeDiagnostic>,
) -> Result<i32, CliError> {
    usage_evidence::write_if_configured(&report).map_err(CliError::UsageEvidence)?;
    let usage_summary = usage_summary::render(&report);
    if let Some((exit_line, doctor_line)) =
        format_exit_bookend(kind, report.outcome, report.exit_code)
    {
        eprintln!("{exit_line}");
        eprintln!("{doctor_line}");
    }
    if let Some(selection) = successful_selection(kind, effective_launch_model, &report)
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
    ExplicitUndiscovered,
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
                | LaunchModelSource::ExplicitUndiscovered
                | LaunchModelSource::Remembered
                | LaunchModelSource::Fallback
        ) || model != launched.id)
    {
        return Some(LastSelection {
            model: model.to_owned(),
            reasoning: report.selected_reasoning,
        });
    }
    matches!(
        launched.source,
        LaunchModelSource::Explicit
            | LaunchModelSource::ExplicitUndiscovered
            | LaunchModelSource::Fallback
    )
    .then(|| LastSelection {
        model: launched.id.clone(),
        reasoning: launched.reasoning,
    })
}

fn fallback_model(
    selected: &LaunchModel,
    error: &RuntimeError,
    models: &[CodingModelProfile],
) -> Option<LaunchModel> {
    if !matches!(
        selected.source,
        LaunchModelSource::Remembered | LaunchModelSource::Default
    ) {
        return None;
    }
    let (unavailable, available) = error.unavailable_model()?;
    if unavailable != selected.id {
        return None;
    }
    let id = models
        .iter()
        .filter(|model| model.id != selected.id && available.contains(&model.id))
        .find(|model| model.id == DEFAULT_MODEL_ID && known_coding_model(&model.id).is_some())
        .or_else(|| {
            models.iter().find(|model| {
                model.id != selected.id
                    && available.contains(&model.id)
                    && known_coding_model(&model.id).is_some()
            })
        })?
        .id
        .clone();
    Some(LaunchModel {
        id,
        source: LaunchModelSource::Fallback,
        reasoning: None,
    })
}

fn should_attempt_fallback(selected: &LaunchModel, error: &RuntimeError) -> bool {
    matches!(
        selected.source,
        LaunchModelSource::Remembered | LaunchModelSource::Default
    ) && error
        .unavailable_model()
        .is_some_and(|(unavailable, _)| unavailable == selected.id)
}

fn format_launch_announcement(kind: HarnessKind, model: &LaunchModel) -> String {
    let qualifier = match model.source {
        LaunchModelSource::Explicit | LaunchModelSource::ExplicitUndiscovered => None,
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

#[derive(Debug)]
struct ExplicitModelResolution {
    model: ResolvedModel,
    catalog: Vec<CodingModelProfile>,
    warning: Option<String>,
    undiscovered: bool,
}

fn offline_requested_model(model: &LaunchModel) -> Result<ResolvedModel, CliError> {
    let profile = valid_model_profile(&model.id)?;
    let warnings = (profile.source == ProfileSource::Generic)
        .then(|| {
            format!(
                "model '{}' has no bundled capability profile; using conservative defaults.",
                model.id
            )
        })
        .into_iter()
        .collect();
    Ok(resolved_model(
        model,
        &profile,
        ModelAvailability::Discovered,
        warnings,
    ))
}

fn resolve_explicit_model(
    _kind: HarnessKind,
    model: &LaunchModel,
    discovered: &[CodingModelProfile],
) -> Result<ExplicitModelResolution, CliError> {
    let fallback_profile = valid_model_profile(&model.id)?;
    let live_profile = discovered.iter().find(|profile| profile.id == model.id);
    let profile = live_profile
        .cloned()
        .unwrap_or_else(|| fallback_profile.clone());
    let undiscovered = live_profile.is_none();
    let generic = known_coding_model(&model.id).is_none();
    let available = discovered
        .iter()
        .map(|profile| profile.id.clone())
        .collect::<Vec<_>>();
    let warning = explicit_model_warning(&model.id, generic, undiscovered, &available);
    let warnings = warning
        .as_deref()
        .and_then(|value| value.strip_prefix("warning: "))
        .map(str::to_owned)
        .into_iter()
        .collect();
    let mut catalog = discovered.to_vec();
    if undiscovered {
        catalog.push(fallback_profile);
    }
    Ok(ExplicitModelResolution {
        model: resolved_model(
            model,
            &profile,
            if undiscovered {
                ModelAvailability::ExplicitUndiscovered
            } else {
                ModelAvailability::Discovered
            },
            warnings,
        ),
        catalog,
        warning,
        undiscovered,
    })
}

fn valid_model_profile(model: &str) -> Result<CodingModelProfile, CliError> {
    if !is_valid_provider_model_id(model) {
        return Err(invalid_model_error());
    }
    coding_model_profile(model).ok_or_else(invalid_model_error)
}

fn invalid_model_error() -> CliError {
    CliError::InvalidPlan(PlanError::InvalidField {
        field: "model",
        message: "model ID is invalid".to_owned(),
    })
}

fn resolved_model(
    model: &LaunchModel,
    profile: &CodingModelProfile,
    availability: ModelAvailability,
    warnings: Vec<String>,
) -> ResolvedModel {
    ResolvedModel {
        requested_id: model.id.clone(),
        resolved_id: model.id.clone(),
        reasoning_selection: model.reasoning,
        availability,
        profile_source: profile.source,
        qualification: if profile.source == ProfileSource::Bundled {
            QualificationStatus::Qualified
        } else {
            QualificationStatus::Unknown
        },
        warnings,
    }
}

fn explicit_model_warning(
    model: &str,
    generic: bool,
    undiscovered: bool,
    available: &[String],
) -> Option<String> {
    let mut warning = match (generic, undiscovered) {
        (true, false) => format!(
            "warning: model '{model}' has no bundled capability profile; using conservative defaults."
        ),
        (false, true) => format!(
            "warning: model '{model}' was not returned by live discovery for this credential; attempting it because you selected it explicitly."
        ),
        (true, true) => format!(
            "warning: model '{model}' was not returned by live discovery and has no bundled capability profile; attempting it with conservative defaults because you selected it explicitly."
        ),
        (false, false) => return None,
    };
    if undiscovered && let Some(suggestion) = near_model_match(model, available) {
        let _ = write!(warning, " Did you mean '{suggestion}'?");
    }
    Some(warning)
}

pub(crate) fn near_model_match(requested: &str, available: &[String]) -> Option<String> {
    let requested = normalize_model_id(requested);
    if requested.is_empty() {
        return None;
    }
    let mut best: Option<(usize, &str)> = None;
    let mut tied = false;
    for candidate in available {
        let normalized = normalize_model_id(candidate);
        if normalized.is_empty() {
            continue;
        }
        let distance = edit_distance(requested.as_bytes(), normalized.as_bytes());
        match best {
            None => {
                best = Some((distance, candidate));
                tied = false;
            }
            Some((best_distance, _)) if distance < best_distance => {
                best = Some((distance, candidate));
                tied = false;
            }
            Some((best_distance, _)) if distance == best_distance => tied = true,
            Some(_) => {}
        }
    }
    let (distance, candidate) = best?;
    (!tied && distance.saturating_mul(4) <= requested.len()).then(|| candidate.to_owned())
}

fn normalize_model_id(value: &str) -> String {
    value
        .bytes()
        .filter(u8::is_ascii_alphanumeric)
        .map(|byte| byte.to_ascii_lowercase())
        .map(char::from)
        .collect()
}

fn edit_distance(left: &[u8], right: &[u8]) -> usize {
    let mut previous = (0..=right.len()).collect::<Vec<_>>();
    let mut current = vec![0; right.len() + 1];
    for (left_index, left_byte) in left.iter().enumerate() {
        current[0] = left_index + 1;
        for (right_index, right_byte) in right.iter().enumerate() {
            let substitution = previous[right_index] + usize::from(left_byte != right_byte);
            current[right_index + 1] = (current[right_index] + 1)
                .min(previous[right_index + 1] + 1)
                .min(substitution);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[right.len()]
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
        | Command::Completions { .. }
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
        | Command::Completions { .. }
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
        LaunchModel, LaunchModelSource, choose_launch_model, credential_arguments,
        direct_chat_gateway_notice, explicit_model_warning, fallback_model, format_exit_bookend,
        format_launch_announcement, format_reasoning_state, near_model_match,
        offline_requested_model, resolve_explicit_model, successful_selection,
    };
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
        assert_eq!(
            successful_selection(
                HarnessKind::Codex,
                &default,
                &execution_report(
                    ExecutionOutcome::Succeeded,
                    Some("qwen3.6"),
                    Some(ReasoningSelection::Toggle(true)),
                ),
            ),
            None,
            "observing the implicit default must not make it persistent"
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
            let resolved = offline_requested_model(&LaunchModel {
                id: model.id.to_owned(),
                source: LaunchModelSource::Default,
                reasoning: None,
            })
            .expect("known model should resolve");

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
        let resolved = offline_requested_model(&LaunchModel {
            id: "future-text-model".to_owned(),
            source: LaunchModelSource::Explicit,
            reasoning: None,
        })
        .expect("valid future model should resolve offline");

        assert_eq!(resolved.profile_source, ProfileSource::Generic);
        assert_eq!(resolved.qualification, QualificationStatus::Unknown);
        assert_eq!(resolved.warnings.len(), 1);
    }

    #[test]
    fn explicit_generic_dry_run_is_offline_and_keeps_a_structured_warning() {
        let cli = Cli::try_parse_checked_from([
            "nan",
            "opencode",
            "--dry-run",
            "--model",
            "future-model",
        ])
        .expect("dry-run command should parse");
        assert!(credential_arguments(&cli).is_none());
        let resolved = offline_requested_model(&LaunchModel {
            id: "future-model".to_owned(),
            source: LaunchModelSource::Explicit,
            reasoning: None,
        })
        .expect("generic model should resolve without discovery");
        assert_eq!(
            resolved.warnings,
            vec![
                "model 'future-model' has no bundled capability profile; using conservative defaults."
            ]
        );
    }

    #[test]
    fn explicit_model_resolution_uses_live_bundled_and_generic_profiles() {
        let qwen = coding_model_profile("qwen3.6").expect("bundled profile should exist");
        let future = coding_model_profile("future-model").expect("generic profile should exist");
        let discovered = vec![qwen, future];

        let live = resolve_explicit_model(
            HarnessKind::Codex,
            &LaunchModel {
                id: "qwen3.6".to_owned(),
                source: LaunchModelSource::Explicit,
                reasoning: None,
            },
            &discovered,
        )
        .expect("discovered explicit model should resolve");
        assert_eq!(live.model.availability, ModelAvailability::Discovered);
        assert_eq!(live.model.profile_source, ProfileSource::Bundled);
        assert_eq!(live.warning, None);
        assert_eq!(live.catalog, discovered);

        let live_generic = resolve_explicit_model(
            HarnessKind::Codex,
            &LaunchModel {
                id: "future-model".to_owned(),
                source: LaunchModelSource::Explicit,
                reasoning: None,
            },
            &discovered,
        )
        .expect("discovered generic model should resolve");
        assert_eq!(
            live_generic.warning.as_deref(),
            Some(
                "warning: model 'future-model' has no bundled capability profile; using conservative defaults."
            )
        );

        let absent_bundled = resolve_explicit_model(
            HarnessKind::Fx,
            &LaunchModel {
                id: "glm5.3-flash".to_owned(),
                source: LaunchModelSource::Explicit,
                reasoning: None,
            },
            &[],
        )
        .expect("absent bundled model should be attempted");
        assert_eq!(
            absent_bundled.model.availability,
            ModelAvailability::ExplicitUndiscovered
        );
        assert_eq!(absent_bundled.model.profile_source, ProfileSource::Bundled);
        assert_eq!(absent_bundled.catalog.len(), 1);
        assert_eq!(
            absent_bundled.warning.as_deref(),
            Some(
                "warning: model 'glm5.3-flash' was not returned by live discovery for this credential; attempting it because you selected it explicitly."
            )
        );

        let absent_generic = resolve_explicit_model(
            HarnessKind::OpenCode,
            &LaunchModel {
                id: "future-model".to_owned(),
                source: LaunchModelSource::Explicit,
                reasoning: None,
            },
            &[],
        )
        .expect("absent generic model should be attempted");
        assert_eq!(absent_generic.model.profile_source, ProfileSource::Generic);
        assert_eq!(absent_generic.catalog.len(), 1);
        assert_eq!(
            absent_generic.warning.as_deref(),
            Some(
                "warning: model 'future-model' was not returned by live discovery and has no bundled capability profile; attempting it with conservative defaults because you selected it explicitly."
            )
        );

        for invalid in ["", " leading-space", "control\u{0007}"] {
            let error = resolve_explicit_model(
                HarnessKind::Codex,
                &LaunchModel {
                    id: invalid.to_owned(),
                    source: LaunchModelSource::Explicit,
                    reasoning: None,
                },
                &[],
            )
            .expect_err("invalid model IDs must fail safely");
            assert!(invalid.is_empty() || !error.to_string().contains(invalid));
        }
        let overlong = "x".repeat(257);
        let error = resolve_explicit_model(
            HarnessKind::Codex,
            &LaunchModel {
                id: overlong.clone(),
                source: LaunchModelSource::Explicit,
                reasoning: None,
            },
            &[],
        )
        .expect_err("overlong model ID must fail safely");
        assert!(!error.to_string().contains(&overlong));
    }

    #[test]
    fn explicit_warning_matrix_and_near_matches_are_deterministic() {
        assert_eq!(
            explicit_model_warning("future-model", true, false, &[]).as_deref(),
            Some(
                "warning: model 'future-model' has no bundled capability profile; using conservative defaults."
            )
        );
        assert_eq!(
            near_model_match("glm53flash", &["glm5.3-flash".to_owned()]),
            Some("glm5.3-flash".to_owned())
        );
        assert_eq!(
            near_model_match("model-c", &["model-a".to_owned(), "model-b".to_owned()]),
            None,
            "equal-distance candidates must not produce a suggestion"
        );
        assert_eq!(
            near_model_match("totally-different", &["qwen3.6".to_owned()]),
            None
        );
        assert_eq!(
            explicit_model_warning("glm53flash", true, true, &["glm5.3-flash".to_owned()])
                .as_deref(),
            Some(
                "warning: model 'glm53flash' was not returned by live discovery and has no bundled capability profile; attempting it with conservative defaults because you selected it explicitly. Did you mean 'glm5.3-flash'?"
            )
        );
    }

    #[test]
    fn implicit_fallback_prefers_default_then_live_bundled_models_only() {
        let selected = LaunchModel {
            id: "old-model".to_owned(),
            source: LaunchModelSource::Remembered,
            reasoning: None,
        };
        let error = RuntimeError::Bridge(BridgeError::SelectedModelUnavailable {
            model: "old-model".to_owned(),
            available: vec![
                "future-model".to_owned(),
                "glm5.3-flash".to_owned(),
                "qwen3.6".to_owned(),
            ],
        });
        let models = [
            coding_model_profile("future-model").expect("generic profile"),
            coding_model_profile("glm5.3-flash").expect("bundled profile"),
            coding_model_profile("qwen3.6").expect("default profile"),
        ];
        assert_eq!(
            fallback_model(&selected, &error, &models),
            Some(LaunchModel {
                id: "qwen3.6".to_owned(),
                source: LaunchModelSource::Fallback,
                reasoning: None,
            })
        );
        let default_selected = LaunchModel {
            source: LaunchModelSource::Default,
            ..selected.clone()
        };
        assert_eq!(
            fallback_model(&default_selected, &error, &models),
            Some(LaunchModel {
                id: "qwen3.6".to_owned(),
                source: LaunchModelSource::Fallback,
                reasoning: None,
            })
        );
        assert_eq!(
            fallback_model(&selected, &error, &models[..2]),
            Some(LaunchModel {
                id: "glm5.3-flash".to_owned(),
                source: LaunchModelSource::Fallback,
                reasoning: None,
            }),
            "the first live bundled model should win when the default is absent"
        );

        let explicit = LaunchModel {
            source: LaunchModelSource::Explicit,
            ..selected.clone()
        };
        assert_eq!(fallback_model(&explicit, &error, &models), None);
        assert_eq!(fallback_model(&selected, &error, &models[..1]), None);
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
                    id: "future-model".to_owned(),
                    source: LaunchModelSource::ExplicitUndiscovered,
                    reasoning: None,
                },
                "Starting codex with model 'future-model'. Reasoning: not specified.",
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
