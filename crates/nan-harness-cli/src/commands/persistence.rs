use jsonc_parser::ParseOptions;
use jsonc_parser::cst::{CstInputValue, CstObject, CstRootNode};
use nan_harness_adapters::persistent_provider_extension;
use nan_harness_core::CodingModelProfile;
use nan_harness_runtime::ResolvedConfig;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::collections::BTreeMap;
use std::env;
use std::fs::{self, Permissions};
use std::ops::Range;
use std::path::{Path, PathBuf};

mod error;
mod filesystem;
mod models;

pub(crate) use error::PersistenceError;
pub(crate) use filesystem::{config_directory, write_private_file};
use filesystem::{
    create_backup, file_name, home_directory, permissions, read_optional, rollback_file,
};
use models::{
    aider_model_metadata, aider_model_settings, deepseek_provider_settings, qwen_code_provider,
    validate_provider_url,
};
pub(crate) use models::{discover_models, effective_provider_base_url};

const STATE_SCHEMA_VERSION: u8 = 1;
const PREFERENCES_SCHEMA_VERSION: u8 = 1;
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
    conflicting_key: Option<&'a str>,
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

struct ManagedFileChange {
    change: IntegrationChange,
    original: Option<Vec<u8>>,
    original_permissions: Option<Permissions>,
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
struct UserPreferences {
    schema_version: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_codex_model: Option<String>,
}

impl Default for UserPreferences {
    fn default() -> Self {
        Self {
            schema_version: PREFERENCES_SCHEMA_VERSION,
            last_codex_model: None,
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

    pub(crate) fn last_codex_model(&self) -> Result<Option<String>, PersistenceError> {
        let preferences = self.load_preferences()?;
        if preferences.last_codex_model.is_some() {
            return Ok(preferences.last_codex_model);
        }
        Ok(self.load_state()?.legacy_last_codex_model)
    }

    pub(crate) fn save_last_codex_model(&self, model: &str) -> Result<(), PersistenceError> {
        if model.is_empty() {
            return Ok(());
        }
        let mut preferences = self.load_preferences()?;
        preferences.last_codex_model = Some(model.to_owned());
        self.save_preferences(&preferences)
    }

    pub(crate) fn persist_pi(
        &self,
        provider_base_url: &str,
    ) -> Result<IntegrationChange, PersistenceError> {
        validate_provider_url(provider_base_url)?;
        let content = persistent_provider_extension(provider_base_url)
            .map_err(|error| PersistenceError::GeneratePiExtension(error.to_string()))?;
        let path = self.home_directory.join(PI_EXTENSION_RELATIVE_PATH);
        let mut state = self.load_state()?;
        let previous = state.pi.clone();
        let (change, managed) = Self::persist_extension(&path, &content, state.pi.as_ref())?;
        state.pi = Some(managed);
        if let Err(error) = self.save_state(&state) {
            rollback_managed_change(&change, &path);
            return Err(error);
        }
        if previous
            .as_ref()
            .is_some_and(|managed| managed.path.is_none())
        {
            let legacy = self.home_directory.join(LEGACY_PI_EXTENSION_RELATIVE_PATH);
            if fs::read(&legacy).is_ok_and(|contents| {
                previous
                    .as_ref()
                    .is_some_and(|managed| sha256(&contents) == managed.sha256)
            }) {
                let _ = fs::remove_file(legacy);
            }
        }
        Ok(change.change)
    }

    pub(crate) fn unpersist_pi(&self) -> Result<RemovalOutcome, PersistenceError> {
        let mut state = self.load_state()?;
        let Some(managed) = state.pi.clone() else {
            return Ok(RemovalOutcome::NotConfigured);
        };
        let path = managed.path.clone().unwrap_or_else(|| {
            let current = self.home_directory.join(PI_EXTENSION_RELATIVE_PATH);
            if current.exists() {
                current
            } else {
                self.home_directory.join(LEGACY_PI_EXTENSION_RELATIVE_PATH)
            }
        });
        let original = read_optional(&path)?;
        let original_permissions = permissions(&path)?;
        if let Some(contents) = original.as_deref()
            && sha256(contents) != managed.sha256
        {
            return Err(PersistenceError::ManagedFileChanged(path));
        }
        if original.is_some() {
            fs::remove_file(&path).map_err(|source| PersistenceError::RemoveFile {
                path: path.clone(),
                source,
            })?;
        }
        state.pi = None;
        if let Err(error) = self.save_state(&state) {
            rollback_file(&path, original.as_deref(), original_permissions.as_ref());
            return Err(error);
        }
        Ok(RemovalOutcome::Removed)
    }

    pub(crate) fn pi_is_active(&self) -> bool {
        let Ok(state) = self.load_state() else {
            return false;
        };
        let Some(managed) = state.pi else {
            return false;
        };
        let path = managed
            .path
            .unwrap_or_else(|| self.home_directory.join(PI_EXTENSION_RELATIVE_PATH));
        fs::read(path).is_ok_and(|contents| sha256(&contents) == managed.sha256)
    }

    pub(crate) fn persist_prime_agent(
        &self,
        provider_base_url: &str,
    ) -> Result<IntegrationChange, PersistenceError> {
        validate_provider_url(provider_base_url)?;
        let content = persistent_provider_extension(provider_base_url)
            .map_err(|error| PersistenceError::GeneratePiExtension(error.to_string()))?;
        let path = self.prime_directory.join("extensions/nan-provider.js");
        let mut state = self.load_state()?;
        let (change, managed) =
            Self::persist_extension(&path, &content, state.prime_agent.as_ref())?;
        state.prime_agent = Some(managed);
        if let Err(error) = self.save_state(&state) {
            rollback_managed_change(&change, &path);
            return Err(error);
        }
        Ok(change.change)
    }

    pub(crate) fn unpersist_prime_agent(&self) -> Result<RemovalOutcome, PersistenceError> {
        let mut state = self.load_state()?;
        let Some(managed) = state.prime_agent.clone() else {
            return Ok(RemovalOutcome::NotConfigured);
        };
        let path = managed
            .path
            .clone()
            .unwrap_or_else(|| self.home_directory.join(PRIME_EXTENSION_RELATIVE_PATH));
        let original = read_optional(&path)?;
        let original_permissions = permissions(&path)?;
        Self::remove_managed_file(&path, &managed)?;
        state.prime_agent = None;
        if let Err(error) = self.save_state(&state) {
            rollback_file(&path, original.as_deref(), original_permissions.as_ref());
            return Err(error);
        }
        Ok(RemovalOutcome::Removed)
    }

    pub(crate) fn prime_agent_is_active(&self) -> bool {
        self.managed_file_is_active(
            |state| state.prime_agent.as_ref(),
            |managed| {
                managed
                    .path
                    .clone()
                    .unwrap_or_else(|| self.prime_directory.join("extensions/nan-provider.js"))
            },
        )
    }

    fn persist_extension(
        path: &Path,
        content: &str,
        managed: Option<&ManagedFile>,
    ) -> Result<(ManagedFileChange, ManagedFile), PersistenceError> {
        let original = read_optional(path)?;
        let original_permissions = permissions(path)?;
        let desired_hash = sha256(content.as_bytes());
        if let Some(existing) = original.as_deref() {
            let existing_hash = sha256(existing);
            match managed {
                Some(managed) if managed.sha256 != existing_hash => {
                    return Err(PersistenceError::ManagedFileChanged(path.to_path_buf()));
                }
                None if existing_hash != desired_hash => {
                    return Err(PersistenceError::UnmanagedFileConflict(path.to_path_buf()));
                }
                _ => {}
            }
        }
        let changed = original.as_deref() != Some(content.as_bytes());
        if changed {
            write_private_file(path, content.as_bytes(), original_permissions.as_ref())?;
        }
        Ok((
            ManagedFileChange {
                change: IntegrationChange {
                    path: path.to_path_buf(),
                    additional_paths: Vec::new(),
                    backup: None,
                    changed,
                },
                original,
                original_permissions,
            },
            ManagedFile {
                sha256: desired_hash,
                path: Some(path.to_path_buf()),
            },
        ))
    }

    fn remove_managed_file(path: &Path, managed: &ManagedFile) -> Result<(), PersistenceError> {
        let Some(contents) = read_optional(path)? else {
            return Ok(());
        };
        if sha256(&contents) != managed.sha256 {
            return Err(PersistenceError::ManagedFileChanged(path.to_path_buf()));
        }
        fs::remove_file(path).map_err(|source| PersistenceError::RemoveFile {
            path: path.to_path_buf(),
            source,
        })
    }

    fn managed_file_is_active(
        &self,
        select: impl FnOnce(&IntegrationState) -> Option<&ManagedFile>,
        path: impl FnOnce(&ManagedFile) -> PathBuf,
    ) -> bool {
        let Ok(state) = self.load_state() else {
            return false;
        };
        let Some(managed) = select(&state) else {
            return false;
        };
        fs::read(path(managed)).is_ok_and(|contents| sha256(&contents) == managed.sha256)
    }

    pub(crate) async fn persist_opencode(
        &self,
        config: &ResolvedConfig,
    ) -> Result<IntegrationChange, PersistenceError> {
        let models = discover_models(config).await?;
        let provider = opencode_provider(&models, &config.provider_base_url);
        let provider_hash = hash_input_value(&provider)?;
        let mut state = self.load_state()?;
        let path = self.opencode_config_path(state.opencode.as_ref())?;
        let original = read_optional(&path)?;
        let original_permissions = permissions(&path)?;
        let created_file = original.is_none();
        let source = original.as_deref().map_or_else(
            || "{}\n".to_owned(),
            |value| String::from_utf8_lossy(value).into_owned(),
        );
        let root = parse_jsonc(&source, &path)?;
        let root_object = root
            .object_value_or_create()
            .ok_or_else(|| PersistenceError::RootIsNotObject(path.clone()))?;
        let provider_property = root_object.get("provider");
        let created_provider_object = provider_property.is_none();
        let providers = match provider_property {
            Some(property) => property
                .object_value()
                .ok_or_else(|| PersistenceError::ProviderIsNotObject(path.clone()))?,
            None => root_object.object_value_or_set("provider"),
        };

        if let Some(existing) = providers.get("nan") {
            let existing_value = existing
                .to_serde_value()
                .ok_or_else(|| PersistenceError::InvalidManagedProvider(path.clone()))?;
            let existing_hash = hash_json_value(&existing_value)?;
            match state.opencode.as_ref() {
                Some(managed) if managed.provider_sha256 != existing_hash => {
                    return Err(PersistenceError::ManagedProviderChanged(path));
                }
                None if existing_hash != provider_hash => {
                    return Err(PersistenceError::UnmanagedProviderConflict(path));
                }
                _ => existing.set_value(provider.clone()),
            }
        } else {
            providers.append("nan", provider);
        }

        let rendered = root.to_string();
        let changed = original.as_deref() != Some(rendered.as_bytes());
        let backup = if state.opencode.is_none() && original.is_some() {
            create_backup(&path)?
        } else {
            None
        };
        if changed {
            write_private_file(&path, rendered.as_bytes(), original_permissions.as_ref())?;
        }
        state.opencode = Some(ManagedOpenCode {
            provider_sha256: provider_hash,
            file_name: file_name(&path)?,
            created_file: state
                .opencode
                .as_ref()
                .is_some_and(|managed| managed.created_file)
                || created_file,
            created_provider_object: state
                .opencode
                .as_ref()
                .is_some_and(|managed| managed.created_provider_object)
                || created_provider_object,
        });
        if let Err(error) = self.save_state(&state) {
            rollback_file(&path, original.as_deref(), original_permissions.as_ref());
            return Err(error);
        }
        Ok(IntegrationChange {
            path,
            additional_paths: Vec::new(),
            backup,
            changed,
        })
    }

    pub(crate) fn unpersist_opencode(&self) -> Result<RemovalOutcome, PersistenceError> {
        let mut state = self.load_state()?;
        let Some(managed) = state.opencode.clone() else {
            return Ok(RemovalOutcome::NotConfigured);
        };
        validate_opencode_file_name(&managed.file_name)?;
        let path = self
            .home_directory
            .join(OPENCODE_CONFIG_DIRECTORY)
            .join(&managed.file_name);
        let original = read_optional(&path)?;
        let Some(contents) = original.as_deref() else {
            state.opencode = None;
            self.save_state(&state)?;
            return Ok(RemovalOutcome::Removed);
        };
        let original_permissions = permissions(&path)?;
        let source = String::from_utf8_lossy(contents);
        let root = parse_jsonc(&source, &path)?;
        let root_object = root
            .object_value()
            .ok_or_else(|| PersistenceError::RootIsNotObject(path.clone()))?;
        let Some(providers) = root_object.object_value("provider") else {
            state.opencode = None;
            self.save_state(&state)?;
            return Ok(RemovalOutcome::Removed);
        };
        let Some(provider) = providers.get("nan") else {
            state.opencode = None;
            self.save_state(&state)?;
            return Ok(RemovalOutcome::Removed);
        };
        let provider_value = provider
            .to_serde_value()
            .ok_or_else(|| PersistenceError::InvalidManagedProvider(path.clone()))?;
        if hash_json_value(&provider_value)? != managed.provider_sha256 {
            return Err(PersistenceError::ManagedProviderChanged(path));
        }

        provider.remove();
        if managed.created_provider_object && providers.properties().is_empty() {
            root_object
                .get("provider")
                .expect("provider property was resolved above")
                .remove();
        }
        let rendered = root.to_string();
        if managed.created_file
            && root_object.properties().is_empty()
            && empty_jsonc_object_is_disposable(&rendered)
        {
            fs::remove_file(&path).map_err(|source| PersistenceError::RemoveFile {
                path: path.clone(),
                source,
            })?;
        } else {
            write_private_file(&path, rendered.as_bytes(), original_permissions.as_ref())?;
        }
        state.opencode = None;
        if let Err(error) = self.save_state(&state) {
            rollback_file(&path, original.as_deref(), original_permissions.as_ref());
            return Err(error);
        }
        Ok(RemovalOutcome::Removed)
    }

    fn opencode_is_active(&self) -> bool {
        let Ok(state) = self.load_state() else {
            return false;
        };
        let Some(managed) = state.opencode else {
            return false;
        };
        if validate_opencode_file_name(&managed.file_name).is_err() {
            return false;
        }
        let path = self
            .home_directory
            .join(OPENCODE_CONFIG_DIRECTORY)
            .join(managed.file_name);
        let Ok(source) = fs::read_to_string(&path) else {
            return false;
        };
        let Ok(root) = parse_jsonc(&source, &path) else {
            return false;
        };
        root.object_value()
            .and_then(|object| object.object_value("provider"))
            .and_then(|providers| providers.get("nan"))
            .and_then(|provider| provider.to_serde_value())
            .and_then(|provider| hash_json_value(&provider).ok())
            .is_some_and(|hash| hash == managed.provider_sha256)
    }

    pub(crate) async fn persist_qwen_code(
        &self,
        config: &ResolvedConfig,
    ) -> Result<IntegrationChange, PersistenceError> {
        let models = discover_models(config).await?;
        let provider = qwen_code_provider(&models, &config.provider_base_url);
        let value_hash = hash_input_value(&provider)?;
        let mut state = self.load_state()?;
        let path = state.qwen_code.as_ref().map_or_else(
            || self.qwen_directory.join("settings.json"),
            |managed| managed.path.clone(),
        );
        let original = read_optional(&path)?;
        let original_permissions = permissions(&path)?;
        let created_file = original.is_none();
        let source = original.as_deref().map_or_else(
            || "{}\n".to_owned(),
            |value| String::from_utf8_lossy(value).into_owned(),
        );
        let root = parse_named_jsonc(&source, &path, "Qwen Code")?;
        let root_object = root.object_value_or_create().ok_or_else(|| {
            PersistenceError::ConfigRootIsNotObject {
                harness: "Qwen Code",
                path: path.clone(),
            }
        })?;
        let providers_property = root_object.get("modelProviders");
        let created_parent_object = providers_property.is_none();
        let providers =
            match providers_property {
                Some(property) => property.object_value().ok_or_else(|| {
                    PersistenceError::ConfigFieldIsNotObject {
                        harness: "Qwen Code",
                        field: "modelProviders",
                        path: path.clone(),
                    }
                })?,
                None => root_object.object_value_or_set("modelProviders"),
            };
        if let Some(existing) = providers.get("openai") {
            let existing_value = existing
                .to_serde_value()
                .ok_or_else(|| PersistenceError::InvalidManagedSection(path.clone()))?;
            let existing_hash = hash_json_value(&existing_value)?;
            match state.qwen_code.as_ref() {
                Some(managed) if managed.value_sha256 != existing_hash => {
                    return Err(PersistenceError::ManagedSectionChanged(path));
                }
                None if existing_hash != value_hash => {
                    return Err(PersistenceError::UnmanagedSectionConflict(path));
                }
                _ => existing.set_value(provider),
            }
        } else {
            providers.append("openai", provider);
        }
        let selected_auth_type = ensure_qwen_auth_selection(
            &root_object,
            &path,
            state
                .qwen_code
                .as_ref()
                .and_then(|managed| managed.selected_auth_type.as_ref()),
        )?;
        let rendered = root.to_string();
        let changed = original.as_deref() != Some(rendered.as_bytes());
        let backup = if state.qwen_code.is_none() && original.is_some() {
            create_backup(&path)?
        } else {
            None
        };
        if changed {
            write_private_file(&path, rendered.as_bytes(), original_permissions.as_ref())?;
        }
        state.qwen_code = Some(ManagedQwenCode {
            value_sha256: value_hash,
            path: path.clone(),
            created_file: state
                .qwen_code
                .as_ref()
                .is_some_and(|managed| managed.created_file)
                || created_file,
            created_parent_object: state
                .qwen_code
                .as_ref()
                .is_some_and(|managed| managed.created_parent_object)
                || created_parent_object,
            selected_auth_type,
        });
        if let Err(error) = self.save_state(&state) {
            rollback_file(&path, original.as_deref(), original_permissions.as_ref());
            return Err(error);
        }
        Ok(IntegrationChange {
            path,
            additional_paths: Vec::new(),
            backup,
            changed,
        })
    }

    pub(crate) fn unpersist_qwen_code(&self) -> Result<RemovalOutcome, PersistenceError> {
        let mut state = self.load_state()?;
        let Some(managed) = state.qwen_code.clone() else {
            return Ok(RemovalOutcome::NotConfigured);
        };
        let path = managed.path.clone();
        let Some(contents) = read_optional(&path)? else {
            state.qwen_code = None;
            self.save_state(&state)?;
            return Ok(RemovalOutcome::Removed);
        };
        let original_permissions = permissions(&path)?;
        let source = String::from_utf8_lossy(&contents);
        let root = parse_named_jsonc(&source, &path, "Qwen Code")?;
        let root_object =
            root.object_value()
                .ok_or_else(|| PersistenceError::ConfigRootIsNotObject {
                    harness: "Qwen Code",
                    path: path.clone(),
                })?;
        if let Some(providers) = root_object.object_value("modelProviders")
            && let Some(provider) = providers.get("openai")
        {
            let value = provider
                .to_serde_value()
                .ok_or_else(|| PersistenceError::InvalidManagedSection(path.clone()))?;
            if hash_json_value(&value)? != managed.value_sha256 {
                return Err(PersistenceError::ManagedSectionChanged(path));
            }
            provider.remove();
            if managed.created_parent_object && providers.properties().is_empty() {
                root_object
                    .get("modelProviders")
                    .expect("modelProviders was resolved above")
                    .remove();
            }
        }
        if let Some(auth_selection) = &managed.selected_auth_type {
            remove_qwen_auth_selection(&root_object, &path, auth_selection)?;
        }
        let rendered = root.to_string();
        if managed.created_file
            && root_object.properties().is_empty()
            && empty_jsonc_object_is_disposable(&rendered)
        {
            fs::remove_file(&path).map_err(|source| PersistenceError::RemoveFile {
                path: path.clone(),
                source,
            })?;
        } else {
            write_private_file(&path, rendered.as_bytes(), original_permissions.as_ref())?;
        }
        state.qwen_code = None;
        if let Err(error) = self.save_state(&state) {
            rollback_file(&path, Some(&contents), original_permissions.as_ref());
            return Err(error);
        }
        Ok(RemovalOutcome::Removed)
    }

    pub(crate) fn qwen_code_is_active(&self) -> bool {
        let Ok(state) = self.load_state() else {
            return false;
        };
        let Some(managed) = state.qwen_code else {
            return false;
        };
        let provider = ManagedJsonProperty {
            value_sha256: managed.value_sha256,
            path: managed.path.clone(),
            created_file: managed.created_file,
            created_parent_object: managed.created_parent_object,
        };
        managed_json_property_is_active(&provider, "modelProviders", "openai")
            && managed
                .selected_auth_type
                .as_ref()
                .is_none_or(|selection| qwen_auth_selection_is_active(&managed.path, selection))
    }

    pub(crate) async fn persist_deepseek_harness(
        &self,
        config: &ResolvedConfig,
    ) -> Result<IntegrationChange, PersistenceError> {
        let models = discover_models(config).await?;
        let body = deepseek_provider_settings(&models, &config.provider_base_url)?;
        let mut state = self.load_state()?;
        let path = state.deepseek_harness.as_ref().map_or_else(
            || self.deepseek_directory.join("settings.yaml"),
            |managed| managed.path.clone(),
        );
        let original = read_optional(&path)?;
        let original_permissions = permissions(&path)?;
        let source = optional_utf8(&path, original.as_deref())?;
        let (rendered, managed) = prepare_managed_block(
            &source,
            &path,
            &body,
            state.deepseek_harness.as_ref(),
            original.is_none(),
            ManagedBlockFormat {
                begin: DEEPSEEK_BLOCK_BEGIN,
                end: DEEPSEEK_BLOCK_END,
                conflicting_key: Some("llm-pi-ai:"),
            },
        )?;
        let changed = source != rendered;
        let backup = if state.deepseek_harness.is_none() && original.is_some() {
            create_backup(&path)?
        } else {
            None
        };
        if changed {
            write_private_file(&path, rendered.as_bytes(), original_permissions.as_ref())?;
        }
        state.deepseek_harness = Some(managed);
        if let Err(error) = self.save_state(&state) {
            rollback_file(&path, original.as_deref(), original_permissions.as_ref());
            return Err(error);
        }
        Ok(IntegrationChange {
            path,
            additional_paths: Vec::new(),
            backup,
            changed,
        })
    }

    pub(crate) fn unpersist_deepseek_harness(&self) -> Result<RemovalOutcome, PersistenceError> {
        let mut state = self.load_state()?;
        let Some(managed) = state.deepseek_harness.clone() else {
            return Ok(RemovalOutcome::NotConfigured);
        };
        let change =
            prepare_managed_block_removal(&managed, DEEPSEEK_BLOCK_BEGIN, DEEPSEEK_BLOCK_END)?;
        apply_prepared_file_change(&change)?;
        state.deepseek_harness = None;
        if let Err(error) = self.save_state(&state) {
            rollback_prepared_file_change(&change);
            return Err(error);
        }
        Ok(RemovalOutcome::Removed)
    }

    pub(crate) fn deepseek_harness_is_active(&self) -> bool {
        let Ok(state) = self.load_state() else {
            return false;
        };
        state.deepseek_harness.as_ref().is_some_and(|managed| {
            managed_block_is_active(managed, DEEPSEEK_BLOCK_BEGIN, DEEPSEEK_BLOCK_END)
        })
    }

    pub(crate) async fn persist_aider(
        &self,
        config: &ResolvedConfig,
    ) -> Result<IntegrationChange, PersistenceError> {
        let models = discover_models(config).await?;
        let settings_body = aider_model_settings(&models, &config.provider_base_url)?;
        let metadata_entries = aider_model_metadata(&models);
        let mut state = self.load_state()?;
        let settings_path = state.aider.as_ref().map_or_else(
            || self.home_directory.join(AIDER_SETTINGS_RELATIVE_PATH),
            |managed| managed.settings.path.clone(),
        );
        let metadata_path = state.aider.as_ref().map_or_else(
            || self.home_directory.join(AIDER_METADATA_RELATIVE_PATH),
            |managed| managed.metadata.path.clone(),
        );
        let original_settings = read_optional(&settings_path)?;
        let original_metadata = read_optional(&metadata_path)?;
        let settings_permissions = permissions(&settings_path)?;
        let metadata_permissions = permissions(&metadata_path)?;
        let settings_source = optional_utf8(&settings_path, original_settings.as_deref())?;
        let metadata_source = optional_utf8(&metadata_path, original_metadata.as_deref())?;
        let (rendered_settings, managed_settings) = prepare_managed_block(
            &settings_source,
            &settings_path,
            &settings_body,
            state.aider.as_ref().map(|managed| &managed.settings),
            original_settings.is_none(),
            ManagedBlockFormat {
                begin: AIDER_BLOCK_BEGIN,
                end: AIDER_BLOCK_END,
                conflicting_key: Some("name: nan/"),
            },
        )?;
        let (rendered_metadata, managed_metadata) = prepare_json_entries(
            &metadata_source,
            &metadata_path,
            &metadata_entries,
            state.aider.as_ref().map(|managed| &managed.metadata),
            original_metadata.is_none(),
        )?;
        let settings_changed = settings_source != rendered_settings;
        let metadata_changed = metadata_source != rendered_metadata;
        if settings_changed {
            write_private_file(
                &settings_path,
                rendered_settings.as_bytes(),
                settings_permissions.as_ref(),
            )?;
        }
        if metadata_changed
            && let Err(error) = write_private_file(
                &metadata_path,
                rendered_metadata.as_bytes(),
                metadata_permissions.as_ref(),
            )
        {
            rollback_file(
                &settings_path,
                original_settings.as_deref(),
                settings_permissions.as_ref(),
            );
            return Err(error);
        }
        state.aider = Some(ManagedAider {
            settings: managed_settings,
            metadata: managed_metadata,
        });
        if let Err(error) = self.save_state(&state) {
            rollback_file(
                &settings_path,
                original_settings.as_deref(),
                settings_permissions.as_ref(),
            );
            rollback_file(
                &metadata_path,
                original_metadata.as_deref(),
                metadata_permissions.as_ref(),
            );
            return Err(error);
        }
        Ok(IntegrationChange {
            path: settings_path,
            additional_paths: vec![metadata_path],
            backup: None,
            changed: settings_changed || metadata_changed,
        })
    }

    pub(crate) fn unpersist_aider(&self) -> Result<RemovalOutcome, PersistenceError> {
        let mut state = self.load_state()?;
        let Some(managed) = state.aider.clone() else {
            return Ok(RemovalOutcome::NotConfigured);
        };
        let settings_change =
            prepare_managed_block_removal(&managed.settings, AIDER_BLOCK_BEGIN, AIDER_BLOCK_END)?;
        let metadata_change = prepare_json_entries_removal(&managed.metadata)?;
        apply_prepared_file_change(&settings_change)?;
        if let Err(error) = apply_prepared_file_change(&metadata_change) {
            rollback_prepared_file_change(&settings_change);
            return Err(error);
        }
        state.aider = None;
        if let Err(error) = self.save_state(&state) {
            rollback_prepared_file_change(&settings_change);
            rollback_prepared_file_change(&metadata_change);
            return Err(error);
        }
        Ok(RemovalOutcome::Removed)
    }

    pub(crate) fn aider_is_active(&self) -> bool {
        let Ok(state) = self.load_state() else {
            return false;
        };
        state.aider.as_ref().is_some_and(|managed| {
            managed_block_is_active(&managed.settings, AIDER_BLOCK_BEGIN, AIDER_BLOCK_END)
                && managed_json_entries_are_active(&managed.metadata)
        })
    }

    fn opencode_config_path(
        &self,
        managed: Option<&ManagedOpenCode>,
    ) -> Result<PathBuf, PersistenceError> {
        let directory = self.home_directory.join(OPENCODE_CONFIG_DIRECTORY);
        if let Some(managed) = managed {
            validate_opencode_file_name(&managed.file_name)?;
            return Ok(directory.join(&managed.file_name));
        }
        let json = directory.join(OPENCODE_JSON);
        let jsonc = directory.join(OPENCODE_JSONC);
        match (json.exists(), jsonc.exists()) {
            (true, true) => Err(PersistenceError::AmbiguousOpenCodeConfig(directory)),
            (_, false) => Ok(json),
            (false, true) => Ok(jsonc),
        }
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
                let preferences: UserPreferences = serde_json::from_slice(&contents)
                    .map_err(PersistenceError::ParsePreferences)?;
                if preferences.schema_version != PREFERENCES_SCHEMA_VERSION {
                    return Err(PersistenceError::UnsupportedPreferencesSchema(
                        preferences.schema_version,
                    ));
                }
                Ok(preferences)
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

fn prepare_managed_block(
    source: &str,
    path: &Path,
    body: &str,
    managed: Option<&ManagedBlock>,
    created_file: bool,
    format: ManagedBlockFormat<'_>,
) -> Result<(String, ManagedBlock), PersistenceError> {
    let desired = format!(
        "{}\n{}{}\n",
        format.begin,
        ensure_trailing_newline(body),
        format.end
    );
    let desired_hash = sha256(desired.as_bytes());
    let current = managed_block_range(source, format.begin, format.end)?;
    let (rendered, added_separator) = if let Some(range) = current {
        let Some(managed) = managed else {
            return Err(PersistenceError::UnmanagedSectionConflict(
                path.to_path_buf(),
            ));
        };
        if sha256(source[range.clone()].as_bytes()) != managed.block_sha256 {
            return Err(PersistenceError::ManagedSectionChanged(path.to_path_buf()));
        }
        let mut rendered = source.to_owned();
        rendered.replace_range(range, &desired);
        (rendered, managed.added_separator)
    } else {
        if managed.is_some() {
            return Err(PersistenceError::ManagedSectionChanged(path.to_path_buf()));
        }
        if format
            .conflicting_key
            .is_some_and(|key| source.lines().any(|line| line.trim() == key))
        {
            return Err(PersistenceError::UnmanagedSectionConflict(
                path.to_path_buf(),
            ));
        }
        let added_separator = !source.is_empty() && !source.ends_with('\n');
        let mut rendered = source.to_owned();
        if added_separator {
            rendered.push('\n');
        }
        rendered.push_str(&desired);
        (rendered, added_separator)
    };
    Ok((
        rendered,
        ManagedBlock {
            block_sha256: desired_hash,
            path: path.to_path_buf(),
            created_file: managed.is_some_and(|managed| managed.created_file) || created_file,
            added_separator,
        },
    ))
}

fn prepare_json_entries(
    source: &str,
    path: &Path,
    desired: &BTreeMap<String, CstInputValue>,
    managed: Option<&ManagedJsonEntries>,
    created_file: bool,
) -> Result<(String, ManagedJsonEntries), PersistenceError> {
    let source = if source.is_empty() { "{}\n" } else { source };
    let root = parse_named_jsonc(source, path, "Aider")?;
    let object =
        root.object_value_or_create()
            .ok_or_else(|| PersistenceError::ConfigRootIsNotObject {
                harness: "Aider",
                path: path.to_path_buf(),
            })?;
    if let Some(managed) = managed {
        for (name, expected_hash) in &managed.entries {
            let property = object
                .get(name)
                .ok_or_else(|| PersistenceError::ManagedSectionChanged(path.to_path_buf()))?;
            let value = property
                .to_serde_value()
                .ok_or_else(|| PersistenceError::InvalidManagedSection(path.to_path_buf()))?;
            if hash_json_value(&value)? != *expected_hash {
                return Err(PersistenceError::ManagedSectionChanged(path.to_path_buf()));
            }
            if !desired.contains_key(name) {
                property.remove();
            }
        }
    } else if object.properties().iter().any(|property| {
        property
            .name()
            .and_then(|name| name.decoded_value().ok())
            .is_some_and(|name| name.starts_with("nan/"))
    }) {
        return Err(PersistenceError::UnmanagedSectionConflict(
            path.to_path_buf(),
        ));
    }
    let mut entries = BTreeMap::new();
    for (name, value) in desired {
        if let Some(existing) = object.get(name) {
            existing.set_value(value.clone());
        } else {
            object.append(name, value.clone());
        }
        entries.insert(name.clone(), hash_input_value(value)?);
    }
    Ok((
        root.to_string(),
        ManagedJsonEntries {
            entries,
            path: path.to_path_buf(),
            created_file: managed.is_some_and(|managed| managed.created_file) || created_file,
        },
    ))
}

fn prepare_managed_block_removal(
    managed: &ManagedBlock,
    begin: &str,
    end: &str,
) -> Result<PreparedFileChange, PersistenceError> {
    let original = read_optional(&managed.path)?;
    let original_permissions = permissions(&managed.path)?;
    let Some(contents) = original.as_deref() else {
        return Ok(PreparedFileChange {
            path: managed.path.clone(),
            original,
            original_permissions,
            replacement: None,
        });
    };
    let source = optional_utf8(&managed.path, Some(contents))?;
    let range = managed_block_range(&source, begin, end)?
        .ok_or_else(|| PersistenceError::ManagedSectionChanged(managed.path.clone()))?;
    if sha256(source[range.clone()].as_bytes()) != managed.block_sha256 {
        return Err(PersistenceError::ManagedSectionChanged(
            managed.path.clone(),
        ));
    }
    let mut rendered = source;
    let start = if managed.added_separator && range.start > 0 {
        range.start - 1
    } else {
        range.start
    };
    rendered.replace_range(start..range.end, "");
    let replacement = if managed.created_file && rendered.is_empty() {
        None
    } else {
        Some(rendered.into_bytes())
    };
    Ok(PreparedFileChange {
        path: managed.path.clone(),
        original,
        original_permissions,
        replacement,
    })
}

fn prepare_json_entries_removal(
    managed: &ManagedJsonEntries,
) -> Result<PreparedFileChange, PersistenceError> {
    let original = read_optional(&managed.path)?;
    let original_permissions = permissions(&managed.path)?;
    let Some(contents) = original.as_deref() else {
        return Ok(PreparedFileChange {
            path: managed.path.clone(),
            original,
            original_permissions,
            replacement: None,
        });
    };
    let source = optional_utf8(&managed.path, Some(contents))?;
    let root = parse_named_jsonc(&source, &managed.path, "Aider")?;
    let object = root
        .object_value()
        .ok_or_else(|| PersistenceError::ConfigRootIsNotObject {
            harness: "Aider",
            path: managed.path.clone(),
        })?;
    for (name, expected_hash) in &managed.entries {
        let Some(property) = object.get(name) else {
            continue;
        };
        let value = property
            .to_serde_value()
            .ok_or_else(|| PersistenceError::InvalidManagedSection(managed.path.clone()))?;
        if hash_json_value(&value)? != *expected_hash {
            return Err(PersistenceError::ManagedSectionChanged(
                managed.path.clone(),
            ));
        }
        property.remove();
    }
    let rendered = root.to_string();
    let replacement = if managed.created_file
        && object.properties().is_empty()
        && empty_jsonc_object_is_disposable(&rendered)
    {
        None
    } else {
        Some(rendered.into_bytes())
    };
    Ok(PreparedFileChange {
        path: managed.path.clone(),
        original,
        original_permissions,
        replacement,
    })
}

fn managed_block_is_active(managed: &ManagedBlock, begin: &str, end: &str) -> bool {
    let Ok(contents) = fs::read_to_string(&managed.path) else {
        return false;
    };
    managed_block_range(&contents, begin, end)
        .ok()
        .flatten()
        .is_some_and(|range| sha256(contents[range].as_bytes()) == managed.block_sha256)
}

fn managed_json_entries_are_active(managed: &ManagedJsonEntries) -> bool {
    let Ok(contents) = fs::read_to_string(&managed.path) else {
        return false;
    };
    let Ok(root) = parse_named_jsonc(&contents, &managed.path, "Aider") else {
        return false;
    };
    let Some(object) = root.object_value() else {
        return false;
    };
    managed.entries.iter().all(|(name, expected_hash)| {
        object
            .get(name)
            .and_then(|property| property.to_serde_value())
            .and_then(|value| hash_json_value(&value).ok())
            .is_some_and(|hash| hash == *expected_hash)
    })
}

fn managed_json_property_is_active(
    managed: &ManagedJsonProperty,
    parent: &str,
    property: &str,
) -> bool {
    let Ok(contents) = fs::read_to_string(&managed.path) else {
        return false;
    };
    let Ok(root) = parse_named_jsonc(&contents, &managed.path, "managed harness") else {
        return false;
    };
    root.object_value()
        .and_then(|object| object.object_value(parent))
        .and_then(|object| object.get(property))
        .and_then(|property| property.to_serde_value())
        .and_then(|value| hash_json_value(&value).ok())
        .is_some_and(|hash| hash == managed.value_sha256)
}

fn ensure_qwen_auth_selection(
    root: &CstObject,
    path: &Path,
    managed: Option<&ManagedQwenAuthSelection>,
) -> Result<Option<ManagedQwenAuthSelection>, PersistenceError> {
    if let Some(managed) = managed {
        let selected = root
            .object_value("security")
            .and_then(|security| security.object_value("auth"))
            .and_then(|auth| auth.get("selectedType"))
            .ok_or_else(|| PersistenceError::ManagedSectionChanged(path.to_path_buf()))?;
        let value = selected
            .to_serde_value()
            .ok_or_else(|| PersistenceError::InvalidManagedSection(path.to_path_buf()))?;
        if hash_json_value(&value)? != managed.value_sha256 {
            return Err(PersistenceError::ManagedSectionChanged(path.to_path_buf()));
        }
        return Ok(Some(managed.clone()));
    }

    let security_property = root.get("security");
    let created_security_object = security_property.is_none();
    let security = match security_property {
        Some(property) => {
            property
                .object_value()
                .ok_or_else(|| PersistenceError::ConfigFieldIsNotObject {
                    harness: "Qwen Code",
                    field: "security",
                    path: path.to_path_buf(),
                })?
        }
        None => root.object_value_or_set("security"),
    };
    let auth_property = security.get("auth");
    let created_auth_object = auth_property.is_none();
    let auth = match auth_property {
        Some(property) => {
            property
                .object_value()
                .ok_or_else(|| PersistenceError::ConfigFieldIsNotObject {
                    harness: "Qwen Code",
                    field: "security.auth",
                    path: path.to_path_buf(),
                })?
        }
        None => security.object_value_or_set("auth"),
    };
    if auth.get("selectedType").is_some() {
        return Ok(None);
    }
    let value = CstInputValue::String("openai".to_owned());
    let value_sha256 = hash_input_value(&value)?;
    auth.append("selectedType", value);
    Ok(Some(ManagedQwenAuthSelection {
        value_sha256,
        created_security_object,
        created_auth_object,
    }))
}

fn remove_qwen_auth_selection(
    root: &CstObject,
    path: &Path,
    managed: &ManagedQwenAuthSelection,
) -> Result<(), PersistenceError> {
    let security = root
        .object_value("security")
        .ok_or_else(|| PersistenceError::ManagedSectionChanged(path.to_path_buf()))?;
    let auth = security
        .object_value("auth")
        .ok_or_else(|| PersistenceError::ManagedSectionChanged(path.to_path_buf()))?;
    let selected = auth
        .get("selectedType")
        .ok_or_else(|| PersistenceError::ManagedSectionChanged(path.to_path_buf()))?;
    let value = selected
        .to_serde_value()
        .ok_or_else(|| PersistenceError::InvalidManagedSection(path.to_path_buf()))?;
    if hash_json_value(&value)? != managed.value_sha256 {
        return Err(PersistenceError::ManagedSectionChanged(path.to_path_buf()));
    }
    selected.remove();
    if managed.created_auth_object && auth.properties().is_empty() {
        security
            .get("auth")
            .expect("auth was resolved above")
            .remove();
    }
    if managed.created_security_object && security.properties().is_empty() {
        root.get("security")
            .expect("security was resolved above")
            .remove();
    }
    Ok(())
}

fn qwen_auth_selection_is_active(path: &Path, managed: &ManagedQwenAuthSelection) -> bool {
    let Ok(contents) = fs::read_to_string(path) else {
        return false;
    };
    let Ok(root) = parse_named_jsonc(&contents, path, "Qwen Code") else {
        return false;
    };
    root.object_value()
        .and_then(|root| root.object_value("security"))
        .and_then(|security| security.object_value("auth"))
        .and_then(|auth| auth.get("selectedType"))
        .and_then(|property| property.to_serde_value())
        .and_then(|value| hash_json_value(&value).ok())
        .is_some_and(|hash| hash == managed.value_sha256)
}

fn managed_block_range(
    source: &str,
    begin: &str,
    end: &str,
) -> Result<Option<Range<usize>>, PersistenceError> {
    let begins = source.match_indices(begin).collect::<Vec<_>>();
    let ends = source.match_indices(end).collect::<Vec<_>>();
    match (begins.as_slice(), ends.as_slice()) {
        ([], []) => Ok(None),
        ([(start, _)], [(end_start, _)]) if start < end_start => {
            let mut end_index = end_start + end.len();
            if source.as_bytes().get(end_index) == Some(&b'\n') {
                end_index += 1;
            }
            Ok(Some(*start..end_index))
        }
        _ => Err(PersistenceError::InvalidManagedBlock),
    }
}

fn ensure_trailing_newline(value: &str) -> String {
    if value.ends_with('\n') {
        value.to_owned()
    } else {
        format!("{value}\n")
    }
}

fn optional_utf8(path: &Path, value: Option<&[u8]>) -> Result<String, PersistenceError> {
    value.map_or_else(
        || Ok(String::new()),
        |contents| {
            String::from_utf8(contents.to_vec()).map_err(|source| PersistenceError::InvalidUtf8 {
                path: path.to_path_buf(),
                source,
            })
        },
    )
}

fn apply_prepared_file_change(change: &PreparedFileChange) -> Result<(), PersistenceError> {
    match change.replacement.as_deref() {
        Some(contents) => {
            write_private_file(&change.path, contents, change.original_permissions.as_ref())
        }
        None if change.path.exists() => {
            fs::remove_file(&change.path).map_err(|source| PersistenceError::RemoveFile {
                path: change.path.clone(),
                source,
            })
        }
        None => Ok(()),
    }
}

fn rollback_prepared_file_change(change: &PreparedFileChange) {
    rollback_file(
        &change.path,
        change.original.as_deref(),
        change.original_permissions.as_ref(),
    );
}

fn rollback_managed_change(change: &ManagedFileChange, path: &Path) {
    rollback_file(
        path,
        change.original.as_deref(),
        change.original_permissions.as_ref(),
    );
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
            CstInputValue::Object(vec![
                (
                    "apiKey".to_owned(),
                    CstInputValue::String("{env:NAN_API_KEY}".to_owned()),
                ),
                (
                    "baseURL".to_owned(),
                    CstInputValue::String(provider_base_url.to_owned()),
                ),
            ]),
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
