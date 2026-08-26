use super::PersistenceError;
use nan_harness_private_fs::open_private_new;
use std::env;
use std::fs::{self, Permissions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use tempfile::Builder as TempFileBuilder;

const CONFIG_DIRECTORY_ENVIRONMENT_VARIABLE: &str = "NAN_HARNESS_CONFIG_DIR";

pub(super) fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, PersistenceError> {
    match fs::read(path) {
        Ok(contents) => Ok(Some(contents)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(PersistenceError::ReadFile {
            path: path.to_path_buf(),
            source,
        }),
    }
}

pub(super) fn permissions(path: &Path) -> Result<Option<Permissions>, PersistenceError> {
    match fs::metadata(path) {
        Ok(metadata) => Ok(Some(metadata.permissions())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(PersistenceError::ReadFile {
            path: path.to_path_buf(),
            source,
        }),
    }
}

pub(crate) fn write_private_file(
    path: &Path,
    payload: &[u8],
    permissions: Option<&Permissions>,
) -> Result<(), PersistenceError> {
    let parent = path
        .parent()
        .ok_or_else(|| PersistenceError::InvalidPath(path.to_path_buf()))?;
    fs::create_dir_all(parent).map_err(|source| PersistenceError::CreateDirectory {
        path: parent.to_path_buf(),
        source,
    })?;
    let mut temporary = TempFileBuilder::new()
        .prefix(".nan-")
        .make_in(parent, open_private_new)
        .map_err(|source| PersistenceError::WriteFile {
            path: path.to_path_buf(),
            source,
        })?;
    temporary
        .write_all(payload)
        .and_then(|()| temporary.flush())
        .and_then(|()| temporary.as_file().sync_all())
        .map_err(|source| PersistenceError::WriteFile {
            path: path.to_path_buf(),
            source,
        })?;
    if let Some(permissions) = permissions {
        set_permissions(temporary.as_file(), permissions).map_err(|source| {
            PersistenceError::WriteFile {
                path: path.to_path_buf(),
                source,
            }
        })?;
    }
    temporary
        .persist(path)
        .map_err(|error| PersistenceError::WriteFile {
            path: path.to_path_buf(),
            source: error.error,
        })?;
    Ok(())
}

fn set_permissions(file: &fs::File, permissions: &Permissions) -> Result<(), std::io::Error> {
    file.set_permissions(permissions.clone())
}

pub(super) fn rollback_file(
    path: &Path,
    original: Option<&[u8]>,
    permissions: Option<&Permissions>,
) {
    match original {
        Some(contents) => {
            let _ = write_private_file(path, contents, permissions);
        }
        None => {
            let _ = fs::remove_file(path);
        }
    }
}

pub(super) fn file_name(path: &Path) -> Result<String, PersistenceError> {
    path.file_name()
        .and_then(|value| value.to_str())
        .map(ToOwned::to_owned)
        .ok_or_else(|| PersistenceError::InvalidPath(path.to_path_buf()))
}

pub(crate) fn config_directory() -> Option<PathBuf> {
    if let Some(directory) = env::var_os(CONFIG_DIRECTORY_ENVIRONMENT_VARIABLE) {
        return Some(PathBuf::from(directory));
    }
    #[cfg(target_os = "macos")]
    {
        home_directory().map(|home| home.join("Library/Application Support/nan-harness"))
    }
    #[cfg(target_os = "windows")]
    {
        env::var_os("APPDATA")
            .map(PathBuf::from)
            .map(|directory| directory.join("nan-harness"))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .map(|directory| directory.join("nan-harness"))
            .or_else(|| home_directory().map(|home| home.join(".config/nan-harness")))
    }
}

pub(super) fn home_directory() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        env::var_os("USERPROFILE").map(PathBuf::from)
    }
    #[cfg(not(windows))]
    {
        env::var_os("HOME").map(PathBuf::from)
    }
}
