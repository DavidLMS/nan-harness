use crate::commands::desktop::{remove_file_if_present, write_private_atomic};
use crate::commands::zed_desktop::ZedDesktopError;
use crate::commands::zed_desktop::documents::{
    backup_file_name, read_optional, remove_managed_settings, sha256,
};
use crate::commands::zed_desktop::paths::{SessionReceipt, ZedPaths};
use std::fs;
use std::io::ErrorKind;

pub(in crate::commands::zed_desktop) fn restore_session(
    paths: &ZedPaths,
) -> Result<bool, ZedDesktopError> {
    let Some(receipt) = super::receipt::read(&paths.session_receipt)? else {
        if paths.backup_directory.exists() {
            return Err(ZedDesktopError::OrphanBackup);
        }
        return Ok(false);
    };
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

pub(in crate::commands::zed_desktop) fn ensure_no_pending_session(
    paths: &ZedPaths,
) -> Result<(), ZedDesktopError> {
    if paths.session_receipt.exists() || paths.backup_directory.exists() {
        Err(ZedDesktopError::PendingRecovery)
    } else {
        Ok(())
    }
}

pub(super) fn restore_after(
    paths: &ZedPaths,
    result: Result<i32, ZedDesktopError>,
) -> Result<i32, ZedDesktopError> {
    match (result, restore_session(paths)) {
        (Ok(code), Ok(_)) => Ok(code),
        (Err(error), Ok(_)) | (_, Err(error)) => Err(error),
    }
}

pub(super) fn discard_unapplied_state(paths: &ZedPaths) -> Result<(), ZedDesktopError> {
    cleanup_session_state(paths)
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
