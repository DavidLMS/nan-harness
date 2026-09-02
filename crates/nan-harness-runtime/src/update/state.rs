use super::{CONFIG_DIRECTORY_ENVIRONMENT_VARIABLE, ReleaseManifest, UpdateError};
use semver::Version;
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tempfile::Builder as TempFileBuilder;

const UPDATE_STATE_SCHEMA_VERSION: u8 = 1;
const CHECK_INTERVAL: Duration = Duration::from_hours(1);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct UpdateState {
    schema_version: u8,
    pub(super) last_checked_unix_seconds: Option<u64>,
    pub(super) skipped_version: Option<Version>,
    pub(super) cached_release: Option<ReleaseManifest>,
}

impl Default for UpdateState {
    fn default() -> Self {
        Self {
            schema_version: UPDATE_STATE_SCHEMA_VERSION,
            last_checked_unix_seconds: None,
            skipped_version: None,
            cached_release: None,
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct UpdateStateStore {
    directory: PathBuf,
    path: PathBuf,
}

impl UpdateStateStore {
    pub(super) fn new(directory: impl Into<PathBuf>) -> Self {
        let directory = directory.into();
        let path = directory.join("update.json");
        Self { directory, path }
    }

    pub(super) fn from_environment() -> Result<Self, UpdateError> {
        if let Some(directory) = env::var_os(CONFIG_DIRECTORY_ENVIRONMENT_VARIABLE) {
            return Ok(Self::new(directory));
        }
        platform_config_directory()
            .map(Self::new)
            .ok_or(UpdateError::MissingConfigDirectory)
    }

    pub(super) fn load(&self) -> Result<UpdateState, UpdateError> {
        match fs::read(&self.path) {
            Ok(contents) => {
                let state: UpdateState =
                    serde_json::from_slice(&contents).map_err(UpdateError::ParseState)?;
                if state.schema_version != UPDATE_STATE_SCHEMA_VERSION {
                    return Err(UpdateError::UnsupportedStateSchema(state.schema_version));
                }
                Ok(state)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(UpdateState::default())
            }
            Err(error) => Err(UpdateError::ReadState(error)),
        }
    }

    pub(super) fn save(&self, state: &UpdateState) -> Result<(), UpdateError> {
        fs::create_dir_all(&self.directory).map_err(UpdateError::CreateConfigDirectory)?;
        let payload = serde_json::to_vec_pretty(state).map_err(UpdateError::SerializeState)?;
        atomic_write(&self.path, &payload).map_err(UpdateError::WriteState)
    }
}

pub(super) fn cache_is_fresh(state: &UpdateState) -> bool {
    let Ok(now) = unix_seconds() else {
        return false;
    };
    cache_is_fresh_at(state, now)
}

fn cache_is_fresh_at(state: &UpdateState, now: u64) -> bool {
    let Some(last_checked) = state.last_checked_unix_seconds else {
        return false;
    };
    now.saturating_sub(last_checked) < CHECK_INTERVAL.as_secs() && state.cached_release.is_some()
}

pub(super) fn unix_seconds() -> Result<u64, UpdateError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(UpdateError::SystemClock)
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

#[cfg(test)]
mod tests {
    use super::{UpdateState, cache_is_fresh_at};
    use crate::update::tests::manifest;

    #[test]
    fn cached_update_results_expire_after_one_hour() {
        let checked_at = 10_000;
        let state = UpdateState {
            last_checked_unix_seconds: Some(checked_at),
            cached_release: Some(manifest("0.2.0", "https://example.com/nan")),
            ..UpdateState::default()
        };

        assert!(cache_is_fresh_at(&state, checked_at + 3_599));
        assert!(!cache_is_fresh_at(&state, checked_at + 3_600));
    }
}
