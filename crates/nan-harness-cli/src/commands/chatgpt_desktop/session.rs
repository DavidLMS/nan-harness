use super::profile::{ManagedProfile, validate_managed_profile};
use super::{
    CONFIG_FILE_NAME, ChatGptDesktopError, MODEL_CATALOG_FILE_NAME, SESSION_SCHEMA_VERSION,
    SESSION_TOKEN_ENVIRONMENT, SURFACE_ID,
};
use crate::commands::desktop::{reject_symlink, remove_file_if_present, write_private_atomic};
use nan_harness_runtime::RunningCodexDesktopBridge;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::str::FromStr;

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct SessionReceipt {
    pub(super) schema_version: u8,
    pub(super) surface: String,
    pub(super) config_file: String,
    pub(super) model_catalog_file: String,
}

pub(super) fn apply_session(
    profile: &ManagedProfile,
    bridge: &RunningCodexDesktopBridge,
    web_search_enabled: bool,
) -> Result<(), ChatGptDesktopError> {
    validate_managed_profile(profile)?;
    reject_orphaned_session_files(profile)?;
    let receipt = SessionReceipt {
        schema_version: SESSION_SCHEMA_VERSION,
        surface: SURFACE_ID.to_owned(),
        config_file: CONFIG_FILE_NAME.to_owned(),
        model_catalog_file: MODEL_CATALOG_FILE_NAME.to_owned(),
    };
    let serialized =
        serde_json::to_vec_pretty(&receipt).map_err(ChatGptDesktopError::SerializeState)?;
    write_private_atomic(&profile.receipt, &[serialized.as_slice(), b"\n"].concat())?;
    write_private_atomic(&profile.catalog, bridge.model_catalog_json().as_bytes())?;
    let config = desktop_config(
        bridge.selected_model(),
        bridge.base_url(),
        &profile.catalog,
        web_search_enabled,
    )?;
    write_private_atomic(&profile.config, config.as_bytes()).map_err(ChatGptDesktopError::from)
}

pub(super) fn reject_orphaned_session_files(
    profile: &ManagedProfile,
) -> Result<(), ChatGptDesktopError> {
    if !profile.receipt.exists() && (profile.config.exists() || profile.catalog.exists()) {
        return Err(ChatGptDesktopError::OrphanedSessionFiles);
    }
    Ok(())
}

pub(super) fn desktop_config(
    selected_model: &str,
    bridge_base_url: &str,
    catalog_path: &Path,
    web_search_enabled: bool,
) -> Result<String, ChatGptDesktopError> {
    let model =
        serde_json::to_string(selected_model).map_err(ChatGptDesktopError::SerializeState)?;
    let base_url = serde_json::to_string(&format!("{}/v1", bridge_base_url.trim_end_matches('/')))
        .map_err(ChatGptDesktopError::SerializeState)?;
    let catalog = serde_json::to_string(&catalog_path.to_string_lossy())
        .map_err(ChatGptDesktopError::SerializeState)?;
    Ok(format!(
        concat!(
            "model = {}\n",
            "model_provider = \"nan_harness\"\n",
            "model_catalog_json = {}\n",
            "suppress_unstable_features_warning = true\n\n",
            "[features]\n",
            "apps = false\n",
            "standalone_web_search = {}\n",
            "responses_websockets = false\n",
            "responses_websockets_v2 = false\n\n",
            "[model_providers.nan_harness]\n",
            "name = \"nan-harness\"\n",
            "base_url = {}\n",
            "env_key = \"{}\"\n",
            "wire_api = \"responses\"\n",
            "request_max_retries = 0\n",
            "stream_max_retries = 0\n",
            "supports_websockets = false\n",
            "supports_standalone_web_search = {}\n",
            "requires_openai_auth = false\n"
        ),
        model, catalog, web_search_enabled, base_url, SESSION_TOKEN_ENVIRONMENT, web_search_enabled
    ))
}

pub(super) fn restore_session(profile: &ManagedProfile) -> Result<bool, ChatGptDesktopError> {
    reject_symlink(&profile.receipt)?;
    let contents = match fs::read(&profile.receipt) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(ChatGptDesktopError::ReadState(error)),
    };
    let receipt: SessionReceipt =
        serde_json::from_slice(&contents).map_err(ChatGptDesktopError::ParseReceipt)?;
    if receipt.schema_version != SESSION_SCHEMA_VERSION
        || receipt.surface != SURFACE_ID
        || receipt.config_file != CONFIG_FILE_NAME
        || receipt.model_catalog_file != MODEL_CATALOG_FILE_NAME
    {
        return Err(ChatGptDesktopError::InvalidReceipt);
    }
    remove_file_if_present(&profile.config)?;
    remove_file_if_present(&profile.catalog)?;
    remove_file_if_present(&profile.receipt)?;
    Ok(true)
}

pub(super) fn selected_model_from_config(
    profile: &ManagedProfile,
    available: &[String],
) -> Option<String> {
    let contents = fs::read_to_string(&profile.config).ok()?;
    let document = toml_edit::DocumentMut::from_str(&contents).ok()?;
    let selected = document.get("model")?.as_str()?;
    available
        .iter()
        .find(|model| model.as_str() == selected)
        .cloned()
}
