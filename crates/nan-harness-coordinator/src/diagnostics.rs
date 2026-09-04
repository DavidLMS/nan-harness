use crate::CoordinatorError;
use crate::paths::private_directory;
use nan_harness_private_fs::{open_private_read, open_private_truncate};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticsStatus {
    pub enabled: bool,
    pub capture_id: Option<String>,
    pub enabled_at_unix_seconds: Option<u64>,
    pub directory: PathBuf,
    pub bytes: u64,
    pub incomplete_files: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CaptureSettings {
    pub(crate) schema_version: u8,
    pub(crate) enabled: bool,
    pub(crate) capture_id: Option<String>,
    pub(crate) enabled_at_unix_seconds: Option<u64>,
}

impl Default for CaptureSettings {
    fn default() -> Self {
        Self {
            schema_version: 1,
            enabled: false,
            capture_id: None,
            enabled_at_unix_seconds: None,
        }
    }
}

/// Enables a new private diagnostic capture, or returns the active capture.
///
/// # Errors
///
/// Returns an error if the private settings or capture directory cannot be
/// created or updated.
pub fn enable_diagnostics() -> Result<DiagnosticsStatus, CoordinatorError> {
    let directory = diagnostics_directory()?;
    if let Ok(settings) = read_settings(&directory)
        && settings.enabled
    {
        return status_from(directory, settings);
    }
    let capture_id = capture_id()?;
    private_directory(&directory.join("captures").join(&capture_id))?;
    let settings = CaptureSettings {
        schema_version: 1,
        enabled: true,
        capture_id: Some(capture_id),
        enabled_at_unix_seconds: Some(now_seconds()),
    };
    write_settings(&directory, &settings)?;
    status_from(directory, settings)
}

/// Disables diagnostic capture without interrupting in-flight writers.
///
/// # Errors
///
/// Returns an error if private settings cannot be updated.
pub fn disable_diagnostics() -> Result<DiagnosticsStatus, CoordinatorError> {
    let directory = diagnostics_directory()?;
    let mut settings = read_settings(&directory).unwrap_or_default();
    settings.enabled = false;
    write_settings(&directory, &settings)?;
    status_from(directory, settings)
}

/// Reads diagnostic capture state and its current disk usage.
///
/// # Errors
///
/// Returns an error if diagnostic state or capture files cannot be inspected.
pub fn read_diagnostics_status() -> Result<DiagnosticsStatus, CoordinatorError> {
    let directory = diagnostics_directory()?;
    let settings = read_settings(&directory).unwrap_or_default();
    status_from(directory, settings)
}

/// Disables capture and removes all completed diagnostic capture files.
///
/// # Errors
///
/// Returns [`CoordinatorError::CaptureBusy`] while an in-flight writer still
/// owns the shared capture lock, or a state error when files cannot be removed.
pub fn purge_diagnostics() -> Result<DiagnosticsStatus, CoordinatorError> {
    let status = disable_diagnostics()?;
    purge_captures(&status.directory)?;
    read_diagnostics_status()
}

fn purge_captures(directory: &Path) -> Result<(), CoordinatorError> {
    let lock_path = directory.join("capture.lock");
    let lock =
        open_private_truncate(&lock_path).map_err(|source| state_error(&lock_path, source))?;
    lock.try_lock().map_err(|_| CoordinatorError::CaptureBusy)?;
    let captures = directory.join("captures");
    match fs::remove_dir_all(&captures) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => return Err(state_error(&captures, source)),
    }
    private_directory(&captures)?;
    drop(lock);
    Ok(())
}

pub(crate) fn active_capture() -> Option<(PathBuf, CaptureSettings)> {
    let directory = crate::config_directory().ok()?.join("diagnostics");
    let settings = read_settings(&directory).ok()?;
    settings.enabled.then_some((directory, settings))
}

fn diagnostics_directory() -> Result<PathBuf, CoordinatorError> {
    let directory = crate::config_directory()?.join("diagnostics");
    private_directory(&directory)?;
    Ok(directory)
}

fn read_settings(directory: &Path) -> Result<CaptureSettings, CoordinatorError> {
    let path = directory.join("settings.json");
    let (file, _) = open_private_read(&path).map_err(|source| state_error(&path, source))?;
    let settings: CaptureSettings =
        serde_json::from_reader(file).map_err(CoordinatorError::Encode)?;
    if settings.schema_version != 1 {
        return Err(CoordinatorError::Protocol(
            "unsupported diagnostic settings version",
        ));
    }
    Ok(settings)
}

fn write_settings(directory: &Path, settings: &CaptureSettings) -> Result<(), CoordinatorError> {
    let path = directory.join("settings.json");
    let payload = serde_json::to_vec_pretty(settings)?;
    let mut file = open_private_truncate(&path).map_err(|source| state_error(&path, source))?;
    file.write_all(&payload)
        .and_then(|()| file.write_all(b"\n"))
        .and_then(|()| file.sync_all())
        .map_err(|source| state_error(&path, source))
}

fn status_from(
    directory: PathBuf,
    settings: CaptureSettings,
) -> Result<DiagnosticsStatus, CoordinatorError> {
    let captures = directory.join("captures");
    let (bytes, incomplete_files) = directory_usage(&captures)?;
    Ok(DiagnosticsStatus {
        enabled: settings.enabled,
        capture_id: settings.capture_id,
        enabled_at_unix_seconds: settings.enabled_at_unix_seconds,
        directory,
        bytes,
        incomplete_files,
    })
}

fn directory_usage(path: &Path) -> Result<(u64, usize), CoordinatorError> {
    let mut bytes = 0_u64;
    let mut incomplete = 0_usize;
    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok((0, 0)),
        Err(source) => return Err(state_error(path, source)),
    };
    for entry in entries {
        let entry = entry.map_err(|source| state_error(path, source))?;
        let metadata = entry
            .metadata()
            .map_err(|source| state_error(&entry.path(), source))?;
        if metadata.is_dir() {
            let (child_bytes, child_incomplete) = directory_usage(&entry.path())?;
            bytes = bytes.saturating_add(child_bytes);
            incomplete = incomplete.saturating_add(child_incomplete);
        } else {
            bytes = bytes.saturating_add(metadata.len());
            incomplete += usize::from(
                entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "incomplete"),
            );
        }
    }
    Ok((bytes, incomplete))
}

fn capture_id() -> Result<String, CoordinatorError> {
    let mut random = [0_u8; 8];
    getrandom::fill(&mut random)?;
    Ok(format!(
        "capture-{}-{}",
        now_seconds(),
        u64::from_le_bytes(random)
    ))
}

fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn state_error(path: &Path, source: std::io::Error) -> CoordinatorError {
    CoordinatorError::State {
        path: path.to_path_buf(),
        source,
    }
}

#[cfg(test)]
mod tests {
    use super::{directory_usage, purge_captures};
    use crate::CoordinatorError;
    use nan_harness_private_fs::open_private_truncate;
    use std::fs;

    #[test]
    fn purge_rejects_active_writers_and_removes_only_captures() {
        let temporary = tempfile::tempdir().expect("temporary directory should exist");
        let diagnostics = temporary.path().join("diagnostics");
        let captures = diagnostics.join("captures/capture-one");
        fs::create_dir_all(&captures).expect("capture directory should exist");
        fs::write(captures.join("request.jsonl"), b"payload")
            .expect("capture fixture should be written");
        fs::write(diagnostics.join("settings.json"), b"settings")
            .expect("settings fixture should be written");

        let lock = open_private_truncate(&diagnostics.join("capture.lock"))
            .expect("capture lock should open");
        lock.try_lock_shared()
            .expect("shared writer lock should hold");
        assert!(matches!(
            purge_captures(&diagnostics),
            Err(CoordinatorError::CaptureBusy)
        ));
        drop(lock);

        purge_captures(&diagnostics).expect("idle captures should purge");
        assert!(diagnostics.join("settings.json").exists());
        assert!(diagnostics.join("captures").is_dir());
        assert_eq!(
            directory_usage(&diagnostics.join("captures")).expect("usage should be readable"),
            (0, 0)
        );
    }
}
