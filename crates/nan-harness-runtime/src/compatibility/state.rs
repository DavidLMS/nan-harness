use super::{CompatibilityError, VerificationManifest};
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tempfile::Builder as TempFileBuilder;

const CONFIG_DIRECTORY_ENVIRONMENT_VARIABLE: &str = "NAN_HARNESS_CONFIG_DIR";
const STATE_SCHEMA_VERSION: u8 = 2;
const CHECK_INTERVAL: Duration = Duration::from_hours(1);
pub(super) const STATE_FILE_NAME: &str = "compatibility.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CompatibilityState {
    pub(super) schema_version: u8,
    pub(super) last_checked_unix_seconds: Option<u64>,
    pub(super) cached_manifest: Option<VerificationManifest>,
}

impl Default for CompatibilityState {
    fn default() -> Self {
        Self {
            schema_version: STATE_SCHEMA_VERSION,
            last_checked_unix_seconds: None,
            cached_manifest: None,
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct CompatibilityStateStore {
    directory: PathBuf,
    path: PathBuf,
}

impl CompatibilityStateStore {
    pub(super) fn new(directory: impl Into<PathBuf>) -> Self {
        let directory = directory.into();
        let path = directory.join(STATE_FILE_NAME);
        Self { directory, path }
    }

    pub(super) fn from_environment() -> Result<Self, CompatibilityError> {
        if let Some(directory) = env::var_os(CONFIG_DIRECTORY_ENVIRONMENT_VARIABLE) {
            return Ok(Self::new(directory));
        }
        platform_config_directory()
            .map(Self::new)
            .ok_or(CompatibilityError::MissingConfigDirectory)
    }

    pub(super) fn load(&self) -> Result<CompatibilityState, CompatibilityError> {
        match fs::read(&self.path) {
            Ok(contents) => {
                let state: CompatibilityState =
                    serde_json::from_slice(&contents).map_err(CompatibilityError::ParseState)?;
                if state.schema_version != STATE_SCHEMA_VERSION {
                    return Err(CompatibilityError::UnsupportedStateSchema(
                        state.schema_version,
                    ));
                }
                Ok(state)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(CompatibilityState::default())
            }
            Err(error) => Err(CompatibilityError::ReadState(error)),
        }
    }

    pub(super) fn save(&self, state: &CompatibilityState) -> Result<(), CompatibilityError> {
        fs::create_dir_all(&self.directory).map_err(CompatibilityError::CreateConfigDirectory)?;
        let payload =
            serde_json::to_vec_pretty(state).map_err(CompatibilityError::SerializeState)?;
        atomic_write(&self.path, &payload).map_err(CompatibilityError::WriteState)
    }
}

pub(super) fn cache_is_fresh(state: &CompatibilityState) -> bool {
    let Ok(now) = unix_seconds() else {
        return false;
    };
    cache_is_fresh_at(state, now)
}

pub(super) fn cache_is_fresh_at(state: &CompatibilityState, now: u64) -> bool {
    let Some(last_checked) = state.last_checked_unix_seconds else {
        return false;
    };
    now.checked_sub(last_checked)
        .is_some_and(|age| age < CHECK_INTERVAL.as_secs())
        && state.cached_manifest.is_some()
}

pub(super) fn unix_seconds() -> Result<u64, CompatibilityError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(CompatibilityError::SystemClock)
}

fn atomic_write(path: &Path, payload: &[u8]) -> Result<(), std::io::Error> {
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "path has no parent")
    })?;
    let mut temporary = TempFileBuilder::new().prefix(".nan-").tempfile_in(parent)?;
    temporary.write_all(payload)?;
    temporary.write_all(b"\n")?;
    temporary.flush()?;
    temporary.as_file().sync_all()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        temporary
            .as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    temporary.persist(path).map_err(|error| error.error)?;
    Ok(())
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
