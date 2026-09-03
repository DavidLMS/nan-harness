use super::{PersistenceError, PersistenceManager, write_private_file};
use nan_harness_core::{DesktopHarnessKind, HarnessKind, ReasoningSelection};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

const STATE_SCHEMA_VERSION: u8 = 1;
const PREFERENCES_SCHEMA_VERSION: u8 = 3;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ManagedFile {
    pub(super) sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ManagedJsonProperty {
    pub(super) value_sha256: String,
    pub(super) path: PathBuf,
    pub(super) created_file: bool,
    pub(super) created_parent_object: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ManagedQwenAuthSelection {
    pub(super) value_sha256: String,
    pub(super) created_security_object: bool,
    pub(super) created_auth_object: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) previous: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ManagedQwenModelSelection {
    pub(super) value_sha256: String,
    pub(super) created_model_object: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) previous: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ManagedQwenListDirectory {
    pub(super) value_sha256: String,
    pub(super) created_tools_object: bool,
    pub(super) created_list_directory_object: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) previous: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ManagedQwenCode {
    pub(super) value_sha256: String,
    pub(super) path: PathBuf,
    pub(super) created_file: bool,
    pub(super) created_parent_object: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) selected_auth_type: Option<ManagedQwenAuthSelection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) selected_model: Option<ManagedQwenModelSelection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) list_directory: Option<ManagedQwenListDirectory>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ManagedBlock {
    pub(super) block_sha256: String,
    pub(super) path: PathBuf,
    pub(super) created_file: bool,
    pub(super) added_separator: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ManagedJsonEntries {
    pub(super) entries: BTreeMap<String, String>,
    pub(super) path: PathBuf,
    pub(super) created_file: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ManagedAider {
    pub(super) settings: ManagedBlock,
    pub(super) metadata: ManagedJsonEntries,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ManagedOpenCode {
    pub(super) provider_sha256: String,
    pub(super) file_name: String,
    pub(super) created_file: bool,
    pub(super) created_provider_object: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) selected_model: Option<ManagedOpenCodeModel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) search_mcp: Option<ManagedOpenCodeSearch>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ManagedOpenCodeSearch {
    pub(super) value_sha256: String,
    pub(super) created_mcp_object: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ManagedOpenCodeModel {
    pub(super) value_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) previous: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct IntegrationState {
    pub(super) schema_version: u8,
    #[serde(default, rename = "lastCodexModel", skip_serializing)]
    pub(super) legacy_last_codex_model: Option<String>,
    #[serde(default)]
    pub(super) pi: Option<ManagedFile>,
    #[serde(default)]
    pub(super) prime_agent: Option<ManagedFile>,
    #[serde(default)]
    pub(super) opencode: Option<ManagedOpenCode>,
    #[serde(default)]
    pub(super) qwen_code: Option<ManagedQwenCode>,
    #[serde(default)]
    pub(super) deepseek_harness: Option<ManagedBlock>,
    #[serde(default)]
    pub(super) aider: Option<ManagedAider>,
}

impl Default for IntegrationState {
    fn default() -> Self {
        Self {
            schema_version: STATE_SCHEMA_VERSION,
            legacy_last_codex_model: None,
            pi: None,
            prime_agent: None,
            opencode: None,
            qwen_code: None,
            deepseek_harness: None,
            aider: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UserPreferencesV1 {
    schema_version: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_codex_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_codex_reasoning: Option<ReasoningSelection>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UserPreferencesV2 {
    schema_version: u8,
    last_selection_by_harness: BTreeMap<HarnessKind, LastSelection>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct UserPreferences {
    pub(super) schema_version: u8,
    pub(super) last_selection_by_harness: BTreeMap<HarnessKind, LastSelection>,
    pub(super) last_selection_by_desktop: BTreeMap<DesktopHarnessKind, LastSelection>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PreferencesSchema {
    schema_version: u8,
}

impl Default for UserPreferences {
    fn default() -> Self {
        Self {
            schema_version: PREFERENCES_SCHEMA_VERSION,
            last_selection_by_harness: BTreeMap::new(),
            last_selection_by_desktop: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct LastSelection {
    pub(crate) model: String,
    pub(crate) reasoning: Option<ReasoningSelection>,
}

impl From<UserPreferencesV1> for UserPreferences {
    fn from(preferences: UserPreferencesV1) -> Self {
        let mut migrated = Self::default();
        if let Some(model) = preferences.last_codex_model {
            migrated.last_selection_by_harness.insert(
                HarnessKind::Codex,
                LastSelection {
                    model,
                    reasoning: preferences.last_codex_reasoning,
                },
            );
        }
        migrated
    }
}

impl From<UserPreferencesV2> for UserPreferences {
    fn from(preferences: UserPreferencesV2) -> Self {
        Self {
            schema_version: PREFERENCES_SCHEMA_VERSION,
            last_selection_by_harness: preferences.last_selection_by_harness,
            last_selection_by_desktop: BTreeMap::new(),
        }
    }
}

impl PersistenceManager {
    pub(super) fn load_state(&self) -> Result<IntegrationState, PersistenceError> {
        match fs::read(&self.state_path) {
            Ok(contents) => {
                let state: IntegrationState =
                    serde_json::from_slice(&contents).map_err(PersistenceError::ParseState)?;
                if state.schema_version != STATE_SCHEMA_VERSION {
                    return Err(PersistenceError::UnsupportedStateSchema(
                        state.schema_version,
                    ));
                }
                Ok(state)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(IntegrationState::default())
            }
            Err(error) => Err(PersistenceError::ReadState(error)),
        }
    }

    pub(super) fn save_state(&self, state: &IntegrationState) -> Result<(), PersistenceError> {
        fs::create_dir_all(&self.state_directory)
            .map_err(PersistenceError::CreateStateDirectory)?;
        let payload = serde_json::to_vec_pretty(state).map_err(PersistenceError::SerializeState)?;
        write_private_file(&self.state_path, &payload, None)
    }

    pub(super) fn load_preferences(&self) -> Result<UserPreferences, PersistenceError> {
        match fs::read(&self.preferences_path) {
            Ok(contents) => {
                let schema: PreferencesSchema = serde_json::from_slice(&contents)
                    .map_err(PersistenceError::ParsePreferences)?;
                match schema.schema_version {
                    1 => serde_json::from_slice::<UserPreferencesV1>(&contents)
                        .map(UserPreferences::from)
                        .map_err(PersistenceError::ParsePreferences),
                    2 => serde_json::from_slice::<UserPreferencesV2>(&contents)
                        .map(UserPreferences::from)
                        .map_err(PersistenceError::ParsePreferences),
                    PREFERENCES_SCHEMA_VERSION => {
                        serde_json::from_slice::<UserPreferences>(&contents)
                            .map_err(PersistenceError::ParsePreferences)
                    }
                    version => Err(PersistenceError::UnsupportedPreferencesSchema(version)),
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(UserPreferences::default())
            }
            Err(error) => Err(PersistenceError::ReadPreferences(error)),
        }
    }

    pub(super) fn save_preferences(
        &self,
        preferences: &UserPreferences,
    ) -> Result<(), PersistenceError> {
        fs::create_dir_all(&self.state_directory)
            .map_err(PersistenceError::CreateStateDirectory)?;
        let payload = serde_json::to_vec_pretty(preferences)
            .map_err(PersistenceError::SerializePreferences)?;
        write_private_file(&self.preferences_path, &payload, None)
    }
}
