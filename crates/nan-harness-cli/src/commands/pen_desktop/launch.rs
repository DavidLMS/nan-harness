use super::PenDesktopError;
use super::paths::PenPaths;
use super::process::SystemPenProcess;
use super::select_model;
use super::session::{self, ensure_no_pending_session, restore_session};
use crate::app::PenDesktopArgs;
use crate::commands::credentials;
use crate::commands::desktop::DesktopSessionLock;
use crate::commands::persistence::{PersistenceManager, discover_models};
use crate::error::CliError;
use nan_harness_core::{CodingModelProfile, DesktopHarnessKind};
use nan_harness_runtime::{
    BridgeDiagnostic, ExecutionOutcome, ProviderUsageSnapshot, ResolvedConfig,
    start_chat_completions_gateway,
};
use tokio::net::TcpListener;

pub(super) fn acquire_session_lock(
    paths: &PenPaths,
) -> Result<DesktopSessionLock, PenDesktopError> {
    DesktopSessionLock::acquire(&paths.state_directory).map_err(PenDesktopError::from)
}

pub(super) fn restore_only(paths: &PenPaths, process: &SystemPenProcess) -> Result<i32, CliError> {
    if process.is_running()? {
        return Err(PenDesktopError::AlreadyRunning.into());
    }
    if restore_session(paths)? {
        eprintln!("Pen Desktop configuration restored.");
    } else {
        eprintln!("No Pen Desktop session needs recovery.");
    }
    Ok(0)
}

pub(super) fn ensure_ready_to_launch(
    paths: &PenPaths,
    process: &SystemPenProcess,
) -> Result<(), PenDesktopError> {
    ensure_no_pending_session(paths)?;
    if process.is_running()? {
        return Err(PenDesktopError::AlreadyRunning);
    }
    Ok(())
}

pub(super) struct PreparedLaunch {
    config: ResolvedConfig,
    models: Vec<CodingModelProfile>,
    manager: PersistenceManager,
    selected_model: String,
}

pub(super) async fn prepare_launch(
    arguments: &PenDesktopArgs,
    interactive: bool,
) -> Result<PreparedLaunch, CliError> {
    let mut launch_config =
        credentials::resolve_or_onboard(arguments.provider_base_url.clone(), interactive).await?;
    let models = match launch_config.model_catalog.take() {
        Some(models) => models,
        None => discover_models(&launch_config.config).await?,
    };
    let manager = PersistenceManager::from_environment()?;
    let requested = requested_model(arguments, &manager)?;
    let selected_model = select_model(&models, requested.as_deref())?.to_owned();
    Ok(PreparedLaunch {
        config: launch_config.config,
        models,
        manager,
        selected_model,
    })
}

fn requested_model(
    arguments: &PenDesktopArgs,
    manager: &PersistenceManager,
) -> Result<Option<String>, CliError> {
    if let Some(model) = &arguments.model {
        return Ok(Some(model.clone()));
    }
    Ok(manager
        .last_desktop_selection(DesktopHarnessKind::Pen)?
        .map(|selection| selection.model))
}

pub(super) async fn launch_managed_session(
    prepared: &PreparedLaunch,
    paths: &PenPaths,
    process: &SystemPenProcess,
    bridge_diagnostics: &mut Vec<BridgeDiagnostic>,
) -> Result<i32, CliError> {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(PenDesktopError::BindGateway)?;
    let gateway =
        start_chat_completions_gateway(&prepared.config, listener, &prepared.selected_model, false)
            .map_err(PenDesktopError::from)?;
    let result = session::run_managed_session(paths, process, &gateway, &prepared.models).await;
    let shutdown = gateway.shutdown_with_usage().await;

    match (result, shutdown) {
        (Err(error), _) => Err(error.into()),
        (Ok(code), Ok((diagnostics, usage))) => {
            merge_diagnostics(bridge_diagnostics, diagnostics);
            remember_model(&prepared.manager, &prepared.selected_model);
            report_usage(&usage, code);
            Ok(code)
        }
        (Ok(_), Err(error)) => Err(PenDesktopError::Gateway(error).into()),
    }
}

fn merge_diagnostics(target: &mut Vec<BridgeDiagnostic>, diagnostics: Vec<BridgeDiagnostic>) {
    for diagnostic in diagnostics {
        if !target.contains(&diagnostic) {
            target.push(diagnostic);
        }
    }
}

/// Pen remembers the selected model even when the app exits with a failure,
/// because the choice was still the user's last deliberate selection.
fn remember_model(manager: &PersistenceManager, selected_model: &str) {
    if let Err(error) = manager.save_last_desktop_selection(DesktopHarnessKind::Pen, selected_model)
    {
        eprintln!("warning: could not save the last Pen model: {error}");
    }
}

fn report_usage(usage: &ProviderUsageSnapshot, code: i32) {
    let outcome = if code == 0 {
        ExecutionOutcome::Succeeded
    } else {
        ExecutionOutcome::Failed
    };
    if let Some(summary) = crate::usage_summary::render_snapshot(usage, outcome) {
        eprintln!("{summary}");
    }
}

#[cfg(test)]
mod tests {
    use super::{BridgeDiagnostic, DesktopHarnessKind, PersistenceManager, merge_diagnostics};
    use crate::app::PenDesktopArgs;
    use nan_harness_runtime::{BridgeDiagnosticReason, BridgeEndpoint};

    fn arguments(model: Option<&str>) -> PenDesktopArgs {
        PenDesktopArgs {
            model: model.map(str::to_owned),
            provider_base_url: None,
            executable: None,
            allow_unsupported: false,
            allow_untested: false,
            dry_run: false,
            restore: false,
        }
    }

    fn manager() -> (tempfile::TempDir, PersistenceManager) {
        let root = tempfile::tempdir().expect("temp root");
        let manager =
            PersistenceManager::new_for_tests(root.path().join("state"), root.path().join("home"));
        (root, manager)
    }

    fn diagnostic(code: &'static str) -> BridgeDiagnostic {
        BridgeDiagnostic {
            code,
            reason: BridgeDiagnosticReason::UpstreamStatus,
            http_status: Some(500),
            endpoint: BridgeEndpoint::Messages,
            model_id: None,
            requested_reasoning: None,
            model_policy: None,
            timeout_phase: None,
            recovery_outcome: None,
            attempt: None,
            priority: None,
            cache_replay_detected: None,
            cache_bypass_attempted: None,
        }
    }

    #[test]
    fn an_explicit_model_is_used_without_consulting_the_remembered_selection() {
        let (_root, manager) = manager();
        manager
            .save_last_desktop_selection(DesktopHarnessKind::Pen, "glm5.3-flash")
            .expect("remember a model");
        let requested = super::requested_model(&arguments(Some("qwen3.6")), &manager)
            .expect("explicit model resolves");
        assert_eq!(requested.as_deref(), Some("qwen3.6"));
    }

    #[test]
    fn the_remembered_model_is_reused_only_when_no_model_is_given() {
        let (_root, manager) = manager();
        assert_eq!(
            super::requested_model(&arguments(None), &manager).expect("no remembered model"),
            None
        );
        super::remember_model(&manager, "glm5.3-flash");
        assert_eq!(
            super::requested_model(&arguments(None), &manager)
                .expect("remembered model")
                .as_deref(),
            Some("glm5.3-flash")
        );
    }

    #[test]
    fn merging_diagnostics_appends_in_order_and_drops_duplicates() {
        let mut collected = vec![diagnostic("first")];
        merge_diagnostics(
            &mut collected,
            vec![diagnostic("first"), diagnostic("second")],
        );
        let codes: Vec<&str> = collected.iter().map(|entry| entry.code).collect();
        assert_eq!(codes, vec!["first", "second"]);
    }
}
