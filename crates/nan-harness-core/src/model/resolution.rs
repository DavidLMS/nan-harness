use super::profile::ProfileSource;
use super::qualification::QualificationStatus;
use super::reasoning::ReasoningSelection;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ModelAvailability {
    Discovered,
    ExplicitUndiscovered,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedModel {
    pub requested_id: String,
    pub resolved_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_selection: Option<ReasoningSelection>,
    pub availability: ModelAvailability,
    pub profile_source: ProfileSource,
    pub qualification: QualificationStatus,
    pub warnings: Vec<String>,
}

#[must_use]
pub fn is_known_non_coding_model(model_id: &str) -> bool {
    super::metadata::KNOWN_NON_CODING_MODELS.contains(&model_id)
}

#[must_use]
pub fn is_valid_provider_model_id(model_id: &str) -> bool {
    !model_id.is_empty()
        && model_id.len() <= 256
        && model_id.trim() == model_id
        && !model_id.chars().any(char::is_control)
}

#[must_use]
pub fn claude_gateway_model_id(provider_model_id: &str) -> String {
    format!(
        "{}{}",
        super::metadata::CLAUDE_GATEWAY_MODEL_PREFIX,
        provider_model_id
    )
}
