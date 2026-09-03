use super::ZedDesktopError;
use super::documents::{
    backup_file_name, patch_settings, read_optional, remove_managed_settings, sha256,
};
use super::paths::{SESSION_SCHEMA_VERSION, SessionReceipt, ZedPaths};
use super::process::SystemZedProcess;
use crate::commands::desktop::{
    create_private_directory, remove_file_if_present, write_private_atomic,
};
use nan_harness_core::CodingModelProfile;
use nan_harness_runtime::RunningChatCompletionsGateway;
use std::fs;
use std::io::ErrorKind;
use std::path::Path;
use std::process::ExitStatus;
use std::time::{Duration, Instant};
use tokio::process::Child;

const PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(250);
const QUIESCENCE_INTERVAL: Duration = Duration::from_secs(5);

pub(super) async fn run_managed_session(
    paths: &ZedPaths,
    process: &SystemZedProcess,
    gateway: &mut RunningChatCompletionsGateway,
    models: &[CodingModelProfile],
    selected_model: &str,
    workspace: &Path,
    arguments: &[String],
) -> Result<i32, ZedDesktopError> {
    begin_session(
        paths,
        process,
        &gateway.client_base_url(),
        models,
        selected_model,
    )?;
    match process.is_running() {
        Ok(false) => {}
        Ok(true) => return restore_after(paths, Err(ZedDesktopError::AlreadyRunning)),
        Err(error) => return Err(error),
    }
    let child = gateway.with_session_token(|token| process.spawn(workspace, arguments, token));
    let mut child = match child {
        Ok(child) => child,
        Err(error) => return restore_after(paths, Err(error)),
    };
    eprintln!(
        "Zed launched through NaN with model '{selected_model}' and {} available text models. Quit Zed to restore your settings.",
        models.len()
    );

    let mut signals = termination_signals();
    let lifecycle = supervise(&mut child, process, gateway, &mut signals).await;
    match lifecycle {
        Ok(code) => restore_after(paths, Ok(code)),
        Err(error) => match process.is_running() {
            Ok(true) => {
                process.terminate_and_wait().await?;
                restore_after(paths, Err(error))
            }
            Ok(false) => restore_after(paths, Err(error)),
            Err(_) => Err(error),
        },
    }
}

pub(super) fn begin_session(
    paths: &ZedPaths,
    process: &SystemZedProcess,
    gateway_url: &str,
    models: &[CodingModelProfile],
    selected_model: &str,
) -> Result<(), ZedDesktopError> {
    ensure_no_pending_session(paths)?;
    let original = read_optional(&paths.settings)?;
    let patched = patch_settings(original.as_deref(), gateway_url, models, selected_model)?;
    create_private_directory(&paths.backup_directory)?;
    if let Some(original) = original.as_deref() {
        write_private_atomic(&paths.backup_directory.join(backup_file_name()), original)?;
    }
    let receipt = SessionReceipt {
        schema_version: SESSION_SCHEMA_VERSION,
        file_existed: original.is_some(),
        original_sha256: original.as_deref().map(sha256),
        backup_file: backup_file_name().to_owned(),
        applied_file_sha256: sha256(&patched.contents),
        applied_provider_sha256: patched.provider_sha256,
        applied_default_model_sha256: patched.default_model_sha256,
        created_language_models: patched.created_language_models,
        created_openai_compatible: patched.created_openai_compatible,
        created_agent: patched.created_agent,
        previous_default_model: patched.previous_default_model,
    };
    write_receipt(&paths.session_receipt, &receipt)?;

    let process_running = process.is_running();
    let current = read_optional(&paths.settings);
    match (process_running, current) {
        (Ok(false), Ok(current)) if same_snapshot(current.as_deref(), original.as_deref()) => {}
        (Ok(true), _) => {
            discard_unapplied_state(paths)?;
            return Err(ZedDesktopError::AlreadyRunning);
        }
        (Err(error), _) => {
            discard_unapplied_state(paths)?;
            return Err(error);
        }
        (_, Err(error)) => {
            discard_unapplied_state(paths)?;
            return Err(error);
        }
        (Ok(false), Ok(_)) => {
            discard_unapplied_state(paths)?;
            return Err(ZedDesktopError::SettingsChangedBeforeWrite);
        }
    }

    if let Err(error) = write_private_atomic(&paths.settings, &patched.contents) {
        let error = ZedDesktopError::State(error);
        let _ = restore_session(paths);
        return Err(error);
    }
    Ok(())
}

pub(super) fn restore_session(paths: &ZedPaths) -> Result<bool, ZedDesktopError> {
    let Some(receipt_contents) = read_optional(&paths.session_receipt)? else {
        if paths.backup_directory.exists() {
            return Err(ZedDesktopError::OrphanBackup);
        }
        return Ok(false);
    };
    let receipt: SessionReceipt =
        serde_json::from_slice(&receipt_contents).map_err(ZedDesktopError::ParseReceipt)?;
    validate_receipt(&receipt)?;
    let current = read_optional(&paths.settings)?;
    if file_matches_original(current.as_deref(), &receipt) {
        cleanup_session_state(paths)?;
        return Ok(true);
    }
    if current
        .as_deref()
        .is_some_and(|contents| sha256(contents) == receipt.applied_file_sha256)
    {
        restore_exact(paths, &receipt)?;
        cleanup_session_state(paths)?;
        return Ok(true);
    }
    let current = current.ok_or(ZedDesktopError::ManagedConfigurationChanged)?;
    match remove_managed_settings(&current, &receipt)? {
        Some(restored) => write_private_atomic(&paths.settings, &restored)?,
        None => remove_file_if_present(&paths.settings)?,
    }
    cleanup_session_state(paths)?;
    Ok(true)
}

pub(super) fn ensure_no_pending_session(paths: &ZedPaths) -> Result<(), ZedDesktopError> {
    if paths.session_receipt.exists() || paths.backup_directory.exists() {
        Err(ZedDesktopError::PendingRecovery)
    } else {
        Ok(())
    }
}

async fn supervise(
    child: &mut Child,
    process: &SystemZedProcess,
    gateway: &mut RunningChatCompletionsGateway,
    signals: &mut tokio::sync::mpsc::UnboundedReceiver<i32>,
) -> Result<i32, ZedDesktopError> {
    let status = tokio::select! {
        status = child.wait() => status.map_err(ZedDesktopError::Wait)?,
        signal = signals.recv() => {
            let code = signal.unwrap_or(143);
            let _ = child.start_kill();
            process.terminate_and_wait().await?;
            return wait_for_quiescence(process, gateway, signals, code).await;
        }
        result = gateway.wait() => {
            let error = result.err().map_or(ZedDesktopError::GatewayExited, ZedDesktopError::Gateway);
            let _ = child.start_kill();
            process.terminate_and_wait().await?;
            return Err(error);
        }
    };
    let code = exit_code(status);
    if code != 0 && !process.is_running()? {
        return Err(ZedDesktopError::DidNotStart);
    }
    wait_for_quiescence(process, gateway, signals, code).await
}

async fn wait_for_quiescence(
    process: &SystemZedProcess,
    gateway: &mut RunningChatCompletionsGateway,
    signals: &mut tokio::sync::mpsc::UnboundedReceiver<i32>,
    exit_code: i32,
) -> Result<i32, ZedDesktopError> {
    let mut quiet_since = None;
    loop {
        if process.is_running()? {
            quiet_since = None;
        } else {
            let since = quiet_since.get_or_insert_with(Instant::now);
            if since.elapsed() >= QUIESCENCE_INTERVAL {
                return Ok(exit_code);
            }
        }
        tokio::select! {
            () = tokio::time::sleep(PROCESS_POLL_INTERVAL) => {}
            signal = signals.recv() => {
                let code = signal.unwrap_or(143);
                process.terminate_and_wait().await?;
                return Ok(code);
            }
            result = gateway.wait() => {
                let error = result.err().map_or(ZedDesktopError::GatewayExited, ZedDesktopError::Gateway);
                if process.is_running()? {
                    process.terminate_and_wait().await?;
                }
                return Err(error);
            }
        }
    }
}

fn restore_after(
    paths: &ZedPaths,
    result: Result<i32, ZedDesktopError>,
) -> Result<i32, ZedDesktopError> {
    match (result, restore_session(paths)) {
        (Ok(code), Ok(_)) => Ok(code),
        (Err(error), Ok(_)) | (_, Err(error)) => Err(error),
    }
}

fn restore_exact(paths: &ZedPaths, receipt: &SessionReceipt) -> Result<(), ZedDesktopError> {
    if receipt.file_existed {
        let backup = fs::read(paths.backup_directory.join(&receipt.backup_file))
            .map_err(ZedDesktopError::ReadBackup)?;
        if Some(sha256(&backup)) != receipt.original_sha256 {
            return Err(ZedDesktopError::BackupHashMismatch);
        }
        write_private_atomic(&paths.settings, &backup)?;
    } else {
        remove_file_if_present(&paths.settings)?;
    }
    Ok(())
}

fn validate_receipt(receipt: &SessionReceipt) -> Result<(), ZedDesktopError> {
    if receipt.schema_version != SESSION_SCHEMA_VERSION
        || receipt.backup_file != backup_file_name()
        || receipt.file_existed != receipt.original_sha256.is_some()
    {
        Err(ZedDesktopError::InvalidReceipt)
    } else {
        Ok(())
    }
}

fn cleanup_session_state(paths: &ZedPaths) -> Result<(), ZedDesktopError> {
    remove_file_if_present(&paths.backup_directory.join(backup_file_name()))?;
    match fs::remove_dir(&paths.backup_directory) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(ZedDesktopError::RemoveBackup(error)),
    }
    remove_file_if_present(&paths.session_receipt)?;
    Ok(())
}

fn discard_unapplied_state(paths: &ZedPaths) -> Result<(), ZedDesktopError> {
    cleanup_session_state(paths)
}

fn write_receipt(path: &Path, receipt: &SessionReceipt) -> Result<(), ZedDesktopError> {
    let mut payload = serde_json::to_vec_pretty(receipt).map_err(ZedDesktopError::Serialize)?;
    payload.push(b'\n');
    write_private_atomic(path, &payload)?;
    Ok(())
}

fn file_matches_original(current: Option<&[u8]>, receipt: &SessionReceipt) -> bool {
    match (
        current,
        receipt.file_existed,
        receipt.original_sha256.as_deref(),
    ) {
        (None, false, _) => true,
        (Some(current), true, Some(hash)) => sha256(current) == hash,
        _ => false,
    }
}

fn same_snapshot(left: Option<&[u8]>, right: Option<&[u8]>) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) => sha256(left) == sha256(right),
        _ => false,
    }
}

fn exit_code(status: ExitStatus) -> i32 {
    status.code().unwrap_or(1)
}

fn termination_signals() -> tokio::sync::mpsc::UnboundedReceiver<i32> {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(async move {
        #[cfg(unix)]
        {
            let Ok(mut interrupt) =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
            else {
                return;
            };
            let Ok(mut terminate) =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            else {
                return;
            };
            loop {
                tokio::select! {
                    value = interrupt.recv() => {
                        if value.is_none() || sender.send(130).is_err() { return; }
                    }
                    value = terminate.recv() => {
                        if value.is_none() || sender.send(143).is_err() { return; }
                    }
                }
            }
        }
        #[cfg(not(unix))]
        loop {
            if tokio::signal::ctrl_c().await.is_err() || sender.send(130).is_err() {
                return;
            }
        }
    });
    receiver
}
