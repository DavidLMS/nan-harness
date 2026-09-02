use super::{
    CONFIG_FILE_NAME, ChatGptDesktopError, MODEL_CATALOG_FILE_NAME, PROFILE_DIRECTORY_NAME,
    PROFILE_MARKER_NAME, PROFILE_SCHEMA_VERSION, SESSION_RECEIPT_NAME, STATE_DIRECTORY_NAME,
    SURFACE_ID,
};
use crate::commands::desktop::{create_private_directory, create_private_new, reject_symlink};
use crate::commands::persistence::PersistenceManager;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub(super) struct ManagedProfile {
    pub(super) root: PathBuf,
    pub(super) marker: PathBuf,
    pub(super) receipt: PathBuf,
    pub(super) config: PathBuf,
    pub(super) catalog: PathBuf,
}

impl ManagedProfile {
    pub(super) fn for_manager(manager: &PersistenceManager) -> Self {
        let root = manager
            .state_directory()
            .join(STATE_DIRECTORY_NAME)
            .join(PROFILE_DIRECTORY_NAME);
        Self {
            marker: root.join(PROFILE_MARKER_NAME),
            receipt: root.join(SESSION_RECEIPT_NAME),
            config: root.join(CONFIG_FILE_NAME),
            catalog: root.join(MODEL_CATALOG_FILE_NAME),
            root,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ProfileMarker {
    pub(super) schema_version: u8,
    pub(super) surface: String,
}

pub(super) fn ensure_managed_profile(profile: &ManagedProfile) -> Result<(), ChatGptDesktopError> {
    reject_symlink(&profile.root)?;
    if profile.root.exists() {
        if profile.marker.exists() {
            create_private_directory(&profile.root)?;
            return validate_managed_profile(profile);
        }
        let empty = fs::read_dir(&profile.root)
            .map_err(ChatGptDesktopError::InspectProfile)?
            .next()
            .is_none();
        if !empty {
            return Err(ChatGptDesktopError::UnmanagedProfile);
        }
    } else {
        create_private_directory(&profile.root)?;
    }
    let marker = ProfileMarker {
        schema_version: PROFILE_SCHEMA_VERSION,
        surface: SURFACE_ID.to_owned(),
    };
    let serialized =
        serde_json::to_vec_pretty(&marker).map_err(ChatGptDesktopError::SerializeState)?;
    let mut file = create_private_new(&profile.marker)?;
    file.write_all(&serialized)
        .and_then(|()| file.write_all(b"\n"))
        .map_err(ChatGptDesktopError::WriteState)?;
    Ok(())
}

pub(super) fn validate_managed_profile(
    profile: &ManagedProfile,
) -> Result<(), ChatGptDesktopError> {
    reject_symlink(&profile.root)?;
    reject_symlink(&profile.marker)?;
    let contents = fs::read(&profile.marker).map_err(ChatGptDesktopError::ReadState)?;
    let marker: ProfileMarker =
        serde_json::from_slice(&contents).map_err(ChatGptDesktopError::ParseMarker)?;
    if marker.schema_version != PROFILE_SCHEMA_VERSION || marker.surface != SURFACE_ID {
        return Err(ChatGptDesktopError::InvalidMarker);
    }
    Ok(())
}
