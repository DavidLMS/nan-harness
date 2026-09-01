use super::arguments::{direct_chat_gateway_disabled, direct_chat_gateway_notice};
use super::discovery::discover_or_install_harness;
use super::models::{
    LaunchModel, LaunchModelSource, fallback_model, format_exit_bookend,
    format_launch_announcement, model_for_launch, should_attempt_fallback, successful_selection,
};
use super::personality::{random_startup_message, random_success_message};
use super::resolution::{
    generate_launch_id, offline_requested_model, required_config, resolve_explicit_model,
    valid_model_profile,
};
use super::signals::install_signal_handlers;
#[allow(clippy::wildcard_imports)]
use super::*;

pub(super) async fn run_simple_harness(
    cli: &Cli,
    interactive: bool,
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
        Command::Omp(arguments) => (HarnessKind::Omp, &arguments.run, &OmpAdapter),
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
            interactive,
            config,
            working_directory,
            bridge_diagnostics,
        )
        .await,
    )
}

pub(super) async fn run_harness(
    kind: HarnessKind,
    arguments: &HarnessRunArgs,
    adapter: &dyn HarnessAdapter,
    disable_direct_chat_gateway: bool,
    interactive: bool,
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
    let result = run_discovered_harness(
        arguments,
        adapter,
        disable_direct_chat_gateway,
        interactive,
        config,
        working_directory,
        bridge_diagnostics,
        &discovery,
    )
    .await;
    result.map_err(|error| RunError::after_discovery(error, discovery.harness))
}

pub(super) async fn run_discovered_harness(
    arguments: &HarnessRunArgs,
    adapter: &dyn HarnessAdapter,
    disable_direct_chat_gateway: bool,
    interactive: bool,
    config: Option<&commands::credentials::ResolvedLaunchConfig>,
    working_directory: &Path,
    bridge_diagnostics: &mut Vec<BridgeDiagnostic>,
    discovery: &DiscoveryReport,
) -> Result<i32, CliError> {
    let kind = discovery.harness.kind;
    let working_directory = working_directory.to_string_lossy().into_owned();
    let launch_id = generate_launch_id()?;
    let mut launch_model = model_for_launch(kind, arguments);
    if let Some(notice) = direct_chat_gateway_notice(disable_direct_chat_gateway, arguments.dry_run)
    {
        eprintln!("{notice}");
    }
    if arguments.dry_run {
        return print_dry_run_plan(
            adapter,
            &launch_id,
            &discovery.harness,
            &launch_model,
            arguments,
            &working_directory,
        );
    }

    check_required_runtime(kind)?;
    let launch_config = required_config(config)?;
    let (session, resolved_model) =
        prepare_launch_session(kind, &mut launch_model, launch_config).await?;
    let plan = build_launch_plan(
        adapter,
        &launch_id,
        &discovery.harness,
        resolved_model,
        arguments,
        &working_directory,
    )?;
    let cancellation = CancellationToken::new();
    let signal_task = install_signal_handlers(cancellation.clone());
    let supervisor = if disable_direct_chat_gateway {
        Supervisor::new().without_direct_chat_gateway()
    } else {
        Supervisor::new()
    };
    if let Some(message) = random_startup_message(interactive) {
        eprintln!("{message}");
    }
    eprintln!("{}", format_launch_announcement(kind, &launch_model));
    let execution = execute_with_fallback(
        &supervisor,
        &plan,
        &session,
        &cancellation,
        &launch_model,
        kind,
        adapter,
        &launch_id,
        &discovery.harness,
        arguments,
        &working_directory,
    )
    .await;
    signal_task.abort();
    let (report, effective_launch_model) = execution?;
    finish_harness_run(
        kind,
        &effective_launch_model,
        interactive,
        report,
        bridge_diagnostics,
    )
}

pub(super) fn build_launch_plan(
    adapter: &dyn HarnessAdapter,
    launch_id: &LaunchId,
    harness: &DetectedHarness,
    model: ResolvedModel,
    arguments: &HarnessRunArgs,
    working_directory: &str,
) -> Result<LaunchPlan, CliError> {
    let context = PlanContext {
        launch_id: launch_id.clone(),
        harness: harness.clone(),
        model,
        working_directory: working_directory.to_owned(),
        user_arguments: arguments.arguments.clone(),
        web_search_policy: web_search_policy(arguments),
        observability_format: ObservabilityFormat::Human,
    };
    build_validated_plan(adapter, &context).map_err(CliError::InvalidPlan)
}

pub(super) fn print_dry_run_plan(
    adapter: &dyn HarnessAdapter,
    launch_id: &LaunchId,
    harness: &DetectedHarness,
    launch_model: &LaunchModel,
    arguments: &HarnessRunArgs,
    working_directory: &str,
) -> Result<i32, CliError> {
    let plan = build_launch_plan(
        adapter,
        launch_id,
        harness,
        offline_requested_model(launch_model)?,
        arguments,
        working_directory,
    )?;
    let normalized = serde_json::to_string_pretty(&plan).map_err(CliError::SerializePlan)?;
    println!("{normalized}");
    Ok(0)
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn execute_with_fallback(
    supervisor: &Supervisor,
    plan: &LaunchPlan,
    session: &LaunchSession<'_>,
    cancellation: &CancellationToken,
    launch_model: &LaunchModel,
    kind: HarnessKind,
    adapter: &dyn HarnessAdapter,
    launch_id: &LaunchId,
    harness: &DetectedHarness,
    arguments: &HarnessRunArgs,
    working_directory: &str,
) -> Result<(nan_harness_runtime::ExecutionReport, LaunchModel), CliError> {
    let result = supervisor
        .execute_in_session(plan, session, cancellation)
        .await;
    let error = match result {
        Ok(report) => return Ok((report, launch_model.clone())),
        Err(error) => error,
    };
    let fallback = fallback_for_error(session, launch_model, &error).await;
    let Some(fallback) = fallback else {
        return Err(error.into());
    };

    eprintln!(
        "warning: model '{}' is no longer available for this credential; using '{fallback}'.",
        launch_model.id,
        fallback = fallback.id
    );
    let fallback_plan = build_launch_plan(
        adapter,
        launch_id,
        harness,
        offline_requested_model(&fallback)?,
        arguments,
        working_directory,
    )?;
    eprintln!("{}", format_launch_announcement(kind, &fallback));
    let report = supervisor
        .execute_in_session(&fallback_plan, session, cancellation)
        .await?;
    Ok((report, fallback))
}

pub(super) async fn fallback_for_error(
    session: &LaunchSession<'_>,
    launch_model: &LaunchModel,
    error: &RuntimeError,
) -> Option<LaunchModel> {
    if !should_attempt_fallback(launch_model, error) {
        return None;
    }
    let models = session.model_catalog().await.ok()?;
    fallback_model(launch_model, error, models)
}

pub(super) async fn prepare_launch_session<'a>(
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

pub(super) fn finish_harness_run(
    kind: HarnessKind,
    effective_launch_model: &LaunchModel,
    interactive: bool,
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
    if let Some(message) = random_success_message(interactive, report.outcome) {
        eprintln!("{message}");
    }
    Ok(report.exit_code)
}

pub(crate) const fn web_search_policy(arguments: &HarnessRunArgs) -> WebSearchPolicy {
    if arguments.search.no_search {
        WebSearchPolicy::Disabled
    } else if arguments.search.force_search {
        WebSearchPolicy::Force
    } else {
        WebSearchPolicy::Auto
    }
}
