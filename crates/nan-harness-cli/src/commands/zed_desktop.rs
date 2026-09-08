mod documents;
mod error;
mod paths;
mod process;
mod session;
#[cfg(test)]
mod tests;

pub(crate) use error::ZedDesktopError;

use crate::app::ZedDesktopArgs;
use crate::commands::credentials::{self, ResolvedLaunchConfig};
use crate::commands::desktop::DesktopSessionLock;
use crate::commands::persistence::{PersistenceManager, discover_models};
use crate::error::CliError;
use nan_harness_core::{
    CodingModelProfile, DesktopHarnessKind, DesktopLaunchPlan, DesktopTransport, WebSearchPolicy,
};
use nan_harness_runtime::{
    BridgeDiagnostic, DesktopCompatibilityEntry, DesktopCompatibilityStatus, ExecutionOutcome,
    ProviderUsageSnapshot, classify_desktop_version, desktop_compatibility,
    start_chat_completions_gateway,
};
use semver::Version;
use std::path::{Path, PathBuf};
use tokio::net::TcpListener;

use paths::ZedPaths;
use process::SystemZedProcess;

const DEFAULT_MODEL_ID: &str = "qwen3.6";

/// Provider access, model catalog, and selection resolved before the gateway
/// starts. `manager` outlives the session because the remembered model is
/// persisted only after a successful exit.
struct LaunchInputs {
    launch_config: ResolvedLaunchConfig,
    models: Vec<CodingModelProfile>,
    selected_model: String,
    manager: PersistenceManager,
}

/// What a managed session leaves behind once the gateway has shut down
/// cleanly and no primary error took precedence.
#[derive(Debug)]
struct CompletedSession {
    code: i32,
    diagnostics: Vec<BridgeDiagnostic>,
    usage: ProviderUsageSnapshot,
}

pub(crate) async fn run(
    arguments: &ZedDesktopArgs,
    interactive: bool,
    bridge_diagnostics: &mut Vec<BridgeDiagnostic>,
) -> Result<i32, CliError> {
    process::validate_passthrough_arguments(&arguments.arguments)?;
    if arguments.dry_run {
        return print_dry_run(arguments);
    }

    let paths = ZedPaths::from_environment(arguments.user_data_dir.as_deref())?;
    let process = SystemZedProcess::new(
        arguments.executable.clone(),
        arguments.user_data_dir.clone(),
    )?;
    if arguments.restore {
        return restore_command(&paths, &process);
    }

    let workspace = check_installation(arguments, &process)?;
    let _lock = lock_ready_session(&paths, &process)?;
    let launch = resolve_launch_inputs(arguments, interactive).await?;
    let session =
        run_managed_gateway(&paths, &process, &workspace, &arguments.arguments, &launch).await?;
    Ok(report_session(&launch, session, bridge_diagnostics))
}

fn restore_command(paths: &ZedPaths, process: &SystemZedProcess) -> Result<i32, CliError> {
    let _lock =
        DesktopSessionLock::acquire(&paths.state_directory).map_err(ZedDesktopError::from)?;
    if process.is_running()? {
        return Err(ZedDesktopError::AlreadyRunning.into());
    }
    if session::restore_session(paths)? {
        eprintln!("Zed settings restored.");
    } else {
        eprintln!("No Zed session needs recovery.");
    }
    Ok(0)
}

fn check_installation(
    arguments: &ZedDesktopArgs,
    process: &SystemZedProcess,
) -> Result<PathBuf, ZedDesktopError> {
    process.ensure_available()?;
    let installed_version = process.installed_version()?;
    let entry = desktop_compatibility(DesktopHarnessKind::Zed)?;
    validate_compatibility(
        &entry,
        installed_version.as_ref(),
        arguments.allow_unsupported,
        arguments.allow_untested,
    )?;
    if process.is_running()? {
        return Err(ZedDesktopError::AlreadyRunning);
    }
    resolve_workspace(arguments.workspace.as_deref())
}

/// Exclusion phase: hold the desktop session lock, reject a pending recovery,
/// and re-check the running state now that concurrent launches are excluded.
fn lock_ready_session(
    paths: &ZedPaths,
    process: &SystemZedProcess,
) -> Result<DesktopSessionLock, ZedDesktopError> {
    let lock =
        DesktopSessionLock::acquire(&paths.state_directory).map_err(ZedDesktopError::from)?;
    session::ensure_no_pending_session(paths)?;
    if process.is_running()? {
        return Err(ZedDesktopError::AlreadyRunning);
    }
    Ok(lock)
}

async fn resolve_launch_inputs(
    arguments: &ZedDesktopArgs,
    interactive: bool,
) -> Result<LaunchInputs, CliError> {
    let mut launch_config =
        credentials::resolve_or_onboard(arguments.provider_base_url.clone(), interactive).await?;
    let models = match launch_config.model_catalog.take() {
        Some(models) => models,
        None => discover_models(&launch_config.config).await?,
    };
    let manager = PersistenceManager::from_environment()?;
    let remembered = if arguments.model.is_none() {
        manager
            .last_desktop_selection(DesktopHarnessKind::Zed)?
            .map(|selection| selection.model)
    } else {
        None
    };
    let selected_model = select_model(
        &models,
        arguments.model.as_deref().or(remembered.as_deref()),
    )?
    .to_owned();
    Ok(LaunchInputs {
        launch_config,
        models,
        selected_model,
        manager,
    })
}

/// Lifecycle phase: the gateway lives exactly as long as the managed session,
/// and is always shut down before an outcome is reported.
async fn run_managed_gateway(
    paths: &ZedPaths,
    process: &SystemZedProcess,
    workspace: &Path,
    arguments: &[String],
    launch: &LaunchInputs,
) -> Result<CompletedSession, ZedDesktopError> {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(ZedDesktopError::BindGateway)?;
    let mut gateway = start_chat_completions_gateway(
        &launch.launch_config.config,
        listener,
        &launch.selected_model,
        false,
    )
    .map_err(ZedDesktopError::from)?;
    let result = session::run_managed_session(
        paths,
        process,
        &mut gateway,
        &launch.models,
        &launch.selected_model,
        workspace,
        arguments,
    )
    .await;
    let shutdown = gateway
        .shutdown_with_usage()
        .await
        .map_err(ZedDesktopError::Gateway);
    completed_session(result, shutdown)
}

/// A session error takes precedence over a shutdown failure, which in turn
/// takes precedence over the exit code.
fn completed_session(
    result: Result<i32, ZedDesktopError>,
    shutdown: Result<(Vec<BridgeDiagnostic>, ProviderUsageSnapshot), ZedDesktopError>,
) -> Result<CompletedSession, ZedDesktopError> {
    match (result, shutdown) {
        (Err(error), _) | (Ok(_), Err(error)) => Err(error),
        (Ok(code), Ok((diagnostics, usage))) => Ok(CompletedSession {
            code,
            diagnostics,
            usage,
        }),
    }
}

fn report_session(
    launch: &LaunchInputs,
    session: CompletedSession,
    bridge_diagnostics: &mut Vec<BridgeDiagnostic>,
) -> i32 {
    append_new_diagnostics(bridge_diagnostics, session.diagnostics);
    if session.code == 0
        && let Err(error) = launch
            .manager
            .save_last_desktop_selection(DesktopHarnessKind::Zed, &launch.selected_model)
    {
        eprintln!("warning: could not save the last Zed model: {error}");
    }
    if let Some(summary) =
        crate::usage_summary::render_snapshot(&session.usage, exit_outcome(session.code))
    {
        eprintln!("{summary}");
    }
    session.code
}

fn append_new_diagnostics(target: &mut Vec<BridgeDiagnostic>, diagnostics: Vec<BridgeDiagnostic>) {
    for diagnostic in diagnostics {
        if !target.contains(&diagnostic) {
            target.push(diagnostic);
        }
    }
}

const fn exit_outcome(code: i32) -> ExecutionOutcome {
    if code == 0 {
        ExecutionOutcome::Succeeded
    } else {
        ExecutionOutcome::Failed
    }
}

fn print_dry_run(arguments: &ZedDesktopArgs) -> Result<i32, CliError> {
    let mut plan = DesktopLaunchPlan::new(
        DesktopHarnessKind::Zed,
        DesktopTransport::ChatCompletionsGateway,
    );
    if arguments.executable.is_some() {
        plan.executable = Some(PathBuf::from("<explicit-executable>"));
    }
    plan.selected_model.clone_from(&arguments.model);
    plan.web_search_policy = WebSearchPolicy::Disabled;
    plan.restore_only = arguments.restore;
    if arguments.workspace.is_some() {
        plan.native_arguments.push("<workspace>".to_owned());
    }
    if arguments.user_data_dir.is_some() {
        plan.native_arguments
            .push("--user-data-dir=<isolated-profile>".to_owned());
    }
    if !arguments.arguments.is_empty() {
        plan.native_arguments.push(format!(
            "<{} native argument{}>",
            arguments.arguments.len(),
            if arguments.arguments.len() == 1 {
                ""
            } else {
                "s"
            }
        ));
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&plan).map_err(ZedDesktopError::Serialize)?
    );
    Ok(0)
}

fn validate_compatibility(
    entry: &DesktopCompatibilityEntry,
    installed: Option<&Version>,
    allow_unsupported: bool,
    _allow_untested: bool,
) -> Result<(), ZedDesktopError> {
    match classify_desktop_version(entry, installed) {
        DesktopCompatibilityStatus::Tested => Ok(()),
        DesktopCompatibilityStatus::ContractOnly => {
            eprintln!(
                "warning: Zed compatibility on this platform is contract-tested, not live-verified"
            );
            Ok(())
        }
        DesktopCompatibilityStatus::NewerUntested => {
            eprintln!(
                "{}",
                crate::commands::desktop::newer_version_warning(
                    "Zed",
                    &installed.map_or_else(|| "unknown".to_owned(), ToString::to_string),
                    &entry
                        .last_compatible_app_version
                        .as_ref()
                        .map_or_else(|| "unknown".to_owned(), ToString::to_string),
                )
            );
            Ok(())
        }
        DesktopCompatibilityStatus::OlderUnsupported if allow_unsupported => {
            eprintln!("warning: this Zed version is older than the supported version");
            Ok(())
        }
        DesktopCompatibilityStatus::OlderUnsupported => Err(ZedDesktopError::OlderUnsupported),
        DesktopCompatibilityStatus::Unavailable => Err(ZedDesktopError::UnsupportedPlatform),
    }
}

fn select_model<'a>(
    models: &'a [CodingModelProfile],
    requested: Option<&str>,
) -> Result<&'a str, ZedDesktopError> {
    let selected = requested.unwrap_or(DEFAULT_MODEL_ID);
    if let Some(model) = models.iter().find(|model| model.id == selected) {
        return Ok(&model.id);
    }
    if requested.is_some() {
        return Err(ZedDesktopError::ModelUnavailable {
            model: selected.to_owned(),
            available: models.iter().map(|model| model.id.clone()).collect(),
        });
    }
    models
        .first()
        .map(|model| model.id.as_str())
        .ok_or(ZedDesktopError::EmptyModelCatalog)
}

fn resolve_workspace(requested: Option<&Path>) -> Result<PathBuf, ZedDesktopError> {
    let workspace = match requested {
        Some(path) if path.is_absolute() => path.to_path_buf(),
        Some(path) => std::env::current_dir()
            .map_err(ZedDesktopError::ReadSettings)?
            .join(path),
        None => std::env::current_dir().map_err(ZedDesktopError::ReadSettings)?,
    };
    if workspace.is_dir() {
        Ok(workspace)
    } else {
        Err(ZedDesktopError::InvalidWorkspace)
    }
}

fn extract_semver(output: &str) -> Option<Version> {
    output.split_whitespace().find_map(|candidate| {
        let candidate = candidate.trim_matches(|character: char| {
            !character.is_ascii_digit() && character != '.' && character != '-' && character != '+'
        });
        Version::parse(candidate).ok()
    })
}
