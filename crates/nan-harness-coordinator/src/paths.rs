use crate::CoordinatorError;
use nan_harness_private_fs::{PrivatePathKind, create_private_dir_all, restrict_path};
use std::env;
use std::path::{Path, PathBuf};

const CONFIG_DIRECTORY_ENVIRONMENT: &str = "NAN_HARNESS_CONFIG_DIR";
const FORCE_MANAGED_PROCESS_ENVIRONMENT: &str = "NAN_HARNESS_INTERNAL_MANAGED_PROCESS";

/// Returns the private per-user NaN Harness configuration directory.
///
/// # Errors
///
/// Returns [`CoordinatorError::MissingConfigDirectory`] when the platform does
/// not expose a usable user configuration location.
pub fn config_directory() -> Result<PathBuf, CoordinatorError> {
    env::var_os(CONFIG_DIRECTORY_ENVIRONMENT)
        .map(PathBuf::from)
        .or_else(platform_config_directory)
        .ok_or(CoordinatorError::MissingConfigDirectory)
}

pub(crate) fn private_directory(path: &Path) -> Result<(), CoordinatorError> {
    create_private_dir_all(path).map_err(|source| CoordinatorError::State {
        path: path.to_path_buf(),
        source,
    })?;
    restrict_path(path, PrivatePathKind::Directory).map_err(|source| CoordinatorError::State {
        path: path.to_path_buf(),
        source,
    })
}

pub(crate) fn is_managed_process() -> bool {
    if env::var_os(FORCE_MANAGED_PROCESS_ENVIRONMENT).is_some() {
        return true;
    }
    env::current_exe()
        .ok()
        .and_then(|path| {
            path.file_stem()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .is_some_and(|name| matches!(name.as_str(), "nanh" | "nan-harness"))
}

fn platform_config_directory() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        env::var_os("HOME")
            .map(PathBuf::from)
            .map(|home| home.join("Library/Application Support/nan-harness"))
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
            .or_else(|| {
                env::var_os("HOME")
                    .map(PathBuf::from)
                    .map(|home| home.join(".config/nan-harness"))
            })
    }
}
