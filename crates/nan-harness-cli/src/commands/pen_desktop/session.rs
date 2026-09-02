use super::PenDesktopError;
use super::documents::{
    hash_value, merge_original_entry, provider_entry, read_json_object, read_optional,
    write_json_private,
};
use super::paths::{FileSnapshot, PenPaths, SESSION_SCHEMA_VERSION, SessionReceipt};
use super::process::{SystemPenProcess, WaitOutcome, terminate_and_wait, wait_for_exit_or_signal};
use crate::commands::desktop::{
    create_private_directory, remove_file_if_present, write_private_atomic,
};
use nan_harness_core::CodingModelProfile;
use nan_harness_runtime::RunningChatCompletionsGateway;
use serde_json::Value;
use std::fs;
use std::io::ErrorKind;

pub(super) async fn run_managed_session(
    paths: &PenPaths,
    process: &SystemPenProcess,
    gateway: &RunningChatCompletionsGateway,
    models: &[CodingModelProfile],
) -> Result<i32, PenDesktopError> {
    let models_document = super::documents::patched_models_document(
        read_json_object(&paths.models)?,
        &gateway.client_base_url(),
        models,
    )?;
    let auth_document = gateway.with_session_token(|token| {
        super::documents::patched_auth_document(read_json_object(&paths.auth)?, token)
    })?;
    begin_session(paths, &models_document, &auth_document)?;
    match process.is_running() {
        Ok(false) => {}
        Ok(true) => return restore_after(paths, Err(PenDesktopError::AlreadyRunning)),
        Err(error) => return restore_after(paths, Err(error)),
    }
    if let Err(error) = process.launch() {
        return restore_after(paths, Err(error));
    }
    eprintln!(
        "Pen Desktop launched through NaN with {} available text models. Quit Pen to restore its previous configuration.",
        models.len()
    );
    let completion = wait_for_exit_or_signal(process).await;
    match completion {
        Ok(WaitOutcome::Exited) => restore_after(paths, Ok(0)),
        Ok(WaitOutcome::Signaled(code)) => {
            terminate_and_wait(process).await?;
            restore_after(paths, Ok(code))
        }
        Err(error) => {
            terminate_and_wait(process).await?;
            restore_after(paths, Err(error))
        }
    }
}

pub(super) fn begin_session(
    paths: &PenPaths,
    models_document: &[u8],
    auth_document: &[u8],
) -> Result<(), PenDesktopError> {
    ensure_no_pending_session(paths)?;
    let models_original = read_optional(&paths.models)?;
    let auth_original = read_optional(&paths.auth)?;
    let models_entry = provider_entry(models_document, super::PenDocumentKind::Models)?;
    let auth_entry = provider_entry(auth_document, super::PenDocumentKind::Auth)?;
    create_private_directory(&paths.session_backup_directory)?;
    let captured = (|| {
        Ok::<_, PenDesktopError>(SessionReceipt {
            schema_version: SESSION_SCHEMA_VERSION,
            models: snapshot(
                &paths.session_backup_directory,
                "models.backup",
                models_original.as_deref(),
                models_document,
                &models_entry,
            )?,
            auth: snapshot(
                &paths.session_backup_directory,
                "auth.backup",
                auth_original.as_deref(),
                auth_document,
                &auth_entry,
            )?,
        })
    })();
    let receipt = match captured {
        Ok(receipt) => receipt,
        Err(error) => {
            cleanup_uncommitted_backups(paths);
            return Err(error);
        }
    };
    if let Err(error) = write_json_private(&paths.session_receipt, &receipt) {
        cleanup_uncommitted_backups(paths);
        return Err(error);
    }
    if let Err(error) = write_private_atomic(&paths.models, models_document)
        .and_then(|()| write_private_atomic(&paths.auth, auth_document))
    {
        let error = PenDesktopError::State(error);
        let _ = restore_session(paths);
        return Err(error);
    }
    Ok(())
}

fn cleanup_uncommitted_backups(paths: &PenPaths) {
    let _ = remove_file_if_present(&paths.session_backup_directory.join("models.backup"));
    let _ = remove_file_if_present(&paths.session_backup_directory.join("auth.backup"));
    let _ = fs::remove_dir(&paths.session_backup_directory);
}

fn snapshot(
    backup_directory: &std::path::Path,
    backup_file: &str,
    original: Option<&[u8]>,
    applied: &[u8],
    applied_entry: &Value,
) -> Result<FileSnapshot, PenDesktopError> {
    if let Some(original) = original {
        write_private_atomic(&backup_directory.join(backup_file), original)?;
    }
    Ok(FileSnapshot {
        existed: original.is_some(),
        original_sha256: original.map(super::documents::sha256),
        backup_file: backup_file.to_owned(),
        applied_file_sha256: super::documents::sha256(applied),
        applied_entry_sha256: hash_value(applied_entry)?,
    })
}

pub(super) fn ensure_no_pending_session(paths: &PenPaths) -> Result<(), PenDesktopError> {
    if paths.session_receipt.exists() || paths.session_backup_directory.exists() {
        return Err(PenDesktopError::PendingRecovery);
    }
    Ok(())
}

fn restore_after(
    paths: &PenPaths,
    result: Result<i32, PenDesktopError>,
) -> Result<i32, PenDesktopError> {
    match (result, restore_session(paths)) {
        (Ok(code), Ok(_)) => Ok(code),
        (Err(error), Ok(_)) | (_, Err(error)) => Err(error),
    }
}

pub(super) fn restore_session(paths: &PenPaths) -> Result<bool, PenDesktopError> {
    let Some(contents) = read_optional(&paths.session_receipt)? else {
        if paths.session_backup_directory.exists() {
            return Err(PenDesktopError::OrphanBackup);
        }
        return Ok(false);
    };
    let receipt: SessionReceipt =
        serde_json::from_slice(&contents).map_err(PenDesktopError::ParseReceipt)?;
    if receipt.schema_version != SESSION_SCHEMA_VERSION
        || receipt.models.backup_file != "models.backup"
        || receipt.auth.backup_file != "auth.backup"
    {
        return Err(PenDesktopError::InvalidReceipt);
    }
    restore_document(paths, super::PenDocumentKind::Models, &receipt.models)?;
    restore_document(paths, super::PenDocumentKind::Auth, &receipt.auth)?;
    remove_file_if_present(&paths.session_backup_directory.join("models.backup"))?;
    remove_file_if_present(&paths.session_backup_directory.join("auth.backup"))?;
    match fs::remove_dir(&paths.session_backup_directory) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(PenDesktopError::RemoveBackup(error)),
    }
    remove_file_if_present(&paths.session_receipt)?;
    Ok(true)
}

fn restore_document(
    paths: &PenPaths,
    kind: super::PenDocumentKind,
    snapshot: &FileSnapshot,
) -> Result<(), PenDesktopError> {
    let target = match kind {
        super::PenDocumentKind::Models => &paths.models,
        super::PenDocumentKind::Auth => &paths.auth,
    };
    let current = read_optional(target)?;
    if file_matches_original(current.as_deref(), snapshot) {
        return Ok(());
    }
    if current
        .as_deref()
        .is_some_and(|contents| super::documents::sha256(contents) == snapshot.applied_file_sha256)
    {
        return restore_exact(paths, target, snapshot);
    }
    let Some(current) = current else {
        return Err(PenDesktopError::ManagedConfigurationChanged(target.clone()));
    };
    let current_entry = provider_entry(&current, kind)?;
    if hash_value(&current_entry)? != snapshot.applied_entry_sha256 {
        return Err(PenDesktopError::ManagedConfigurationChanged(target.clone()));
    }
    let original = read_snapshot(paths, snapshot)?;
    let replacement = merge_original_entry(&current, original.as_deref(), kind)?;
    write_private_atomic(target, &replacement)?;
    Ok(())
}

fn restore_exact(
    paths: &PenPaths,
    target: &std::path::Path,
    snapshot: &FileSnapshot,
) -> Result<(), PenDesktopError> {
    if let Some(original) = read_snapshot(paths, snapshot)? {
        write_private_atomic(target, &original)?;
    } else {
        remove_file_if_present(target)?;
    }
    Ok(())
}

fn read_snapshot(
    paths: &PenPaths,
    snapshot: &FileSnapshot,
) -> Result<Option<Vec<u8>>, PenDesktopError> {
    if !snapshot.existed {
        return Ok(None);
    }
    let contents = fs::read(paths.session_backup_directory.join(&snapshot.backup_file))
        .map_err(PenDesktopError::ReadBackup)?;
    if Some(super::documents::sha256(&contents)) != snapshot.original_sha256 {
        return Err(PenDesktopError::BackupHashMismatch);
    }
    Ok(Some(contents))
}

fn file_matches_original(current: Option<&[u8]>, snapshot: &FileSnapshot) -> bool {
    match (
        current,
        snapshot.existed,
        snapshot.original_sha256.as_deref(),
    ) {
        (None, false, _) => true,
        (Some(current), true, Some(hash)) => super::documents::sha256(current) == hash,
        _ => false,
    }
}
