//! The private backup of the profile configuration a managed session replaces.
//!
//! The backup always lives at the fixed path the managed profile derives, so no
//! name stored in a receipt is ever joined into a filesystem path, and it is
//! written under the private-file contract like every other managed file.

use super::super::{ChatGptDesktopError, profile::ManagedProfile};
use crate::commands::desktop::{reject_symlink, write_private_atomic};
use sha2::{Digest, Sha256};
use std::fs;

pub(super) fn capture_original_config(
    profile: &ManagedProfile,
) -> Result<Option<Vec<u8>>, ChatGptDesktopError> {
    reject_symlink(&profile.config)?;
    match fs::read(&profile.config) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(ChatGptDesktopError::ReadState(error)),
    }
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

pub(super) fn write_private(
    path: &std::path::Path,
    contents: &[u8],
) -> Result<(), ChatGptDesktopError> {
    write_private_atomic(path, contents).map_err(ChatGptDesktopError::from)
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
