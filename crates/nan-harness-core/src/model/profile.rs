use super::metadata::{
    CodingModelMetadata, GENERIC_CODING_MODEL_CONTEXT_WINDOW, GENERIC_CODING_MODEL_DESCRIPTION,
    GENERIC_CODING_MODEL_MAX_OUTPUT_TOKENS,
};
use super::reasoning::ReasoningPolicy;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingModelProfile {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub context_window: u64,
    pub max_output_tokens: u64,
    pub image_input: bool,
    pub reasoning: ReasoningPolicy,
    pub source: ProfileSource,
}

impl CodingModelProfile {
    #[must_use]
    pub fn generic(model_id: &str) -> Self {
        Self {
            id: model_id.to_owned(),
            display_name: format!("NaN · {model_id}"),
            description: GENERIC_CODING_MODEL_DESCRIPTION.to_owned(),
            context_window: GENERIC_CODING_MODEL_CONTEXT_WINDOW,
            max_output_tokens: GENERIC_CODING_MODEL_MAX_OUTPUT_TOKENS,
            image_input: false,
            reasoning: ReasoningPolicy::Unknown,
            source: ProfileSource::Generic,
        }
    }
}

impl From<&CodingModelMetadata> for CodingModelProfile {
    fn from(metadata: &CodingModelMetadata) -> Self {
        Self {
            id: metadata.id.to_owned(),
            display_name: metadata.display_name.to_owned(),
            description: metadata.description.to_owned(),
            context_window: metadata.context_window,
            max_output_tokens: metadata.max_output_tokens,
            image_input: metadata.image_input,
            reasoning: metadata.reasoning,
            source: ProfileSource::Bundled,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProfileSource {
    UserOverride,
    Bundled,
    Generic,
}
