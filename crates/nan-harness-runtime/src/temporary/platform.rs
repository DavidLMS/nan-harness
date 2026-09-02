use super::TemporaryError;
use nan_harness_private_fs::{PrivatePathKind, restrict_path};
#[cfg(not(any(unix, windows)))]
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

#[cfg(windows)]
pub(super) fn windows_user_home() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

#[cfg(not(windows))]
pub(super) fn windows_user_home() -> Option<PathBuf> {
    None
}

#[cfg(unix)]
pub(super) fn link_entry(source: &Path, target: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(source, target)
}

#[cfg(windows)]
pub(super) fn link_entry(source: &Path, target: &Path) -> std::io::Result<()> {
    use std::os::windows::fs::{symlink_dir, symlink_file};

    if std::fs::metadata(source)?.is_dir() {
        symlink_dir(source, target)
    } else {
        symlink_file(source, target)
    }
}

#[cfg(not(any(unix, windows)))]
pub(super) fn link_entry(_source: &Path, _target: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        ErrorKind::Unsupported,
        "configuration overlays require symbolic link support",
    ))
}

pub(super) fn restrict_directory(path: &Path) -> Result<(), TemporaryError> {
    restrict_path(path, PrivatePathKind::Directory).map_err(|source| TemporaryError::Permissions {
        path: path.to_path_buf(),
        source,
    })
}
