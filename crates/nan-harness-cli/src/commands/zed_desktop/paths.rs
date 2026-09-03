use super::ZedDesktopError;
use crate::commands::persistence::config_directory;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub(super) const SESSION_SCHEMA_VERSION: u8 = 1;

#[derive(Debug, Clone)]
pub(super) struct ZedPaths {
    pub(super) settings: PathBuf,
    pub(super) state_directory: PathBuf,
    pub(super) session_receipt: PathBuf,
    pub(super) backup_directory: PathBuf,
}

impl ZedPaths {
    pub(super) fn from_environment() -> Result<Self, ZedDesktopError> {
        let home = user_home().ok_or(ZedDesktopError::MissingHomeDirectory)?;
        let state_directory = config_directory()
            .ok_or(ZedDesktopError::MissingStateDirectory)?
            .join("zed-desktop");
        let settings = settings_path_for_platform(
            current_platform()?,
            &home,
            std::env::var_os("XDG_CONFIG_HOME")
                .as_deref()
                .map(Path::new),
            std::env::var_os("APPDATA").as_deref().map(Path::new),
        )?;
        Self::new(settings, state_directory)
    }

    pub(super) fn new(
        settings: PathBuf,
        state_directory: PathBuf,
    ) -> Result<Self, ZedDesktopError> {
        if !settings.is_absolute() || !state_directory.is_absolute() {
            return Err(ZedDesktopError::InvalidPath);
        }
        Ok(Self {
            settings,
            session_receipt: state_directory.join("session.json"),
            backup_directory: state_directory.join("session-backups"),
            state_directory,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ZedPlatform {
    Macos,
    Windows,
    Linux,
}

pub(super) fn current_platform() -> Result<ZedPlatform, ZedDesktopError> {
    if cfg!(target_os = "macos") {
        Ok(ZedPlatform::Macos)
    } else if cfg!(windows) {
        Ok(ZedPlatform::Windows)
    } else if cfg!(target_os = "linux") {
        Ok(ZedPlatform::Linux)
    } else {
        Err(ZedDesktopError::UnsupportedPlatform)
    }
}

pub(super) fn settings_path_for_platform(
    platform: ZedPlatform,
    home: &Path,
    xdg_config_home: Option<&Path>,
    app_data: Option<&Path>,
) -> Result<PathBuf, ZedDesktopError> {
    if !home.is_absolute() {
        return Err(ZedDesktopError::InvalidPath);
    }
    let directory = match platform {
        ZedPlatform::Macos | ZedPlatform::Linux => xdg_config_home
            .map_or_else(|| home.join(".config"), Path::to_path_buf)
            .join("zed"),
        ZedPlatform::Windows => app_data
            .filter(|path| path.is_absolute())
            .ok_or(ZedDesktopError::MissingPlatformDirectory)?
            .join("Zed"),
    };
    if !directory.is_absolute() {
        return Err(ZedDesktopError::InvalidPath);
    }
    Ok(directory.join("settings.json"))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(clippy::struct_excessive_bools)]
pub(super) struct SessionReceipt {
    pub(super) schema_version: u8,
    pub(super) file_existed: bool,
    pub(super) original_sha256: Option<String>,
    pub(super) backup_file: String,
    pub(super) applied_file_sha256: String,
    pub(super) applied_provider_sha256: String,
    pub(super) applied_default_model_sha256: String,
    pub(super) created_language_models: bool,
    pub(super) created_openai_compatible: bool,
    pub(super) created_agent: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) previous_default_model: Option<Value>,
}

fn user_home() -> Option<PathBuf> {
    if cfg!(windows) {
        std::env::var_os("USERPROFILE").map(PathBuf::from)
    } else {
        std::env::var_os("HOME").map(PathBuf::from)
    }
}
