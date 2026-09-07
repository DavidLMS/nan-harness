//! The private backup of the profile configuration a managed session replaces.
//!
//! The backup is the capture of the original state: it holds the exact bytes,
//! the original permissions and the digest that restoration verifies. It always
//! lives at the fixed path the managed profile derives, so no name stored in a
//! receipt is ever joined into a filesystem path.

use super::super::{ChatGptDesktopError, profile::ManagedProfile};
use crate::commands::desktop::{reject_symlink, write_private_atomic};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

/// The state of the profile configuration before any managed write.
#[derive(Debug)]
pub(super) struct OriginalConfig {
    pub(super) bytes: Vec<u8>,
    /// Unix permission bits of the original file, restored with its bytes.
    pub(super) mode: Option<u32>,
}

impl OriginalConfig {
    pub(super) fn sha256(&self) -> String {
        sha256(&self.bytes)
    }
}

/// Reads the profile configuration that a managed session is about to replace.
pub(super) fn capture_original_config(
    profile: &ManagedProfile,
) -> Result<Option<OriginalConfig>, ChatGptDesktopError> {
    reject_symlink(&profile.config)?;
    match fs::read(&profile.config) {
        Ok(bytes) => Ok(Some(OriginalConfig {
            mode: file_mode(&profile.config)?,
            bytes,
        })),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(ChatGptDesktopError::ReadState(error)),
    }
}

#[cfg(unix)]
fn file_mode(path: &Path) -> Result<Option<u32>, ChatGptDesktopError> {
    use std::os::unix::fs::PermissionsExt as _;

    let metadata = fs::metadata(path).map_err(ChatGptDesktopError::ReadState)?;
    Ok(Some(metadata.permissions().mode()))
}

#[cfg(not(unix))]
fn file_mode(path: &Path) -> Result<Option<u32>, ChatGptDesktopError> {
    fs::metadata(path).map_err(ChatGptDesktopError::ReadState)?;
    Ok(None)
}

pub(super) fn write_backup(
    profile: &ManagedProfile,
    original: &OriginalConfig,
) -> Result<(), ChatGptDesktopError> {
    write_private_atomic(&profile.config_backup, &original.bytes).map_err(ChatGptDesktopError::from)
}

/// Reads the backed-up bytes and refuses any content the receipt does not
/// describe, so a tampered backup can never be written back over the profile.
pub(super) fn read_backup(
    profile: &ManagedProfile,
    expected_sha256: &str,
) -> Result<Vec<u8>, ChatGptDesktopError> {
    reject_symlink(&profile.config_backup)?;
    let bytes = match fs::read(&profile.config_backup) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(ChatGptDesktopError::MissingBackup);
        }
        Err(error) => return Err(ChatGptDesktopError::ReadState(error)),
    };
    if sha256(&bytes) != expected_sha256 {
        return Err(ChatGptDesktopError::BackupHashMismatch);
    }
    Ok(bytes)
}

/// Writes profile configuration privately, reapplying an original file mode.
pub(super) fn write_config(
    path: &Path,
    contents: &[u8],
    mode: Option<u32>,
) -> Result<(), ChatGptDesktopError> {
    write_private_atomic(path, contents)?;
    restore_mode(path, mode)
}

#[cfg(unix)]
fn restore_mode(path: &Path, mode: Option<u32>) -> Result<(), ChatGptDesktopError> {
    use std::os::unix::fs::PermissionsExt as _;

    // The profile directory stays owner-only, so restoring the user's original
    // file mode inside it cannot widen access beyond what they already had.
    let Some(mode) = mode else {
        return Ok(());
    };
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .map_err(ChatGptDesktopError::WriteState)
}

#[cfg(not(unix))]
fn restore_mode(_path: &Path, _mode: Option<u32>) -> Result<(), ChatGptDesktopError> {
    Ok(())
}

pub(super) fn sha256(payload: &[u8]) -> String {
    let digest = Sha256::digest(payload);
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut encoded, "{byte:02x}");
    }
    encoded
}
