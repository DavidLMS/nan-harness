mod documents;
mod error;
mod paths;
mod process;
mod session;
#[cfg(test)]
mod tests;

pub(crate) use error::ZedDesktopError;

use crate::app::ZedDesktopArgs;
use crate::commands::credentials;
use crate::commands::desktop::DesktopSessionLock;
use crate::commands::persistence::{PersistenceManager, discover_models};
use crate::error::CliError;
use nan_harness_core::{
    CodingModelProfile, DesktopHarnessKind, DesktopLaunchPlan, DesktopTransport, WebSearchPolicy,
};
use nan_harness_runtime::{
    BridgeDiagnostic, DesktopCompatibilityStatus, ExecutionOutcome, classify_desktop_version,
    desktop_compatibility, start_chat_completions_gateway,
};
use semver::Version;
use std::path::{Path, PathBuf};
use tokio::net::TcpListener;

use paths::ZedPaths;
use process::SystemZedProcess;

const DEFAULT_MODEL_ID: &str = "qwen3.6";

pub(crate) async fn run(
    arguments: &ZedDesktopArgs,
    interactive: bool,
    bridge_diagnostics: &mut Vec<BridgeDiagnostic>,
) -> Result<i32, CliError> {
    process::validate_passthrough_arguments(&arguments.arguments)?;
    if arguments.dry_run {
        return print_dry_run(arguments);
    }

    let paths = ZedPaths::from_environment()?;
    let process = SystemZedProcess::new(arguments.executable.clone())?;
    if arguments.restore {
        let _lock =
            DesktopSessionLock::acquire(&paths.state_directory).map_err(ZedDesktopError::from)?;
        if process.is_running()? {
            return Err(ZedDesktopError::AlreadyRunning.into());
        }
        if session::restore_session(&paths)? {
            eprintln!("Zed settings restored.");
        } else {
            eprintln!("No Zed session needs recovery.");
        }
        return Ok(0);
    }

    process.ensure_available()?;
    let installed_version = process.installed_version()?;
    validate_compatibility(
        installed_version.as_ref(),
        arguments.allow_unsupported,
        arguments.allow_untested,
    )?;
    if process.is_running()? {
        return Err(ZedDesktopError::AlreadyRunning.into());
    }
    let workspace = resolve_workspace(arguments.workspace.as_deref())?;
    let _lock =
        DesktopSessionLock::acquire(&paths.state_directory).map_err(ZedDesktopError::from)?;
    session::ensure_no_pending_session(&paths)?;
    if process.is_running()? {
        return Err(ZedDesktopError::AlreadyRunning.into());
    }

    let mut launch_config = credentials::resolve_or_onboard(None, interactive).await?;
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
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(ZedDesktopError::BindGateway)?;
    let mut gateway =
        start_chat_completions_gateway(&launch_config.config, listener, &selected_model, false)
            .map_err(ZedDesktopError::from)?;
    let result = session::run_managed_session(
        &paths,
        &process,
        &mut gateway,
        &models,
        &selected_model,
        &workspace,
        &arguments.arguments,
    )
    .await;
    let shutdown = gateway.shutdown_with_usage().await;

    match (result, shutdown) {
        (Err(error), _) => Err(error.into()),
        (Ok(code), Ok((diagnostics, usage))) => {
            for diagnostic in diagnostics {
                if !bridge_diagnostics.contains(&diagnostic) {
                    bridge_diagnostics.push(diagnostic);
                }
            }
            if code == 0
                && let Err(error) =
                    manager.save_last_desktop_selection(DesktopHarnessKind::Zed, &selected_model)
            {
                eprintln!("warning: could not save the last Zed model: {error}");
            }
            let outcome = if code == 0 {
                ExecutionOutcome::Succeeded
            } else {
                ExecutionOutcome::Failed
            };
            if let Some(summary) = crate::usage_summary::render_snapshot(&usage, outcome) {
                eprintln!("{summary}");
            }
            Ok(code)
        }
        (Ok(_), Err(error)) => Err(ZedDesktopError::Gateway(error).into()),
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
    installed: Option<&Version>,
    allow_unsupported: bool,
    allow_untested: bool,
) -> Result<(), ZedDesktopError> {
    let entry = desktop_compatibility(DesktopHarnessKind::Zed)?;
    match classify_desktop_version(&entry, installed) {
        DesktopCompatibilityStatus::Tested => Ok(()),
        DesktopCompatibilityStatus::ContractOnly => {
            eprintln!(
                "warning: Zed compatibility on this platform is contract-tested, not live-verified"
            );
            Ok(())
        }
        DesktopCompatibilityStatus::NewerUntested if allow_untested => {
            eprintln!("warning: this Zed version is newer than the live-verified version");
            Ok(())
        }
        DesktopCompatibilityStatus::NewerUntested => Err(ZedDesktopError::NewerUntested),
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
