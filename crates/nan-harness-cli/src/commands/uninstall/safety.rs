use super::UninstallError;
use std::env;
use std::fs;
use std::path::Path;

pub(super) fn validate_data_directory(path: &Path) -> Result<(), UninstallError> {
    if !path.is_absolute()
        || path.parent().is_none()
        || path
            .parent()
            .is_some_and(|parent| parent.parent().is_none())
        || env::var_os(home_environment_variable()).is_some_and(|home| Path::new(&home) == path)
    {
        return Err(UninstallError::UnsafeDataDirectory(path.to_path_buf()));
    }
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(UninstallError::UnsafeDataDirectory(path.to_path_buf()))
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(UninstallError::InspectDataDirectory {
            path: path.to_path_buf(),
            source,
        }),
    }
}

pub(super) fn ensure_no_pending_desktop_session(
    data_directory: &Path,
) -> Result<(), UninstallError> {
    for (surface, relative) in [
        (
            "ChatGPT Desktop",
            "chatgpt-desktop/profile/.nan-session.json",
        ),
        ("Claude Desktop", "claude-desktop-receipt.json"),
        ("Hermes Desktop", "hermes-desktop/session.json"),
        ("Pen Desktop", "pen-desktop/session.json"),
    ] {
        let receipt = data_directory.join(relative);
        match fs::symlink_metadata(&receipt) {
            Ok(_) => return Err(UninstallError::DesktopRecoveryRequired(surface)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(UninstallError::InspectDataDirectory {
                    path: receipt,
                    source,
                });
            }
        }
    }
    Ok(())
}

#[cfg(windows)]
const fn home_environment_variable() -> &'static str {
    "USERPROFILE"
}

#[cfg(not(windows))]
const fn home_environment_variable() -> &'static str {
    "HOME"
}
