use serde::{Deserialize, Serialize};
use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use thiserror::Error;

pub const CONFIG_DIRECTORY_ENVIRONMENT_VARIABLE: &str = "NAN_HARNESS_CONFIG_DIR";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReportConsent {
    mode: ConsentMode,
    telemetry_enabled: bool,
}

impl ReportConsent {
    #[must_use]
    pub const fn automatic() -> Self {
        Self {
            mode: ConsentMode::Automatic,
            telemetry_enabled: true,
        }
    }

    #[must_use]
    pub const fn one_time() -> Self {
        Self {
            mode: ConsentMode::OneTime,
            telemetry_enabled: false,
        }
    }

    #[must_use]
    pub const fn mode(self) -> ConsentMode {
        self.mode
    }

    #[must_use]
    pub const fn telemetry_enabled(self) -> bool {
        self.telemetry_enabled
    }

    #[must_use]
    pub const fn is_valid(self) -> bool {
        matches!(
            (self.mode, self.telemetry_enabled),
            (ConsentMode::Automatic, true) | (ConsentMode::OneTime, false)
        )
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ConsentMode {
    Automatic,
    OneTime,
}

impl ConsentMode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Automatic => "automatic",
            Self::OneTime => "one-time",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TelemetryPreference {
    On,
    #[default]
    Off,
}

impl TelemetryPreference {
    #[must_use]
    pub const fn enabled(self) -> bool {
        matches!(self, Self::On)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TelemetrySettings {
    enabled: bool,
}

impl TelemetrySettings {
    #[must_use]
    pub const fn preference(&self) -> TelemetryPreference {
        if self.enabled {
            TelemetryPreference::On
        } else {
            TelemetryPreference::Off
        }
    }

    #[must_use]
    pub const fn enabled(&self) -> bool {
        self.enabled
    }
}

#[derive(Debug, Clone)]
pub struct TelemetrySettingsStore {
    directory: PathBuf,
    path: PathBuf,
}

impl TelemetrySettingsStore {
    #[must_use]
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        let directory = directory.into();
        let path = directory.join("telemetry.json");
        Self { directory, path }
    }

    /// Resolves the per-user settings directory for the current platform.
    ///
    /// # Errors
    ///
    /// Returns [`SettingsError`] when no supported user configuration directory is available.
    pub fn from_environment() -> Result<Self, SettingsError> {
        if let Some(directory) = env::var_os(CONFIG_DIRECTORY_ENVIRONMENT_VARIABLE) {
            return Ok(Self::new(directory));
        }
        platform_config_directory()
            .map(Self::new)
            .ok_or(SettingsError::MissingConfigDirectory)
    }

    /// Loads the persisted preference, defaulting to off when no file exists.
    ///
    /// # Errors
    ///
    /// Returns [`SettingsError`] when an existing settings file cannot be read or parsed.
    pub fn load(&self) -> Result<TelemetrySettings, SettingsError> {
        match fs::read(&self.path) {
            Ok(contents) => serde_json::from_slice(&contents).map_err(SettingsError::Parse),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(TelemetrySettings::default())
            }
            Err(error) => Err(SettingsError::Read(error)),
        }
    }

    /// Persists the selected telemetry preference.
    ///
    /// # Errors
    ///
    /// Returns [`SettingsError`] when the directory or settings file cannot be written.
    pub fn set(&self, preference: TelemetryPreference) -> Result<(), SettingsError> {
        fs::create_dir_all(&self.directory).map_err(SettingsError::CreateDirectory)?;
        let payload = serde_json::to_vec_pretty(&TelemetrySettings {
            enabled: preference.enabled(),
        })
        .map_err(SettingsError::Serialize)?;
        let mut options = OpenOptions::new();
        options.create(true).truncate(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        let mut file = options.open(&self.path).map_err(SettingsError::Write)?;
        file.write_all(&payload).map_err(SettingsError::Write)?;
        file.write_all(b"\n").map_err(SettingsError::Write)?;
        file.sync_all().map_err(SettingsError::Write)
    }

    #[must_use]
    pub fn directory(&self) -> &Path {
        &self.directory
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
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

#[derive(Debug, Error)]
pub enum SettingsError {
    #[error("could not determine the NaN Harness configuration directory")]
    MissingConfigDirectory,
    #[error("could not create the NaN Harness configuration directory: {0}")]
    CreateDirectory(std::io::Error),
    #[error("could not read telemetry settings: {0}")]
    Read(std::io::Error),
    #[error("telemetry settings are not valid JSON: {0}")]
    Parse(serde_json::Error),
    #[error("could not serialize telemetry settings: {0}")]
    Serialize(serde_json::Error),
    #[error("could not write telemetry settings: {0}")]
    Write(std::io::Error),
}
