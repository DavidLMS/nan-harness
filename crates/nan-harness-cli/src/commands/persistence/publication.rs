//! In-process publication and recovery for native configuration files and receipts.
use super::filesystem::write_private_file_with_permissions;
use super::{
    IntegrationState, PersistenceError, PersistenceManager, PreparedFileChange, permissions,
    read_optional,
};
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use tempfile::TempPath;

impl PreparedFileChange {
    pub(crate) fn read(
        path: PathBuf,
        replacement: Option<Vec<u8>>,
    ) -> Result<Self, PersistenceError> {
        Ok(Self {
            original: read_optional(&path)?,
            original_permissions: permissions(&path)?,
            path,
            replacement,
            replacement_permissions: None,
        })
    }
}

impl PersistenceManager {
    pub(super) fn prepare_integration_files(
        mut files: Vec<PreparedFileChange>,
        state: &IntegrationState,
        mut receipt: PreparedFileChange,
    ) -> Result<Vec<PreparedFileChange>, PersistenceError> {
        let payload = serde_json::to_vec_pretty(state).map_err(PersistenceError::SerializeState)?;
        receipt.replacement = Some(payload);
        files.push(receipt);
        Ok(files)
    }

    pub(crate) fn publish_configuration_files(
        &self,
        files: &[PreparedFileChange],
    ) -> Result<(), PersistenceError> {
        publish(files, &self.state_directory)
    }
}

pub(crate) fn publish(
    files: &[PreparedFileChange],
    state_directory: &Path,
) -> Result<(), PersistenceError> {
    // Keep private copies before the first mutation, so a failed restoration cannot
    // discard the only recoverable prior contents. These are not a crash journal.
    if files.is_empty() {
        return Ok(());
    }
    let backups = prepare_backups(files, state_directory)?;
    let mut published = Vec::new();
    for (index, file) in files.iter().enumerate() {
        let result = publish_one(file, index);
        match result {
            Ok(Publication::Published(permissions)) => published.push((index, permissions)),
            Ok(Publication::Unchanged) => {}
            Err(source) => return recover(files, &published, backups, source),
        }
    }
    Ok(())
}

enum Publication {
    Unchanged,
    Published(Option<fs::Permissions>),
}

fn publish_one(file: &PreparedFileChange, index: usize) -> Result<Publication, PersistenceError> {
    #[cfg(test)]
    checkpoint(index)?;
    #[cfg(not(test))]
    let _ = index;
    if !matches_snapshot(
        file,
        file.original.as_deref(),
        file.original_permissions.as_ref(),
    )? {
        return Err(PersistenceError::ManagedFileChanged(file.path.clone()));
    }
    if file.original == file.replacement {
        return Ok(Publication::Unchanged);
    }
    replace(
        file,
        file.replacement.as_deref(),
        file.replacement_permissions.as_ref(),
    )
    .map(Publication::Published)
}

fn matches_snapshot(
    file: &PreparedFileChange,
    contents: Option<&[u8]>,
    expected_permissions: Option<&fs::Permissions>,
) -> Result<bool, PersistenceError> {
    Ok(read_optional(&file.path)?.as_deref() == contents
        && permissions(&file.path)?.as_ref() == expected_permissions)
}

fn replace(
    file: &PreparedFileChange,
    contents: Option<&[u8]>,
    mode: Option<&fs::Permissions>,
) -> Result<Option<fs::Permissions>, PersistenceError> {
    match contents {
        Some(contents) => write_private_file_with_permissions(&file.path, contents, mode).map(Some),
        None => fs::remove_file(&file.path)
            .or_else(|error| {
                if error.kind() == std::io::ErrorKind::NotFound {
                    Ok(())
                } else {
                    Err(error)
                }
            })
            .map_err(|source| PersistenceError::RemoveFile {
                path: file.path.clone(),
                source,
            })
            .map(|()| None),
    }
}

fn prepare_backups(
    files: &[PreparedFileChange],
    directory: &Path,
) -> Result<Vec<TempPath>, PersistenceError> {
    nan_harness_private_fs::create_private_dir_all(directory)
        .map_err(PersistenceError::CreateStateDirectory)?;
    files
        .iter()
        .filter(|file| file.original != file.replacement)
        .map(|file| {
            let contents = recovery_snapshot(file)?;
            let mut backup = tempfile::Builder::new()
                .prefix(".nan-recovery-")
                .suffix(".json")
                .make_in(directory, nan_harness_private_fs::open_private_new)
                .map_err(|source| PersistenceError::WriteFile {
                    path: file.path.clone(),
                    source,
                })?;
            backup
                .write_all(&contents)
                .and_then(|()| backup.as_file().sync_all())
                .map_err(|source| PersistenceError::WriteFile {
                    path: file.path.clone(),
                    source,
                })?;
            Ok(backup.into_temp_path())
        })
        .collect()
}

// A private, self-describing prior snapshot, retained only if in-process recovery
// fails. It is never replayed automatically and makes no crash-recovery promise.
fn recovery_snapshot(file: &PreparedFileChange) -> Result<Vec<u8>, PersistenceError> {
    #[cfg(unix)]
    let mode = {
        use std::os::unix::fs::PermissionsExt as _;
        file.original_permissions
            .as_ref()
            .map(fs::Permissions::mode)
    };
    #[cfg(not(unix))]
    let mode: Option<u32> = None;
    serde_json::to_vec(&serde_json::json!({
        "path": file.path, "original": file.original, "unixMode": mode,
        "readonly": file.original_permissions.as_ref().map(fs::Permissions::readonly),
    }))
    .map_err(PersistenceError::SerializeState)
}

fn recover(
    files: &[PreparedFileChange],
    published: &[(usize, Option<fs::Permissions>)],
    backups: Vec<TempPath>,
    source: PersistenceError,
) -> Result<(), PersistenceError> {
    let mut failures = 0;
    for (index, expected) in published.iter().rev() {
        let file = &files[*index];
        // Published replacements are private; an intervening permission or content
        // edit belongs to the user, including creation after a published removal.
        let restored = restore_one(file, expected.as_ref(), *index);
        if restored.is_err() {
            failures += 1;
        }
    }
    if failures == 0 {
        return Err(source);
    }
    let recovery_files = backups
        .into_iter()
        .map(|backup| {
            // TempPath::keep can fail to disable automatic cleanup on some platforms;
            // retain its path object in that case rather than deleting the backup.
            match backup.keep() {
                Ok(path) => path,
                Err(error) => {
                    let path = error.path.to_path_buf();
                    std::mem::forget(error.path);
                    path
                }
            }
        })
        .collect();
    Err(PersistenceError::RollbackIncomplete {
        source: Box::new(source),
        failures,
        recovery_files,
    })
}

fn restore_one(
    file: &PreparedFileChange,
    expected: Option<&fs::Permissions>,
    index: usize,
) -> Result<(), PersistenceError> {
    #[cfg(test)]
    RECOVERY_HOOK.with_borrow_mut(|hook| hook.as_mut().map_or(Ok(()), |hook| hook(index)))?;
    #[cfg(not(test))]
    let _ = index;
    if !matches_snapshot(file, file.replacement.as_deref(), expected)? {
        return Err(PersistenceError::ManagedFileChanged(file.path.clone()));
    }
    replace(
        file,
        file.original.as_deref(),
        file.original_permissions.as_ref(),
    )
    .map(|_| ())
}

#[cfg(test)]
type PublicationHook =
    std::cell::RefCell<Option<Box<dyn FnMut(usize) -> Result<(), PersistenceError>>>>;
#[cfg(test)]
thread_local! {
    pub(crate) static PUBLICATION_HOOK: PublicationHook = std::cell::RefCell::new(None);
    pub(crate) static RECOVERY_HOOK: PublicationHook = std::cell::RefCell::new(None);
}
#[cfg(test)]
fn checkpoint(index: usize) -> Result<(), PersistenceError> {
    PUBLICATION_HOOK.with_borrow_mut(|hook| hook.as_mut().map_or(Ok(()), |hook| hook(index)))
}
