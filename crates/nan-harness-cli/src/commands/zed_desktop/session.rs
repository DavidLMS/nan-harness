mod preparation;
mod receipt;
mod restoration;
mod supervision;

use super::ZedDesktopError;
use super::paths::ZedPaths;
use super::process::SystemZedProcess;
use nan_harness_core::CodingModelProfile;
use nan_harness_runtime::RunningChatCompletionsGateway;
use std::path::Path;

#[cfg(test)]
pub(super) use preparation::begin_session_for_test;
#[cfg(test)]
pub(super) use preparation::begin_session_with_check;
pub(super) use restoration::{ensure_no_pending_session, restore_session};

pub(super) async fn run_managed_session(
    paths: &ZedPaths,
    process: &SystemZedProcess,
    gateway: &mut RunningChatCompletionsGateway,
    models: &[CodingModelProfile],
    selected_model: &str,
    workspace: &Path,
    arguments: &[String],
) -> Result<i32, ZedDesktopError> {
    preparation::begin_session(
        paths,
        process,
        &gateway.client_base_url(),
        models,
        selected_model,
    )?;
    match process.is_running() {
        Ok(false) => {}
        Ok(true) => {
            return restoration::restore_after(paths, Err(ZedDesktopError::AlreadyRunning));
        }
        Err(error) => return Err(error),
    }
    let child = gateway.with_session_token(|token| process.spawn(workspace, arguments, token));
    let mut child = match child {
        Ok(child) => child,
        Err(error) => return restoration::restore_after(paths, Err(error)),
    };
    eprintln!(
        "Zed launched through NaN with model '{selected_model}' and {} available text models. Quit Zed to restore your settings.",
        models.len()
    );

    let mut signals = supervision::termination_signals();
    let lifecycle = supervision::supervise(&mut child, process, gateway, &mut signals).await;
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
