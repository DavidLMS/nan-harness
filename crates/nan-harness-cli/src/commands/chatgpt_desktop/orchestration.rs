use super::ChatGptDesktopError;
use super::installation::ChatGptInstallation;
use super::platform::chatgpt_is_running;
use super::process::supervise_desktop;
use super::profile::{ManagedProfile, ensure_managed_profile};
use super::session::{
    apply_session, reject_orphaned_session_files, restore_session, selected_model_from_config,
};
use crate::app::ChatGptDesktopArgs;
use crate::commands::desktop::DesktopSessionLock;
use crate::commands::persistence::PersistenceManager;
use crate::error::CliError;
use crate::runner::install_signal_handlers;
use nan_harness_core::DesktopHarnessKind;
use nan_harness_runtime::{
    BridgeDiagnostic, CodexDesktopBridgeError, DesktopCompatibilityReport,
    DesktopCompatibilityStatus, start_codex_desktop_bridge,
};
use std::path::Path;

pub(super) fn enforce_compatibility(
    report: &DesktopCompatibilityReport,
    allow_unsupported: bool,
    allow_untested: bool,
) -> Result<(), ChatGptDesktopError> {
    match report.status {
        DesktopCompatibilityStatus::Tested => Ok(()),
        DesktopCompatibilityStatus::ContractOnly => {
            eprintln!(
                "warning: ChatGPT Desktop compatibility on this platform is based on deterministic contracts, not a live verification"
            );
            Ok(())
        }
        DesktopCompatibilityStatus::NewerUntested if allow_untested => {
            eprintln!(
                "warning: this ChatGPT Desktop version is newer than the pinned compatibility evidence"
            );
            Ok(())
        }
        DesktopCompatibilityStatus::NewerUntested => Err(ChatGptDesktopError::NewerUntested {
            last_app: report.last_compatible_app_version.clone(),
            last_codex: report.last_compatible_bundled_codex_version.clone(),
        }),
        DesktopCompatibilityStatus::OlderUnsupported if allow_unsupported => {
            eprintln!("warning: running an older unsupported ChatGPT Desktop version");
            Ok(())
        }
        DesktopCompatibilityStatus::OlderUnsupported => {
            Err(ChatGptDesktopError::OlderUnsupported {
                minimum_app: report.minimum_app_version.clone(),
                minimum_codex: report.minimum_bundled_codex_version.clone(),
            })
        }
        DesktopCompatibilityStatus::Unavailable => Err(ChatGptDesktopError::UnsupportedPlatform),
    }
}

pub(super) async fn run_managed_session(
    arguments: &ChatGptDesktopArgs,
    interactive: bool,
    bridge_diagnostics: &mut Vec<BridgeDiagnostic>,
    manager: &PersistenceManager,
    state_directory: &Path,
    installation: &ChatGptInstallation,
    remembered_model: Option<&str>,
) -> Result<i32, CliError> {
    let _lock = DesktopSessionLock::acquire(state_directory).map_err(ChatGptDesktopError::from)?;
    let profile = ManagedProfile::for_manager(manager);
    ensure_managed_profile(&profile)?;
    if restore_session(&profile)? {
        eprintln!("Recovered configuration from an interrupted ChatGPT Desktop session.");
    }
    reject_orphaned_session_files(&profile)?;

    let mut config = crate::commands::credentials::resolve_or_onboard(
        arguments.provider_base_url.clone(),
        interactive,
    )
    .await?;
    let discovered_models = config.model_catalog.take();
    let mut bridge = start_codex_desktop_bridge(
        &config.config,
        discovered_models,
        arguments.model.as_deref().or(remembered_model),
        arguments.aux_model.as_deref(),
        !arguments.search.no_search,
    )
    .await
    .map_err(ChatGptDesktopError::from)?;
    apply_session(&profile, &bridge, !arguments.search.no_search)?;
    if arguments.debug {
        eprintln!(
            "warning: debug mode exposes verbose ChatGPT Desktop logs; treat terminal output as private"
        );
    }
    eprintln!(
        "Starting ChatGPT Desktop Preview with NaN model '{}'.",
        bridge.selected_model()
    );
    if bridge.auxiliary_model() != bridge.selected_model() {
        eprintln!(
            "Desktop background requests use auxiliary NaN model '{}'.",
            bridge.auxiliary_model()
        );
    }

    let cancellation = nan_harness_runtime::CancellationToken::new();
    let signal_task = install_signal_handlers(cancellation.clone());
    let result = supervise_desktop(
        installation,
        &profile,
        &mut bridge,
        arguments.debug,
        &cancellation,
        bridge_diagnostics,
    )
    .await;
    signal_task.abort();
    let selected_after_exit = selected_model_from_config(&profile, bridge.available_models());
    if chatgpt_is_running()? {
        bridge.shutdown();
        let _ = bridge.wait().await;
        return Err(ChatGptDesktopError::AppDidNotTerminate.into());
    }
    bridge.shutdown();
    let bridge_wait = bridge.wait().await;
    let usage = bridge.usage();
    let cleanup = restore_session(&profile);
    cleanup?;
    let exit_code = result?;
    bridge_wait
        .map_err(CodexDesktopBridgeError::from)
        .map_err(ChatGptDesktopError::from)?;
    if let Some(model) = selected_after_exit
        && let Err(error) = manager.save_last_desktop_selection(DesktopHarnessKind::ChatGpt, &model)
    {
        eprintln!("warning: could not save the last Desktop model: {error}");
    }
    let outcome = if exit_code == 0 {
        nan_harness_runtime::ExecutionOutcome::Succeeded
    } else {
        nan_harness_runtime::ExecutionOutcome::Failed
    };
    if let Some(summary) = crate::usage_summary::render_snapshot(&usage, outcome) {
        eprintln!("{summary}");
    }
    Ok(exit_code)
}
