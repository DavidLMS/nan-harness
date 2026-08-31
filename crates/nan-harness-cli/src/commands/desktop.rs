use nan_harness_private_fs::{PrivatePathKind, open_private_new, restrict_path};
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::Write;
use std::path::Path;
use thiserror::Error;

#[derive(Debug)]
pub(crate) struct DesktopSessionLock {
    file: File,
}

impl DesktopSessionLock {
    pub(crate) fn acquire(directory: &Path) -> Result<Self, DesktopStateError> {
        create_private_directory(directory)?;
        let lock_path = directory.join("session.lock");
        reject_symlink(&lock_path)?;
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(DesktopStateError::Io)?;
        nan_harness_private_fs::restrict_file(&mut file).map_err(DesktopStateError::Io)?;
        match file.try_lock() {
            Ok(()) => Ok(Self { file }),
            Err(TryLockError::WouldBlock) => Err(DesktopStateError::AlreadyLocked),
            Err(TryLockError::Error(error)) => Err(DesktopStateError::Io(error)),
        }
    }
}

impl Drop for DesktopSessionLock {
    fn drop(&mut self) {
        let _ = File::unlock(&self.file);
    }
}

pub(crate) fn create_private_directory(path: &Path) -> Result<(), DesktopStateError> {
    reject_symlink(path)?;
    nan_harness_private_fs::create_private_dir_all(path).map_err(DesktopStateError::Io)?;
    restrict_path(path, PrivatePathKind::Directory).map_err(DesktopStateError::Io)
}

pub(crate) fn write_private_atomic(path: &Path, contents: &[u8]) -> Result<(), DesktopStateError> {
    let parent = path.parent().ok_or(DesktopStateError::InvalidPath)?;
    create_private_directory(parent)?;
    reject_symlink(path)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(DesktopStateError::Io)?;
    nan_harness_private_fs::restrict_file(temporary.as_file_mut())
        .map_err(DesktopStateError::Io)?;
    temporary
        .write_all(contents)
        .and_then(|()| temporary.flush())
        .map_err(DesktopStateError::Io)?;
    temporary
        .persist(path)
        .map_err(|error| DesktopStateError::Io(error.error))?;
    restrict_path(path, PrivatePathKind::File).map_err(DesktopStateError::Io)
}

pub(crate) fn create_private_new(path: &Path) -> Result<File, DesktopStateError> {
    let parent = path.parent().ok_or(DesktopStateError::InvalidPath)?;
    create_private_directory(parent)?;
    reject_symlink(path)?;
    open_private_new(path).map_err(DesktopStateError::Io)
}

pub(crate) fn remove_file_if_present(path: &Path) -> Result<(), DesktopStateError> {
    reject_symlink(path)?;
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(DesktopStateError::Io(error)),
    }
}

pub(crate) fn reject_symlink(path: &Path) -> Result<(), DesktopStateError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(DesktopStateError::Symlink),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(DesktopStateError::Io(error)),
    }
}

#[derive(Debug, Error)]
pub(crate) enum DesktopStateError {
    #[error("another managed desktop session is already active")]
    AlreadyLocked,
    #[error("the managed desktop state contains an unsafe symbolic link")]
    Symlink,
    #[error("the managed desktop state path is invalid")]
    InvalidPath,
    #[error("managed desktop state operation failed: {0}")]
    Io(std::io::Error),
}

impl DesktopStateError {
    pub(crate) const fn code(&self) -> &'static str {
        match self {
            Self::AlreadyLocked => "NH-DESKTOP-001",
            Self::Symlink | Self::InvalidPath => "NH-DESKTOP-002",
            Self::Io(_) => "NH-DESKTOP-003",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DesktopSessionLock, create_private_directory, write_private_atomic};

    #[test]
    fn desktop_session_lock_is_exclusive() {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let state = directory.path().join("desktop");
        let first = DesktopSessionLock::acquire(&state).expect("first lock should succeed");
        assert!(DesktopSessionLock::acquire(&state).is_err());
        drop(first);
        DesktopSessionLock::acquire(&state).expect("released lock should be reusable");
    }

    #[cfg(unix)]
    #[test]
    fn desktop_state_uses_owner_only_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let state = directory.path().join("desktop");
        let file = state.join("session.json");
        create_private_directory(&state).expect("directory should be private");
        write_private_atomic(&file, b"{}\n").expect("file should be private");

        assert_eq!(
            std::fs::metadata(&state)
                .expect("directory metadata")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(file)
                .expect("file metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}
