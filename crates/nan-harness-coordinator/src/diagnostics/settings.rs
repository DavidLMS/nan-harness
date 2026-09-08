use super::{CaptureSettings, capture_id, private_directory, state_error};
use crate::CoordinatorError;
use nan_harness_private_fs::{open_private_new, open_private_read, open_private_truncate};
use std::fs::{self, File};
use std::io::{Read as _, Write as _};
use std::path::Path;
use tempfile::{Builder, NamedTempFile};

pub(super) fn lock_settings(directory: &Path) -> Result<File, CoordinatorError> {
    let path = directory.join("settings.lock");
    let lock = open_private_truncate(&path).map_err(|source| state_error(&path, source))?;
    lock.lock().map_err(|source| state_error(&path, source))?;
    Ok(lock)
}

pub(super) fn read_settings(directory: &Path) -> Result<CaptureSettings, CoordinatorError> {
    read_payload(directory)?.map_or_else(|| Ok(CaptureSettings::default()), |bytes| parse(&bytes))
}

fn read_payload(directory: &Path) -> Result<Option<Vec<u8>>, CoordinatorError> {
    let path = directory.join("settings.json");
    let mut file = match open_private_read(&path) {
        Ok((file, _)) => file,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            // A dangling symlink is existing state, not a missing settings file.
            return match fs::symlink_metadata(&path) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
                _ => Err(state_error(&path, source)),
            };
        }
        Err(source) => return Err(state_error(&path, source)),
    };
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|source| state_error(&path, source))?;
    Ok(Some(bytes))
}

fn parse(bytes: &[u8]) -> Result<CaptureSettings, CoordinatorError> {
    // JSON errors may quote user-owned values. Report only fixed safe text.
    let settings: CaptureSettings = serde_json::from_slice(bytes).map_err(|_| {
        CoordinatorError::Protocol("invalid diagnostic settings; use diagnostics off to recover")
    })?;
    if settings.schema_version != 1 {
        return Err(CoordinatorError::Protocol(
            "unsupported diagnostic settings version; use diagnostics off to recover",
        ));
    }
    Ok(settings)
}

pub(super) fn disable_settings(
    directory: &Path,
) -> Result<(CaptureSettings, bool), CoordinatorError> {
    let (mut settings, recovered) = match read_payload(directory)? {
        None => (CaptureSettings::default(), false),
        Some(bytes) => {
            if let Ok(settings) = parse(&bytes) {
                (settings, false)
            } else {
                backup_settings(directory, &bytes)?;
                (CaptureSettings::default(), true)
            }
        }
    };
    settings.enabled = false;
    write_settings(directory, &settings)?;
    Ok((settings, recovered))
}

fn backup_settings(directory: &Path, bytes: &[u8]) -> Result<(), CoordinatorError> {
    // Recovery evidence is not a capture and survives ordinary capture purge.
    let backups = directory.join("settings-backups");
    private_directory(&backups)?;
    let path = backups.join(format!("{}.json", capture_id()?));
    let temporary = prepare_private_file(&backups, bytes)?;
    temporary
        .persist_noclobber(&path)
        .map_err(|error| state_error(&path, error.error))?;
    Ok(())
}

pub(super) fn write_settings(
    directory: &Path,
    settings: &CaptureSettings,
) -> Result<(), CoordinatorError> {
    let path = directory.join("settings.json");
    let mut payload = serde_json::to_vec_pretty(settings)?;
    payload.push(b'\n');
    let temporary = prepare_private_file(directory, &payload)?;
    // Rust's rename supports replacing an open destination on modern Windows;
    // tempfile's persist uses only MoveFileExW, which rejects that case.
    fs::rename(temporary.path(), &path).map_err(|source| state_error(&path, source))?;
    Ok(())
}

fn prepare_private_file(
    directory: &Path,
    payload: &[u8],
) -> Result<NamedTempFile, CoordinatorError> {
    let mut temporary = Builder::new()
        .prefix(".settings-")
        .make_in(directory, open_private_new)
        .map_err(|source| state_error(directory, source))?;
    temporary
        .write_all(payload)
        .and_then(|()| temporary.as_file().sync_all())
        .map_err(|source| state_error(directory, source))?;
    Ok(temporary)
}

#[cfg(test)]
mod tests;
