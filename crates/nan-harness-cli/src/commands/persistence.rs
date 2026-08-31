use jsonc_parser::ParseOptions;
use jsonc_parser::cst::{CstInputValue, CstObject, CstRootNode};
use nan_harness_core::{CodingModelProfile, DesktopHarnessKind, HarnessKind, ReasoningSelection};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::collections::BTreeMap;
use std::env;
use std::fs::{self, Permissions};
use std::path::{Path, PathBuf};

mod error;
mod filesystem;
mod integrations;
mod managed;
mod models;

pub(crate) use error::PersistenceError;
pub(crate) use filesystem::{config_directory, write_private_file};
use filesystem::{file_name, home_directory, permissions, read_optional, rollback_file};
use managed::{
    apply_prepared_file_change, ensure_qwen_auth_selection, ensure_qwen_list_directory,
    ensure_qwen_model_selection, managed_block_is_active, managed_json_entries_are_active,
    managed_json_property_is_active, optional_utf8, prepare_json_entries,
    prepare_json_entries_removal, prepare_managed_block, prepare_managed_block_removal,
    qwen_auth_selection_is_active, qwen_list_directory_is_active, qwen_model_selection_is_active,
    remove_qwen_auth_selection, remove_qwen_list_directory, remove_qwen_model_selection,
    rollback_prepared_file_change,
};
pub(crate) use models::discover_models;
use models::{
    aider_model_metadata, aider_model_settings, deepseek_provider_settings, qwen_code_provider,
};

const STATE_SCHEMA_VERSION: u8 = 1;
const PREFERENCES_SCHEMA_VERSION: u8 = 3;
const PI_EXTENSION_RELATIVE_PATH: &str = ".pi/agent/extensions/nan-provider.js";
const LEGACY_PI_EXTENSION_RELATIVE_PATH: &str = ".pi/agent/extensions/nan-provider.mjs";
const PRIME_EXTENSION_RELATIVE_PATH: &str = ".prime/agent/extensions/nan-provider.js";
const PRIME_DIRECTORY_ENVIRONMENT_VARIABLE: &str = "PRIME_AGENT_CODING_AGENT_DIR";
const QWEN_DIRECTORY_ENVIRONMENT_VARIABLE: &str = "QWEN_HOME";
const DEEPSEEK_DIRECTORY_ENVIRONMENT_VARIABLE: &str = "DSH_HOME";
const AIDER_SETTINGS_RELATIVE_PATH: &str = ".aider.model.settings.yml";
const AIDER_METADATA_RELATIVE_PATH: &str = ".aider.model.metadata.json";
const DEEPSEEK_BLOCK_BEGIN: &str = "# nan-harness:begin deepseek-provider";
const DEEPSEEK_BLOCK_END: &str = "# nan-harness:end deepseek-provider";
const AIDER_BLOCK_BEGIN: &str = "# nan-harness:begin aider-models";
const AIDER_BLOCK_END: &str = "# nan-harness:end aider-models";
const OPENCODE_CONFIG_DIRECTORY: &str = ".config/opencode";
const OPENCODE_JSON: &str = "opencode.json";
const OPENCODE_JSONC: &str = "opencode.jsonc";

#[derive(Debug, Clone, Copy)]
struct ManagedBlockFormat<'a> {
    begin: &'a str,
    end: &'a str,
    conflicting_keys: &'a [&'a str],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IntegrationChange {
    pub(crate) path: PathBuf,
    pub(crate) additional_paths: Vec<PathBuf>,
    pub(crate) backup: Option<PathBuf>,
    pub(crate) changed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RemovalOutcome {
    Removed,
    NotConfigured,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PersistentIntegration {
    OpenCode,
    Pi,
    PrimeAgent,
    QwenCode,
    DeepSeekHarness,
    Aider,
}

impl std::fmt::Display for PersistentIntegration {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::OpenCode => "OpenCode",
            Self::Pi => "Pi",
            Self::PrimeAgent => "Prime Agent",
            Self::QwenCode => "Qwen Code",
            Self::DeepSeekHarness => "DeepSeek Harness",
            Self::Aider => "Aider",
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ManagedFile {
    sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ManagedJsonProperty {
    value_sha256: String,
    path: PathBuf,
    created_file: bool,
    created_parent_object: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ManagedQwenAuthSelection {
    value_sha256: String,
    created_security_object: bool,
    created_auth_object: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    previous: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ManagedQwenModelSelection {
    value_sha256: String,
    created_model_object: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    previous: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ManagedQwenListDirectory {
    value_sha256: String,
    created_tools_object: bool,
    created_list_directory_object: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    previous: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ManagedQwenCode {
    value_sha256: String,
    path: PathBuf,
    created_file: bool,
    created_parent_object: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    selected_auth_type: Option<ManagedQwenAuthSelection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    selected_model: Option<ManagedQwenModelSelection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    list_directory: Option<ManagedQwenListDirectory>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ManagedBlock {
    block_sha256: String,
    path: PathBuf,
    created_file: bool,
    added_separator: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ManagedJsonEntries {
    entries: BTreeMap<String, String>,
    path: PathBuf,
    created_file: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ManagedAider {
    settings: ManagedBlock,
    metadata: ManagedJsonEntries,
}

struct PreparedFileChange {
    path: PathBuf,
    original: Option<Vec<u8>>,
    original_permissions: Option<Permissions>,
    replacement: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ManagedOpenCode {
    provider_sha256: String,
    file_name: String,
    created_file: bool,
    created_provider_object: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    selected_model: Option<ManagedOpenCodeModel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    search_mcp: Option<ManagedOpenCodeSearch>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ManagedOpenCodeSearch {
    value_sha256: String,
    created_mcp_object: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ManagedOpenCodeModel {
    value_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    previous: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct IntegrationState {
    schema_version: u8,
    #[serde(default, rename = "lastCodexModel", skip_serializing)]
    legacy_last_codex_model: Option<String>,
    #[serde(default)]
    pi: Option<ManagedFile>,
    #[serde(default)]
    prime_agent: Option<ManagedFile>,
    #[serde(default)]
    opencode: Option<ManagedOpenCode>,
    #[serde(default)]
    qwen_code: Option<ManagedQwenCode>,
    #[serde(default)]
    deepseek_harness: Option<ManagedBlock>,
    #[serde(default)]
    aider: Option<ManagedAider>,
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
struct UserPreferences {
    schema_version: u8,
    last_selection_by_harness: BTreeMap<HarnessKind, LastSelection>,
    last_selection_by_desktop: BTreeMap<DesktopHarnessKind, LastSelection>,
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

#[derive(Debug)]
pub(crate) struct PersistenceManager {
    state_directory: PathBuf,
    state_path: PathBuf,
    preferences_path: PathBuf,
    home_directory: PathBuf,
    prime_directory: PathBuf,
    qwen_directory: PathBuf,
    deepseek_directory: PathBuf,
}

impl PersistenceManager {
    pub(crate) fn from_environment() -> Result<Self, PersistenceError> {
        let state_directory = config_directory().ok_or(PersistenceError::MissingConfigDirectory)?;
        let home_directory = home_directory().ok_or(PersistenceError::MissingHomeDirectory)?;
        let prime_directory = env::var_os(PRIME_DIRECTORY_ENVIRONMENT_VARIABLE)
            .map_or_else(|| home_directory.join(".prime/agent"), PathBuf::from);
        let qwen_directory = env::var_os(QWEN_DIRECTORY_ENVIRONMENT_VARIABLE)
            .map_or_else(|| home_directory.join(".qwen"), PathBuf::from);
        let deepseek_directory = env::var_os(DEEPSEEK_DIRECTORY_ENVIRONMENT_VARIABLE)
            .map_or_else(|| home_directory.join(".dsh"), PathBuf::from);
        Ok(Self::new_with_directories(
            state_directory,
            home_directory,
            prime_directory,
            qwen_directory,
            deepseek_directory,
        ))
    }

    #[cfg(test)]
    fn new(state_directory: impl Into<PathBuf>, home_directory: impl Into<PathBuf>) -> Self {
        let home_directory = home_directory.into();
        Self::new_with_directories(
            state_directory,
            home_directory.clone(),
            home_directory.join(".prime/agent"),
            home_directory.join(".qwen"),
            home_directory.join(".dsh"),
        )
    }

    #[cfg(test)]
    pub(crate) fn new_for_tests(
        state_directory: impl Into<PathBuf>,
        home_directory: impl Into<PathBuf>,
    ) -> Self {
        Self::new(state_directory, home_directory)
    }

    fn new_with_directories(
        state_directory: impl Into<PathBuf>,
        home_directory: impl Into<PathBuf>,
        prime_directory: impl Into<PathBuf>,
        qwen_directory: impl Into<PathBuf>,
        deepseek_directory: impl Into<PathBuf>,
    ) -> Self {
        let state_directory = state_directory.into();
        let state_path = state_directory.join("integrations.json");
        let preferences_path = state_directory.join("preferences.json");
        Self {
            state_directory,
            state_path,
            preferences_path,
            home_directory: home_directory.into(),
            prime_directory: prime_directory.into(),
            qwen_directory: qwen_directory.into(),
            deepseek_directory: deepseek_directory.into(),
        }
    }

    pub(crate) fn state_directory(&self) -> &Path {
        &self.state_directory
    }

    pub(crate) fn managed_catalog_paths(
        &self,
        integration: PersistentIntegration,
    ) -> Result<Vec<PathBuf>, PersistenceError> {
        let state = self.load_state()?;
        let paths = match integration {
            PersistentIntegration::OpenCode => {
                vec![self.opencode_config_path(state.opencode.as_ref())?]
            }
            PersistentIntegration::QwenCode => vec![state.qwen_code.as_ref().map_or_else(
                || self.qwen_directory.join("settings.json"),
                |managed| managed.path.clone(),
            )],
            PersistentIntegration::DeepSeekHarness => {
                vec![state.deepseek_harness.as_ref().map_or_else(
                    || self.deepseek_directory.join("settings.yaml"),
                    |managed| managed.path.clone(),
                )]
            }
            PersistentIntegration::Aider => state.aider.as_ref().map_or_else(
                || {
                    vec![
                        self.home_directory.join(AIDER_SETTINGS_RELATIVE_PATH),
                        self.home_directory.join(AIDER_METADATA_RELATIVE_PATH),
                    ]
                },
                |managed| vec![managed.settings.path.clone(), managed.metadata.path.clone()],
            ),
            PersistentIntegration::Pi | PersistentIntegration::PrimeAgent => Vec::new(),
        };
        Ok(paths)
    }

    pub(crate) fn configured_integrations(
        &self,
    ) -> Result<Vec<PersistentIntegration>, PersistenceError> {
        let state = self.load_state()?;
        let mut integrations = Vec::new();
        if state.opencode.is_some() {
            integrations.push(PersistentIntegration::OpenCode);
        }
        if state.pi.is_some() {
            integrations.push(PersistentIntegration::Pi);
        }
        if state.prime_agent.is_some() {
            integrations.push(PersistentIntegration::PrimeAgent);
        }
        if state.qwen_code.is_some() {
            integrations.push(PersistentIntegration::QwenCode);
        }
        if state.deepseek_harness.is_some() {
            integrations.push(PersistentIntegration::DeepSeekHarness);
        }
        if state.aider.is_some() {
            integrations.push(PersistentIntegration::Aider);
        }
        Ok(integrations)
    }

    pub(crate) fn unpersist(
        &self,
        integration: PersistentIntegration,
    ) -> Result<RemovalOutcome, PersistenceError> {
        match integration {
            PersistentIntegration::OpenCode => self.unpersist_opencode(),
            PersistentIntegration::Pi => self.unpersist_pi(),
            PersistentIntegration::PrimeAgent => self.unpersist_prime_agent(),
            PersistentIntegration::QwenCode => self.unpersist_qwen_code(),
            PersistentIntegration::DeepSeekHarness => self.unpersist_deepseek_harness(),
            PersistentIntegration::Aider => self.unpersist_aider(),
        }
    }

    pub(crate) fn integration_is_active(&self, integration: PersistentIntegration) -> bool {
        match integration {
            PersistentIntegration::OpenCode => self.opencode_is_active(),
            PersistentIntegration::Pi => self.pi_is_active(),
            PersistentIntegration::PrimeAgent => self.prime_agent_is_active(),
            PersistentIntegration::QwenCode => self.qwen_code_is_active(),
            PersistentIntegration::DeepSeekHarness => self.deepseek_harness_is_active(),
            PersistentIntegration::Aider => self.aider_is_active(),
        }
    }

    #[cfg(test)]
    pub(crate) fn last_codex_model(&self) -> Result<Option<String>, PersistenceError> {
        Ok(self
            .last_selection(HarnessKind::Codex)?
            .map(|selection| selection.model))
    }

    #[cfg(test)]
    pub(crate) fn save_last_codex_model(&self, model: &str) -> Result<(), PersistenceError> {
        self.save_last_selection(HarnessKind::Codex, model, None)
    }

    pub(crate) fn last_selection(
        &self,
        kind: HarnessKind,
    ) -> Result<Option<LastSelection>, PersistenceError> {
        let preferences = self.load_preferences()?;
        if let Some(selection) = preferences.last_selection_by_harness.get(&kind) {
            return Ok(Some(selection.clone()));
        }
        if kind == HarnessKind::Codex {
            return Ok(self
                .load_state()?
                .legacy_last_codex_model
                .map(|model| LastSelection {
                    model,
                    reasoning: None,
                }));
        }
        Ok(None)
    }

    pub(crate) fn save_last_selection(
        &self,
        kind: HarnessKind,
        model: &str,
        reasoning: Option<ReasoningSelection>,
    ) -> Result<(), PersistenceError> {
        if model.is_empty() {
            return Ok(());
        }
        let mut preferences = self.load_preferences()?;
        preferences.last_selection_by_harness.insert(
            kind,
            LastSelection {
                model: model.to_owned(),
                reasoning,
            },
        );
        self.save_preferences(&preferences)
    }

    pub(crate) fn last_desktop_selection(
        &self,
        kind: DesktopHarnessKind,
    ) -> Result<Option<LastSelection>, PersistenceError> {
        Ok(self
            .load_preferences()?
            .last_selection_by_desktop
            .get(&kind)
            .cloned())
    }

    pub(crate) fn save_last_desktop_selection(
        &self,
        kind: DesktopHarnessKind,
        model: &str,
    ) -> Result<(), PersistenceError> {
        if model.is_empty() {
            return Ok(());
        }
        let mut preferences = self.load_preferences()?;
        preferences.last_selection_by_desktop.insert(
            kind,
            LastSelection {
                model: model.to_owned(),
                reasoning: None,
            },
        );
        self.save_preferences(&preferences)
    }

    fn load_state(&self) -> Result<IntegrationState, PersistenceError> {
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

    fn save_state(&self, state: &IntegrationState) -> Result<(), PersistenceError> {
        fs::create_dir_all(&self.state_directory)
            .map_err(PersistenceError::CreateStateDirectory)?;
        let payload = serde_json::to_vec_pretty(state).map_err(PersistenceError::SerializeState)?;
        write_private_file(&self.state_path, &payload, None)
    }

    fn load_preferences(&self) -> Result<UserPreferences, PersistenceError> {
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

    fn save_preferences(&self, preferences: &UserPreferences) -> Result<(), PersistenceError> {
        fs::create_dir_all(&self.state_directory)
            .map_err(PersistenceError::CreateStateDirectory)?;
        let payload = serde_json::to_vec_pretty(preferences)
            .map_err(PersistenceError::SerializePreferences)?;
        write_private_file(&self.preferences_path, &payload, None)
    }
}

fn opencode_provider(models: &[CodingModelProfile], provider_base_url: &str) -> CstInputValue {
    let models = models
        .iter()
        .map(|model| {
            (
                model.id.clone(),
                CstInputValue::Object(vec![
                    (
                        "name".to_owned(),
                        CstInputValue::String(model.display_name.clone()),
                    ),
                    (
                        "description".to_owned(),
                        CstInputValue::String(model.description.clone()),
                    ),
                    (
                        "limit".to_owned(),
                        CstInputValue::Object(vec![
                            (
                                "context".to_owned(),
                                CstInputValue::Number(model.context_window.to_string()),
                            ),
                            (
                                "output".to_owned(),
                                CstInputValue::Number(model.max_output_tokens.to_string()),
                            ),
                        ]),
                    ),
                ]),
            )
        })
        .collect();
    CstInputValue::Object(vec![
        (
            "npm".to_owned(),
            CstInputValue::String("@ai-sdk/openai-compatible".to_owned()),
        ),
        ("name".to_owned(), CstInputValue::String("NaN".to_owned())),
        (
            "options".to_owned(),
            CstInputValue::Object(vec![(
                "baseURL".to_owned(),
                CstInputValue::String(provider_base_url.to_owned()),
            )]),
        ),
        ("models".to_owned(), CstInputValue::Object(models)),
    ])
}

fn parse_jsonc(source: &str, path: &Path) -> Result<CstRootNode, PersistenceError> {
    CstRootNode::parse(source, &ParseOptions::default()).map_err(|error| {
        PersistenceError::ParseOpenCodeConfig {
            path: path.to_path_buf(),
            message: error.to_string(),
        }
    })
}

fn parse_named_jsonc(
    source: &str,
    path: &Path,
    harness: &'static str,
) -> Result<CstRootNode, PersistenceError> {
    CstRootNode::parse(source, &ParseOptions::default()).map_err(|error| {
        PersistenceError::ParseHarnessConfig {
            harness,
            path: path.to_path_buf(),
            message: error.to_string(),
        }
    })
}

fn hash_input_value(value: &CstInputValue) -> Result<String, PersistenceError> {
    let root = CstRootNode::parse("{}", &ParseOptions::default())
        .map_err(|error| PersistenceError::GenerateOpenCodeProvider(error.to_string()))?;
    root.set_value(value.clone());
    let value = root
        .to_serde_value()
        .ok_or(PersistenceError::GenerateOpenCodeProvider(
            "provider value is empty".to_owned(),
        ))?;
    hash_json_value(&value)
}

fn hash_json_value(value: &serde_json::Value) -> Result<String, PersistenceError> {
    let encoded = serde_json::to_vec(value).map_err(PersistenceError::SerializeProvider)?;
    Ok(sha256(&encoded))
}

fn sha256(value: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";

    let digest = Sha256::digest(value);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

fn empty_jsonc_object_is_disposable(value: &str) -> bool {
    value
        .chars()
        .all(|character| character.is_whitespace() || matches!(character, '{' | '}'))
}

fn validate_opencode_file_name(value: &str) -> Result<(), PersistenceError> {
    if matches!(value, OPENCODE_JSON | OPENCODE_JSONC) {
        Ok(())
    } else {
        Err(PersistenceError::InvalidReceiptPath(value.to_owned()))
    }
}

#[cfg(test)]
mod tests;
