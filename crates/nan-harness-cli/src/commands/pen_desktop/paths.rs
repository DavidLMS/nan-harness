use super::PenDesktopError;
use crate::commands::persistence::config_directory;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub(super) const SESSION_SCHEMA_VERSION: u8 = 1;
pub(super) const PERSISTENT_SCHEMA_VERSION: u8 = 1;

#[derive(Debug, Clone)]
pub(super) struct PenPaths {
    pub(super) models: PathBuf,
    pub(super) auth: PathBuf,
    pub(super) state_directory: PathBuf,
    pub(super) session_receipt: PathBuf,
    pub(super) session_backup_directory: PathBuf,
    pub(super) persistent_receipt: PathBuf,
    pub(super) persistent_backup_directory: PathBuf,
}

impl PenPaths {
    pub(super) fn from_environment() -> Result<Self, PenDesktopError> {
        let home = user_home().ok_or(PenDesktopError::MissingHomeDirectory)?;
        let state = config_directory()
            .ok_or(PenDesktopError::MissingStateDirectory)?
            .join("pen-desktop");
        Self::new(&home, &state)
    }

    pub(super) fn new(home: &Path, state_directory: &Path) -> Result<Self, PenDesktopError> {
        if !home.is_absolute() || !state_directory.is_absolute() {
            return Err(PenDesktopError::InvalidPath);
        }
        let pencil_directory = home.join(".pencil");
        Ok(Self {
            models: pencil_directory.join("models.json"),
            auth: pencil_directory.join("agent-auth"),
            session_receipt: state_directory.join("session.json"),
            session_backup_directory: state_directory.join("session-backups"),
            persistent_receipt: state_directory.join("configuration.json"),
            persistent_backup_directory: state_directory.join("configuration-backups"),
            state_directory: state_directory.to_path_buf(),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct FileSnapshot {
    pub(super) existed: bool,
    pub(super) original_sha256: Option<String>,
    pub(super) backup_file: String,
    pub(super) applied_file_sha256: String,
    pub(super) applied_entry_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct SessionReceipt {
    pub(super) schema_version: u8,
    pub(super) models: FileSnapshot,
    pub(super) auth: FileSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct PersistentReceipt {
    pub(super) schema_version: u8,
    pub(super) models_file_existed: bool,
    pub(super) auth_file_existed: bool,
    pub(super) models_backup: PersistentEntryBackup,
    pub(super) auth_backup: PersistentEntryBackup,
    pub(super) credential_fingerprint: String,
    pub(super) applied_models_sha256: String,
    pub(super) applied_auth_sha256: String,
    pub(super) model_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct PersistentEntryBackup {
    pub(super) existed: bool,
    pub(super) sha256: Option<String>,
    pub(super) backup_file: String,
}

pub(super) fn user_home() -> Option<PathBuf> {
    if cfg!(windows) {
        std::env::var_os("USERPROFILE").map(PathBuf::from)
    } else {
        std::env::var_os("HOME").map(PathBuf::from)
    }
}
