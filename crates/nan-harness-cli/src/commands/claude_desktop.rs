use crate::app::ClaudeDesktopArgs;
use crate::commands::credentials;
use crate::commands::persistence::PersistenceManager;
use crate::error::CliError;
use nan_harness_core::{DesktopHarnessKind, DesktopLaunchPlan, DesktopTransport, WebSearchPolicy};
use nan_harness_private_fs::open_private_new;
use nan_harness_runtime::{
    BridgeActivity, BridgeDiagnostic, ClaudeAutoModeReviewStage, DesktopCompatibilityEvidence,
    DesktopCompatibilityStatus, RunningClaudeDesktopBridge, classify_desktop_version,
    desktop_compatibility, start_claude_desktop_bridge,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest as _, Sha256};
use std::fs::{self, File, OpenOptions, Permissions, TryLockError};
use std::future::Future;
use std::io::{ErrorKind, Write as _};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::process::Stdio;
use std::time::Duration;
use tempfile::Builder as TempFileBuilder;

const PROFILE_ID: &str = "6e616e68-6172-4e65-8000-000000000001";
const PROFILE_NAME: &str = "NaN Harness";
const RECEIPT_SCHEMA: u8 = 2;
const DOCUMENT_IDS: [&str; 4] = [
    "normal-config",
    "third-party-config",
    "profile-meta",
    "profile",
];

mod configuration;
mod error;
mod orchestration;
mod paths;
mod process;
mod session;
#[cfg(test)]
mod tests;

#[allow(clippy::wildcard_imports)]
use configuration::*;
pub(crate) use error::ClaudeDesktopError;
#[allow(clippy::wildcard_imports)]
use orchestration::*;
#[allow(clippy::wildcard_imports)]
use paths::*;
#[allow(clippy::wildcard_imports)]
use process::*;
#[allow(clippy::wildcard_imports)]
use session::*;

pub(crate) async fn run(
    arguments: &ClaudeDesktopArgs,
    interactive: bool,
    bridge_diagnostics: &mut Vec<BridgeDiagnostic>,
) -> Result<i32, CliError> {
    if arguments.dry_run {
        return print_dry_run(arguments);
    }
    let compatibility =
        desktop_compatibility(DesktopHarnessKind::Claude).map_err(ClaudeDesktopError::from)?;
    match classify_desktop_version(&compatibility, None) {
        DesktopCompatibilityStatus::ContractOnly => eprintln!(
            "warning: Claude Desktop compatibility on this platform is based on deterministic contracts, not a live verification"
        ),
        DesktopCompatibilityStatus::Unavailable => {
            return Err(ClaudeDesktopError::UnsupportedPlatform.into());
        }
        DesktopCompatibilityStatus::Tested
        | DesktopCompatibilityStatus::NewerUntested
        | DesktopCompatibilityStatus::OlderUnsupported => {}
    }
    debug_assert_ne!(
        compatibility.evidence,
        DesktopCompatibilityEvidence::Unavailable
    );
    let manager = PersistenceManager::from_environment()?;
    let remembered_model = if arguments.model.is_none() {
        manager
            .last_desktop_selection(DesktopHarnessKind::Claude)?
            .map(|selection| selection.model)
    } else {
        None
    };
    let requested_model = arguments.model.as_deref().or(remembered_model.as_deref());
    let platform = DesktopPlatform::current()?;
    let paths = DesktopPaths::from_environment(platform)?;
    let process = SystemDesktopProcess::new(platform, arguments.executable.clone());
    if arguments.restore {
        return restore_command(&paths, &process);
    }
    let _lock = prepare_session_lock(&paths, &process)?;
    ensure_no_pending_recovery(&paths)?;
    if process.is_running()? {
        return Err(ClaudeDesktopError::AlreadyRunning.into());
    }
    let mut config =
        credentials::resolve_or_onboard(arguments.provider_base_url.clone(), interactive).await?;
    let discovered_models = config.model_catalog.take();
    let bridge = start_claude_desktop_bridge(
        &config.config,
        discovered_models,
        requested_model,
        arguments.show_auto,
        !arguments.search.no_search,
    )
    .await
    .map_err(ClaudeDesktopError::from)?;
    let selected_model = bridge.selected_model().to_owned();
    let result = run_ready_session(&paths, &process, &bridge, arguments.show_auto).await;
    let shutdown = bridge.shutdown_with_usage().await;
    match (result, shutdown) {
        (Err(error), _) => Err(error.into()),
        (Ok(code), Ok((diagnostics, usage))) => {
            append_diagnostics(bridge_diagnostics, diagnostics);
            if let Err(error) =
                manager.save_last_desktop_selection(DesktopHarnessKind::Claude, &selected_model)
            {
                eprintln!("warning: could not save the last Desktop model: {error}");
            }
            let outcome = if code == 0 {
                nan_harness_runtime::ExecutionOutcome::Succeeded
            } else {
                nan_harness_runtime::ExecutionOutcome::Failed
            };
            if let Some(summary) = crate::usage_summary::render_snapshot(&usage, outcome) {
                eprintln!("{summary}");
            }
            Ok(code)
        }
        (Ok(_), Err(error)) => Err(ClaudeDesktopError::Bridge(error).into()),
    }
}
