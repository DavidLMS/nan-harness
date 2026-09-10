mod preparation;
mod receipt;
mod restoration;
mod supervision;

use super::ZedDesktopError;
use super::paths::ZedPaths;
use super::process::SystemZedProcess;
use nan_harness_core::{CodingModelProfile, ContextLimit};
use nan_harness_runtime::RunningChatCompletionsGateway;
use std::path::Path;

#[cfg(test)]
pub(super) use preparation::begin_session_for_test;
#[cfg(test)]
pub(super) use preparation::begin_session_with_check;
pub(super) use restoration::{ensure_no_pending_session, restore_session};

pub(super) struct ZedSessionLaunch<'a> {
    pub(super) models: &'a [CodingModelProfile],
    pub(super) selected_model: &'a str,
    pub(super) workspace: &'a Path,
    pub(super) arguments: &'a [String],
    pub(super) context_limit: Option<&'a ContextLimit>,
}

pub(super) async fn run_managed_session(
    paths: &ZedPaths,
    process: &SystemZedProcess,
    gateway: &mut RunningChatCompletionsGateway,
    launch: ZedSessionLaunch<'_>,
) -> Result<i32, ZedDesktopError> {
    preparation::begin_session_with_context(
        paths,
        &gateway.client_base_url(),
        launch.models,
        launch.selected_model,
        launch.context_limit,
        || process.is_running(),
    )?;
    match process.is_running() {
        Ok(false) => {}
        Ok(true) => {
            return restoration::restore_after(paths, Err(ZedDesktopError::AlreadyRunning));
        }
        Err(error) => return Err(error),
    }
    let child = gateway
        .with_session_token(|token| process.spawn(launch.workspace, launch.arguments, token));
    let mut child = match child {
        Ok(child) => child,
        Err(error) => return restoration::restore_after(paths, Err(error)),
    };
    eprintln!(
        "Zed launched through NaN with model '{}' and {} available text models. Quit Zed to restore your settings.",
        launch.selected_model,
        launch.models.len()
    );

    let mut signals = supervision::termination_signals();
    let lifecycle = supervision::supervise(&mut child.child, process, gateway, &mut signals).await;
    match lifecycle {
        Ok(code) => restoration::restore_after(paths, Ok(code)),
        Err(error) => match process.is_running() {
            Ok(true) => {
                process.terminate_and_wait().await?;
                restoration::restore_after(paths, Err(error))
            }
            Ok(false) => restoration::restore_after(paths, Err(error)),
            Err(_) => Err(error),
        },
    }
}
