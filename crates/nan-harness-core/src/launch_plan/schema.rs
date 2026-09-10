use super::Transport;
use crate::desktop::DesktopHarnessKind;
use crate::error::PlanError;
use crate::harness::DetectedHarness;
use crate::harness::HarnessKind;
use crate::model::ResolvedModel;
use crate::secret::SecretRef;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

pub const BRIDGE_BASE_URL_PLACEHOLDER: &str = "{runtime:bridge_base_url}";
pub const FX_GATEWAY_CHAT_URL_PLACEHOLDER: &str = "{runtime:bridge_chat_url}";
pub const PROVIDER_BASE_URL_PLACEHOLDER: &str = "{runtime:provider_base_url}";
pub const CLAUDE_AVAILABLE_MODELS_PLACEHOLDER: &str = "{runtime:claude_available_models}";
pub const CLAUDE_MODEL_PICKER_PLACEHOLDER: &str = "{runtime:claude_model_picker}";
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

/// The native compaction setting derived for one launch.
///
/// The requested value is an approximate trigger. Native harnesses may compact
/// earlier to preserve their own safety margin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContextLimit {
    pub requested_tokens: u64,
    pub effective_context_window: u64,
    pub native: NativeContextLimit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum NativeContextLimit {
    ClaudeAutoCompactPercent { percent: u64 },
    CodexTokenLimit { tokens: u64 },
    OpenCodeBuffer { buffer_tokens: u64 },
    HermesThreshold { threshold_tokens: u64 },
    PiReserve { reserve_tokens: u64 },
    OmpThreshold { threshold_tokens: u64 },
    QwenFraction { fraction_millionths: u64 },
    KimiReserve { reserved_context_size: u64 },
    AiderHistory { max_chat_history_tokens: u64 },
    GooseFraction { fraction_millionths: u64 },
    ZedThreshold { threshold: u64 },
}

impl ContextLimit {
    /// Derives a native setting for an experimental Desktop surface.
    ///
    /// # Errors
    ///
    /// Returns [`PlanError`] when the requested threshold is invalid or the
    /// desktop surface has no supported native compaction override.
    pub fn for_desktop(
        kind: DesktopHarnessKind,
        requested_tokens: u64,
        effective_context_window: u64,
    ) -> Result<Self, PlanError> {
        if requested_tokens == 0 {
            return Err(PlanError::InvalidField {
                field: "context",
                message: "must be a positive token count".to_owned(),
            });
        }
        if effective_context_window == 0 || requested_tokens >= effective_context_window {
            return Err(PlanError::InvalidField {
                field: "context",
                message: format!(
                    "must be less than the effective context window ({effective_context_window} tokens)"
                ),
            });
        }
        let native = match kind {
            DesktopHarnessKind::Hermes => NativeContextLimit::HermesThreshold {
                threshold_tokens: requested_tokens,
            },
            DesktopHarnessKind::Zed => NativeContextLimit::ZedThreshold {
                threshold: requested_tokens,
            },
            _ => {
                return Err(PlanError::InvalidField {
                    field: "context",
                    message: format!("{kind} does not support native compaction overrides"),
                });
            }
        };
        Ok(Self {
            requested_tokens,
            effective_context_window,
            native,
        })
    }

    /// Derives a native setting from the starting model's effective window.
    ///
    /// # Errors
    ///
    /// Returns [`PlanError`] when the requested threshold cannot be represented
    /// without disabling the native compaction safeguard.
    pub fn for_harness(
        kind: HarnessKind,
        requested_tokens: u64,
        effective_context_window: u64,
    ) -> Result<Self, PlanError> {
        if requested_tokens == 0 {
            return Err(PlanError::InvalidField {
                field: "context",
                message: "must be a positive token count".to_owned(),
            });
        }
        if effective_context_window == 0 || requested_tokens >= effective_context_window {
            return Err(PlanError::InvalidField {
                field: "context",
                message: format!(
                    "must be less than the effective context window ({effective_context_window} tokens)"
                ),
            });
        }
        let native = match kind {
            HarnessKind::ClaudeCode => NativeContextLimit::ClaudeAutoCompactPercent {
                percent: ceil_percent(requested_tokens, effective_context_window).min(99),
            },
            HarnessKind::Codex => NativeContextLimit::CodexTokenLimit {
                tokens: requested_tokens,
            },
            HarnessKind::OpenCode => NativeContextLimit::OpenCodeBuffer {
                buffer_tokens: effective_context_window - requested_tokens,
            },
            HarnessKind::Hermes => NativeContextLimit::HermesThreshold {
                threshold_tokens: requested_tokens,
            },
            HarnessKind::Pi | HarnessKind::PrimeAgent => NativeContextLimit::PiReserve {
                reserve_tokens: effective_context_window - requested_tokens,
            },
            HarnessKind::Omp => NativeContextLimit::OmpThreshold {
                threshold_tokens: requested_tokens,
            },
            HarnessKind::QwenCode => NativeContextLimit::QwenFraction {
                fraction_millionths: fraction_millionths(
                    requested_tokens,
                    effective_context_window,
                ),
            },
            HarnessKind::KimiCode => NativeContextLimit::KimiReserve {
                reserved_context_size: effective_context_window - requested_tokens,
            },
            HarnessKind::Aider => NativeContextLimit::AiderHistory {
                max_chat_history_tokens: requested_tokens,
            },
            HarnessKind::Goose => NativeContextLimit::GooseFraction {
                fraction_millionths: fraction_millionths(
                    requested_tokens,
                    effective_context_window,
                ),
            },
            HarnessKind::DeepSeekHarness
            | HarnessKind::OpenClaw
            | HarnessKind::Cline
            | HarnessKind::Fx => {
                return Err(PlanError::InvalidField {
                    field: "context",
                    message: format!("{kind} does not support native compaction overrides"),
                });
            }
        };
        Ok(Self {
            requested_tokens,
            effective_context_window,
            native,
        })
    }

    #[must_use]
    pub fn fraction(&self) -> Option<String> {
        let (NativeContextLimit::QwenFraction {
            fraction_millionths: millionths,
        }
        | NativeContextLimit::GooseFraction {
            fraction_millionths: millionths,
        }) = self.native
        else {
            return None;
        };
        Some(format!(
            "{}.{:06}",
            millionths / 1_000_000,
            millionths % 1_000_000
        ))
    }
}

fn ceil_percent(value: u64, denominator: u64) -> u64 {
    value.saturating_mul(100).saturating_add(denominator - 1) / denominator
}

fn fraction_millionths(value: u64, denominator: u64) -> u64 {
    value
        .saturating_mul(1_000_000)
        .saturating_add(denominator / 2)
        / denominator
}

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_max_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_limit: Option<ContextLimit>,
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
