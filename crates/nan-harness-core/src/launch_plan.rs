use crate::error::PlanError;
use crate::harness::{DetectedHarness, HarnessKind};
use crate::model::ResolvedModel;
use crate::secret::SecretRef;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Component, Path};

pub const BRIDGE_BASE_URL_PLACEHOLDER: &str = "{runtime:bridge_base_url}";
pub const PROVIDER_BASE_URL_PLACEHOLDER: &str = "{runtime:provider_base_url}";
pub const CLAUDE_AVAILABLE_MODELS_PLACEHOLDER: &str = "{runtime:claude_available_models}";
pub const CODEX_MODEL_CATALOG_PLACEHOLDER: &str = "{runtime:codex_model_catalog}";
pub const USER_HOME_PLACEHOLDER: &str = "{runtime:user_home}";
pub const ARTIFACT_PLACEHOLDER_PREFIX: &str = "{artifact:";

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportKind {
    DirectChat,
    AnthropicBridge,
    ResponsesBridge,
}

impl fmt::Display for TransportKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::DirectChat => "direct-chat",
            Self::AnthropicBridge => "anthropic-bridge",
            Self::ResponsesBridge => "responses-bridge",
        };
        formatter.write_str(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Protocol {
    ChatCompletions,
    AnthropicMessages,
    OpenAiResponses,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListenAddress {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum Transport {
    DirectChat {
        protocol: Protocol,
        base_url: String,
        credential_target: String,
    },
    AnthropicBridge {
        client_protocol: Protocol,
        upstream_protocol: Protocol,
        listen: ListenAddress,
        provider_credential_ref: SecretRef,
        session_token_ref: SecretRef,
    },
    ResponsesBridge {
        client_protocol: Protocol,
        upstream_protocol: Protocol,
        listen: ListenAddress,
        provider_credential_ref: SecretRef,
        session_token_ref: SecretRef,
    },
}

impl Transport {
    #[must_use]
    pub const fn kind(&self) -> TransportKind {
        match self {
            Self::DirectChat { .. } => TransportKind::DirectChat,
            Self::AnthropicBridge { .. } => TransportKind::AnthropicBridge,
            Self::ResponsesBridge { .. } => TransportKind::ResponsesBridge,
        }
    }

    #[must_use]
    pub const fn is_bridge(&self) -> bool {
        !matches!(self, Self::DirectChat { .. })
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
    MergeJson,
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
    pub transport: Transport,
    pub process: ProcessSpec,
    pub environment: EnvironmentOverlay,
    pub temporary_artifacts: Vec<TemporaryArtifact>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub configuration_overlays: Vec<ConfigurationOverlay>,
    pub cleanup: CleanupPolicy,
    pub observability: ObservabilityPolicy,
}

pub struct LaunchPlanValidator;

impl LaunchPlanValidator {
    /// Checks all schema version 1 launch-plan invariants.
    ///
    /// # Errors
    ///
    /// Returns [`PlanError`] for the first invalid or unsafe field.
    pub fn validate(plan: &LaunchPlan) -> Result<(), PlanError> {
        validate_required_fields(plan)?;
        validate_transport(plan)?;
        validate_environment(plan)?;
        validate_artifacts(plan)?;
        validate_configuration_overlays(plan)?;
        validate_cleanup(plan)?;
        validate_observability(plan)
    }
}

fn validate_required_fields(plan: &LaunchPlan) -> Result<(), PlanError> {
    if plan.schema_version != 1 {
        return invalid("schemaVersion", "only schema version 1 is supported");
    }
    if plan.harness.executable.is_empty() {
        return invalid("harness.executable", "cannot be empty");
    }
    if plan.harness.detected_version.is_empty() {
        return invalid("harness.detectedVersion", "cannot be empty");
    }
    if plan.model.requested_id.is_empty() || plan.model.resolved_id.is_empty() {
        return invalid("model", "requested and resolved IDs cannot be empty");
    }
    if !Path::new(&plan.process.working_directory).is_absolute() {
        return invalid("process.workingDirectory", "must be an absolute path");
    }
    Ok(())
}

fn validate_transport(plan: &LaunchPlan) -> Result<(), PlanError> {
    let expected = match plan.harness.kind {
        HarnessKind::ClaudeCode => TransportKind::AnthropicBridge,
        HarnessKind::Codex | HarnessKind::RooCode => TransportKind::ResponsesBridge,
        HarnessKind::OpenCode
        | HarnessKind::Hermes
        | HarnessKind::Pi
        | HarnessKind::PrimeAgent
        | HarnessKind::DeepSeekHarness
        | HarnessKind::OpenClaw
        | HarnessKind::Cline
        | HarnessKind::QwenCode
        | HarnessKind::Aider
        | HarnessKind::Goose => TransportKind::DirectChat,
    };
    let actual = plan.transport.kind();
    if actual != expected {
        return Err(PlanError::TransportMismatch {
            harness: plan.harness.kind,
            expected,
            actual,
        });
    }

    match &plan.transport {
        Transport::DirectChat {
            protocol,
            base_url,
            credential_target,
        } => {
            if protocol != &Protocol::ChatCompletions {
                return invalid(
                    "transport.protocol",
                    "direct transport requires chat-completions",
                );
            }
            if base_url != PROVIDER_BASE_URL_PLACEHOLDER && !is_http_url(base_url) {
                return invalid("transport.baseUrl", "must be an HTTP or HTTPS URL");
            }
            if !plan.environment.secrets.contains_key(credential_target) {
                return Err(PlanError::MissingSecretReference {
                    reference: credential_target.clone(),
                });
            }
        }
        Transport::AnthropicBridge {
            client_protocol,
            upstream_protocol,
            listen,
            session_token_ref,
            ..
        } => {
            validate_bridge_protocols(
                *client_protocol,
                *upstream_protocol,
                listen,
                Protocol::AnthropicMessages,
                Protocol::ChatCompletions,
            )?;
            validate_child_secret_ref(&plan.environment, session_token_ref)?;
        }
        Transport::ResponsesBridge {
            client_protocol,
            upstream_protocol,
            listen,
            session_token_ref,
            ..
        } => {
            validate_bridge_protocols(
                *client_protocol,
                *upstream_protocol,
                listen,
                Protocol::OpenAiResponses,
                Protocol::ChatCompletions,
            )?;
            validate_child_secret_ref(&plan.environment, session_token_ref)?;
        }
    }
    Ok(())
}

fn validate_bridge_protocols(
    client: Protocol,
    upstream: Protocol,
    listen: &ListenAddress,
    expected_client: Protocol,
    expected_upstream: Protocol,
) -> Result<(), PlanError> {
    if client != expected_client || upstream != expected_upstream {
        return invalid(
            "transport",
            "bridge protocols do not match the selected bridge",
        );
    }
    if listen.host != "127.0.0.1" {
        return invalid("transport.listen.host", "bridges must bind to 127.0.0.1");
    }
    Ok(())
}

fn validate_child_secret_ref(
    environment: &EnvironmentOverlay,
    reference: &SecretRef,
) -> Result<(), PlanError> {
    if environment.secrets.values().any(|value| value == reference) {
        Ok(())
    } else {
        Err(PlanError::MissingSecretReference {
            reference: reference.to_string(),
        })
    }
}

fn validate_environment(plan: &LaunchPlan) -> Result<(), PlanError> {
    for variable in plan
        .environment
        .public
        .keys()
        .chain(plan.environment.secrets.keys())
        .chain(plan.environment.remove.iter())
    {
        if !is_valid_environment_name(variable) {
            return invalid(
                "environment",
                format!("'{variable}' is not a valid variable name"),
            );
        }
    }

    for variable in plan.environment.public.keys() {
        if plan.environment.secrets.contains_key(variable)
            || plan.environment.remove.contains(variable)
        {
            return Err(PlanError::ConflictingEnvironment {
                variable: variable.clone(),
            });
        }
    }
    for variable in plan.environment.secrets.keys() {
        if plan.environment.remove.contains(variable) {
            return Err(PlanError::ConflictingEnvironment {
                variable: variable.clone(),
            });
        }
        if !plan
            .observability
            .redact_environment_names
            .contains(variable)
        {
            return invalid(
                "observability.redactEnvironmentNames",
                format!("must include secret environment variable '{variable}'"),
            );
        }
    }
    Ok(())
}

fn validate_artifacts(plan: &LaunchPlan) -> Result<(), PlanError> {
    let mut ids = BTreeSet::new();
    for artifact in &plan.temporary_artifacts {
        if !ids.insert(artifact.id.clone()) {
            return Err(PlanError::UnsafeTemporaryArtifact {
                artifact_id: artifact.id.clone(),
                reason: "artifact IDs must be unique".to_owned(),
            });
        }
        if !is_valid_artifact_id(&artifact.id) {
            return unsafe_artifact(artifact, "ID must match ^[a-z][a-z0-9_-]{2,63}$");
        }
        if !is_safe_path_hint(&artifact.path_hint) {
            return unsafe_artifact(artifact, "pathHint must be one relative path component");
        }
        match (artifact.kind, artifact.mode, &artifact.content_template) {
            (TemporaryArtifactKind::File, TemporaryArtifactMode::OwnerFile, Some(_))
            | (TemporaryArtifactKind::Directory, TemporaryArtifactMode::OwnerDirectory, None) => {}
            _ => {
                return unsafe_artifact(
                    artifact,
                    "files require mode 0600 and content; directories require mode 0700 and no content",
                );
            }
        }
        validate_template_placeholders(plan, &artifact.id, artifact.content_template.as_deref())?;
    }

    ids.extend(
        plan.configuration_overlays
            .iter()
            .map(|overlay| overlay.id.clone()),
    );

    for (field, value) in plan
        .process
        .arguments
        .iter()
        .map(|value| ("process.arguments", value))
        .chain(
            plan.environment
                .public
                .values()
                .map(|value| ("environment.public", value)),
        )
    {
        let artifact_ids = artifact_placeholders(value).ok_or_else(|| PlanError::InvalidField {
            field,
            message: format!("contains malformed artifact placeholder '{value}'"),
        })?;
        for artifact_id in artifact_ids {
            if !ids.contains(artifact_id) {
                return invalid(
                    field,
                    format!("references unknown temporary artifact '{artifact_id}'"),
                );
            }
        }
    }
    Ok(())
}

fn validate_template_placeholders(
    plan: &LaunchPlan,
    resource_id: &str,
    template: Option<&str>,
) -> Result<(), PlanError> {
    let Some(template) = template else {
        return Ok(());
    };
    let mut remainder = template
        .replace(BRIDGE_BASE_URL_PLACEHOLDER, "")
        .replace(PROVIDER_BASE_URL_PLACEHOLDER, "")
        .replace(CLAUDE_AVAILABLE_MODELS_PLACEHOLDER, "")
        .replace(CODEX_MODEL_CATALOG_PLACEHOLDER, "")
        .replace(USER_HOME_PLACEHOLDER, "");

    if let Some(session_token_ref) = session_token_reference(&plan.transport) {
        remainder = remainder.replace(&format!("{{secret:{}}}", session_token_ref.as_str()), "");
    }

    if remainder.contains("{runtime:") || remainder.contains("{secret:") {
        unsafe_resource(
            resource_id,
            "contentTemplate contains an unknown runtime or secret placeholder",
        )
    } else {
        Ok(())
    }
}

fn validate_configuration_overlays(plan: &LaunchPlan) -> Result<(), PlanError> {
    let mut ids = plan
        .temporary_artifacts
        .iter()
        .map(|artifact| artifact.id.clone())
        .collect::<BTreeSet<_>>();
    for overlay in &plan.configuration_overlays {
        if !ids.insert(overlay.id.clone()) {
            return Err(PlanError::UnsafeTemporaryArtifact {
                artifact_id: overlay.id.clone(),
                reason: "temporary resource IDs must be unique".to_owned(),
            });
        }
        if !is_valid_artifact_id(&overlay.id) {
            return unsafe_resource(&overlay.id, "ID must match ^[a-z][a-z0-9_-]{2,63}$");
        }
        if !is_safe_path_hint(&overlay.path_hint) {
            return unsafe_resource(&overlay.id, "pathHint must be one relative path component");
        }
        if !is_safe_user_home_path(&overlay.source_path) {
            return unsafe_resource(
                &overlay.id,
                "sourcePath must be {runtime:user_home} or a safe path below it",
            );
        }
        let mut paths = BTreeSet::new();
        for file in &overlay.files {
            if !is_safe_relative_path(&file.path) {
                return unsafe_resource(
                    &overlay.id,
                    "overlay file paths must be relative and safe",
                );
            }
            let file_path = Path::new(&file.path);
            if paths.iter().any(|existing: &String| {
                let existing_path = Path::new(existing);
                existing_path.starts_with(file_path) || file_path.starts_with(existing_path)
            }) {
                return unsafe_resource(
                    &overlay.id,
                    "overlay file paths cannot contain one another",
                );
            }
            if !paths.insert(file.path.clone()) {
                return unsafe_resource(&overlay.id, "overlay file paths must be unique");
            }
            if file.mode != TemporaryArtifactMode::OwnerFile {
                return unsafe_resource(&overlay.id, "overlay files require mode 0600");
            }
            validate_template_placeholders(plan, &overlay.id, Some(&file.content_template))?;
        }
    }
    Ok(())
}

fn session_token_reference(transport: &Transport) -> Option<&SecretRef> {
    match transport {
        Transport::AnthropicBridge {
            session_token_ref, ..
        }
        | Transport::ResponsesBridge {
            session_token_ref, ..
        } => Some(session_token_ref),
        Transport::DirectChat { .. } => None,
    }
}

fn artifact_placeholders(mut value: &str) -> Option<Vec<&str>> {
    let mut placeholders = Vec::new();
    while let Some(start) = value.find(ARTIFACT_PLACEHOLDER_PREFIX) {
        let remainder = &value[start + ARTIFACT_PLACEHOLDER_PREFIX.len()..];
        let end = remainder.find('}')?;
        let artifact_id = &remainder[..end];
        if artifact_id.is_empty() || artifact_id.contains(['{', '}']) {
            return None;
        }
        placeholders.push(artifact_id);
        value = &remainder[end + 1..];
    }
    Some(placeholders)
}

fn validate_cleanup(plan: &LaunchPlan) -> Result<(), PlanError> {
    if plan.cleanup.grace_period_ms > 30_000 {
        return invalid("cleanup.gracePeriodMs", "cannot exceed 30000");
    }
    if plan.transport.is_bridge() != plan.cleanup.terminate_bridge {
        return invalid(
            "cleanup.terminateBridge",
            "must be true exactly when the selected transport uses a bridge",
        );
    }
    if (!plan.temporary_artifacts.is_empty() || !plan.configuration_overlays.is_empty())
        && !plan.cleanup.delete_temporary_artifacts
    {
        return invalid(
            "cleanup.deleteTemporaryArtifacts",
            "must be true when the plan creates temporary artifacts",
        );
    }
    Ok(())
}

fn validate_observability(plan: &LaunchPlan) -> Result<(), PlanError> {
    if plan.observability.payload_capture {
        invalid(
            "observability.payloadCapture",
            "payload capture is forbidden in schema version 1",
        )
    } else {
        Ok(())
    }
}

fn invalid(field: &'static str, message: impl Into<String>) -> Result<(), PlanError> {
    Err(PlanError::InvalidField {
        field,
        message: message.into(),
    })
}

fn unsafe_artifact(
    artifact: &TemporaryArtifact,
    reason: impl Into<String>,
) -> Result<(), PlanError> {
    unsafe_resource(&artifact.id, reason)
}

fn unsafe_resource(resource_id: &str, reason: impl Into<String>) -> Result<(), PlanError> {
    Err(PlanError::UnsafeTemporaryArtifact {
        artifact_id: resource_id.to_owned(),
        reason: reason.into(),
    })
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

fn is_http_url(value: &str) -> bool {
    (value.starts_with("http://") || value.starts_with("https://"))
        && !value.chars().any(char::is_whitespace)
        && value
            .split_once("://")
            .is_some_and(|(_, rest)| !rest.is_empty())
}

fn is_valid_environment_name(value: &str) -> bool {
    let mut characters = value.chars();
    let Some(first) = characters.next() else {
        return false;
    };
    (first.is_ascii_uppercase() || first == '_')
        && characters.all(|character| {
            character.is_ascii_uppercase() || character.is_ascii_digit() || character == '_'
        })
}

fn is_valid_artifact_id(value: &str) -> bool {
    let mut characters = value.chars();
    let Some(first) = characters.next() else {
        return false;
    };
    (3..=64).contains(&value.len())
        && first.is_ascii_lowercase()
        && characters.all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || character == '_'
                || character == '-'
        })
}

fn is_safe_user_home_path(value: &str) -> bool {
    if value == USER_HOME_PLACEHOLDER {
        return true;
    }
    value
        .strip_prefix(USER_HOME_PLACEHOLDER)
        .and_then(|suffix| suffix.strip_prefix('/'))
        .is_some_and(is_safe_relative_path)
}

fn is_safe_relative_path(value: &str) -> bool {
    let path = Path::new(value);
    !value.is_empty()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

fn is_safe_path_hint(value: &str) -> bool {
    let mut components = Path::new(value).components();
    matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none()
}
