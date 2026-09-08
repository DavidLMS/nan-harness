use super::{CompatibilityError, VerificationManifest};
use nan_harness_private_fs::{
    PrivatePathKind, create_private_dir_all, restrict_file, restrict_path,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::env;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tempfile::Builder as TempFileBuilder;

const CONFIG_DIRECTORY_ENVIRONMENT_VARIABLE: &str = "NAN_HARNESS_CONFIG_DIR";
const STATE_SCHEMA_VERSION: u8 = 4;
const CHECK_INTERVAL: Duration = Duration::from_hours(1);
/// Cache file for the exact-version feed.
///
/// The v2/v3 caches remain untouched: older binaries reject the exact-version fields.
pub(super) const STATE_FILE_NAME: &str = "compatibility-v4.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CompatibilityState {
    pub(super) schema_version: u8,
    /// SHA-256 of the feed URL the cached manifest came from. A cache is only reused for the same
    /// source; the URL itself is never persisted, because it can carry a token.
    #[serde(default)]
    pub(super) source_fingerprint: Option<String>,
    pub(super) last_checked_unix_seconds: Option<u64>,
    pub(super) cached_manifest: Option<VerificationManifest>,
}

impl Default for CompatibilityState {
    fn default() -> Self {
        Self {
            schema_version: STATE_SCHEMA_VERSION,
            source_fingerprint: None,
            last_checked_unix_seconds: None,
            cached_manifest: None,
        }
    }
}

impl CompatibilityState {
    pub(super) fn matches_source(&self, source: &str) -> bool {
        self.source_fingerprint.as_deref() == Some(source_fingerprint(source).as_str())
    }
}

/// Identifies a feed without persisting its URL, which may carry a credential or token.
pub(super) fn source_fingerprint(source: &str) -> String {
    let digest = Sha256::digest(source.as_bytes());
    digest.iter().fold(String::new(), |mut hex, byte| {
        use std::fmt::Write as _;
        let _ = write!(hex, "{byte:02x}");
        hex
    })
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
        create_private_dir_all(&self.directory)
            .map_err(CompatibilityError::CreateConfigDirectory)?;
        let payload =
            serde_json::to_vec_pretty(state).map_err(CompatibilityError::SerializeState)?;
        atomic_write(&self.path, &payload).map_err(CompatibilityError::WriteState)
    }
}

pub(super) fn cache_is_fresh(state: &CompatibilityState, source: &str) -> bool {
    let Ok(now) = unix_seconds() else {
        return false;
    };
    cache_is_fresh_at(state, source, now)
}

pub(super) fn cache_is_fresh_at(state: &CompatibilityState, source: &str, now: u64) -> bool {
    let Some(last_checked) = state.last_checked_unix_seconds else {
        return false;
    };
    if !state.matches_source(source) {
        return false;
    }
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
    // Harden before the payload is written, on every platform: this cache is a private file.
    restrict_file(temporary.as_file_mut())?;
    temporary.write_all(payload)?;
    temporary.write_all(b"\n")?;
    temporary.flush()?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;
    restrict_path(path, PrivatePathKind::File)?;
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
