use super::Transport;
use crate::error::PlanError;
use crate::harness::DetectedHarness;
use crate::model::ResolvedModel;
use crate::secret::SecretRef;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

pub const BRIDGE_BASE_URL_PLACEHOLDER: &str = "{runtime:bridge_base_url}";
pub const FX_GATEWAY_CHAT_URL_PLACEHOLDER: &str = "{runtime:bridge_chat_url}";
pub const PROVIDER_BASE_URL_PLACEHOLDER: &str = "{runtime:provider_base_url}";
pub const CLAUDE_AVAILABLE_MODELS_PLACEHOLDER: &str = "{runtime:claude_available_models}";
pub const CLAUDE_MODEL_PRESENTATIONS_PLACEHOLDER: &str = "{runtime:claude_model_presentations}";
pub const CODEX_MODEL_CATALOG_PLACEHOLDER: &str = "{runtime:codex_model_catalog}";
pub const SELECTED_MODEL_REASONING_EFFORT_PLACEHOLDER: &str =
    "{runtime:selected_model_reasoning_effort}";
pub const AIDER_MODEL_METADATA_PLACEHOLDER: &str = "{runtime:aider_model_metadata}";
pub const AIDER_MODEL_SETTINGS_PLACEHOLDER: &str = "{runtime:aider_model_settings}";
pub const CLINE_MODEL_CATALOG_PLACEHOLDER: &str = "{runtime:cline_model_catalog}";
pub const DEEPSEEK_MODEL_CATALOG_PLACEHOLDER: &str = "{runtime:deepseek_model_catalog}";
pub const GOOSE_MODEL_CATALOG_PLACEHOLDER: &str = "{runtime:goose_model_catalog}";
pub const GOOSE_ADDITIONAL_CONFIG_FILES_PLACEHOLDER: &str =
    "{runtime:goose_additional_config_files}";
pub const HERMES_MODEL_CATALOG_PLACEHOLDER: &str = "{runtime:hermes_model_catalog}";
pub const OPENCODE_MODEL_CATALOG_PLACEHOLDER: &str = "{runtime:opencode_model_catalog}";
pub const OPENCLAW_MODEL_ALIASES_PLACEHOLDER: &str = "{runtime:openclaw_model_aliases}";
pub const OPENCLAW_MODEL_CATALOG_PLACEHOLDER: &str = "{runtime:openclaw_model_catalog}";
pub const PI_MODEL_CATALOG_PLACEHOLDER: &str = "{runtime:pi_model_catalog}";
pub const QWEN_CODE_MODEL_CATALOG_PLACEHOLDER: &str = "{runtime:qwen_code_model_catalog}";
pub const KIMI_CODE_MODEL_CATALOG_PLACEHOLDER: &str = "{runtime:kimi_code_model_catalog}";
pub const SELECTED_MODEL_DISPLAY_NAME_PLACEHOLDER: &str = "{runtime:selected_model_display_name}";
pub const SELECTED_MODEL_CONTEXT_WINDOW_PLACEHOLDER: &str =
    "{runtime:selected_model_context_window}";
pub const SELECTED_MODEL_MAX_OUTPUT_TOKENS_PLACEHOLDER: &str =
    "{runtime:selected_model_max_output_tokens}";
pub const SELECTED_MODEL_CAPABILITIES_PLACEHOLDER: &str = "{runtime:selected_model_capabilities}";
pub const USER_HOME_PLACEHOLDER: &str = "{runtime:user_home}";
pub const CODEX_HOME_PLACEHOLDER: &str = "{runtime:codex_home}";
pub const CODEX_HOME_OVERLAY_ID: &str = "codex-home";
pub const CODEX_HOME_ARTIFACT_PLACEHOLDER: &str = "{artifact:codex-home}";
pub const CODEX_PROFILE_ARTIFACT_ID: &str = "codex-profile";
pub const ARTIFACT_PLACEHOLDER_PREFIX: &str = "{artifact:";
pub const NAN_SEARCH_BLOCK_BEGIN: &str = "{runtime:nan_search:begin}";
pub const NAN_SEARCH_BLOCK_END: &str = "{runtime:nan_search:end}";

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LaunchId(String);

impl LaunchId {
    /// Creates a validated launch identifier.
    ///
    /// # Errors
    ///
    /// Returns [`PlanError`] when the value does not match the launch ID format.
    pub fn new(value: impl Into<String>) -> Result<Self, PlanError> {
        let value = value.into();
        if is_valid_launch_id(&value) {
            Ok(Self(value))
        } else {
            Err(PlanError::InvalidField {
                field: "launchId",
                message: "must match ^launch_[a-z0-9]{12,64}$".to_owned(),
            })
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for LaunchId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("LaunchId").field(&self.0).finish()
    }
}

impl fmt::Display for LaunchId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Serialize for LaunchId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for LaunchId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TerminalMode {
    Inherit,
    Captured,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessSpec {
    pub arguments: Vec<String>,
    pub working_directory: String,
    pub terminal: TerminalMode,
    pub forward_signals: bool,
    pub preserve_exit_code: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvironmentOverlay {
    pub public: BTreeMap<String, String>,
    pub secrets: BTreeMap<String, SecretRef>,
    pub remove: BTreeSet<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TemporaryArtifactKind {
    File,
    Directory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TemporaryArtifactMode {
    #[serde(rename = "0600")]
    OwnerFile,
    #[serde(rename = "0700")]
    OwnerDirectory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArtifactLifecycle {
    Launch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemporaryArtifact {
    pub id: String,
    pub kind: TemporaryArtifactKind,
    pub path_hint: String,
    pub mode: TemporaryArtifactMode,
    pub content_template: Option<String>,
    pub lifecycle: ArtifactLifecycle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OverlayFilePolicy {
    Replace,
    Preserve,
    Copy,
    CopyBinary,
    MergeJson,
    MergeToml,
    MergeYaml,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayFile {
    pub path: String,
    pub mode: TemporaryArtifactMode,
    pub content_template: String,
    pub policy: OverlayFilePolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigurationOverlay {
    pub id: String,
    pub path_hint: String,
    pub source_path: String,
    pub files: Vec<OverlayFile>,
    pub lifecycle: ArtifactLifecycle,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchScopedFile {
    pub id: String,
    pub directory: String,
    pub file_name: String,
    pub ownership_prefix: String,
    pub mode: TemporaryArtifactMode,
    pub content_template: String,
    pub lifecycle: ArtifactLifecycle,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupPolicy {
    pub terminate_bridge: bool,
    pub delete_temporary_artifacts: bool,
    pub grace_period_ms: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ObservabilityFormat {
    Human,
    Json,
    Quiet,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WebSearchPolicy {
    #[default]
    Auto,
    Disabled,
    Force,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservabilityPolicy {
    pub format: ObservabilityFormat,
    pub payload_capture: bool,
    pub redact_environment_names: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchPlan {
    pub schema_version: u8,
    pub launch_id: LaunchId,
    pub harness: DetectedHarness,
    pub model: ResolvedModel,
    pub web_search_policy: WebSearchPolicy,
    pub transport: Transport,
    pub process: ProcessSpec,
    pub environment: EnvironmentOverlay,
    pub temporary_artifacts: Vec<TemporaryArtifact>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub configuration_overlays: Vec<ConfigurationOverlay>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub launch_scoped_files: Vec<LaunchScopedFile>,
    pub cleanup: CleanupPolicy,
    pub observability: ObservabilityPolicy,
}

fn is_valid_launch_id(value: &str) -> bool {
    let Some(suffix) = value.strip_prefix("launch_") else {
        return false;
    };
    (12..=64).contains(&suffix.len())
        && suffix
            .chars()
            .all(|character| character.is_ascii_lowercase() || character.is_ascii_digit())
}
