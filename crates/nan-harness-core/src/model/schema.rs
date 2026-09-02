use super::profile::ProfileSource;
use super::qualification::QualificationMatrix;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InputModality {
    Text,
    Image,
    Audio,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatMaxTokensField {
    MaxTokens,
    MaxCompletionTokens,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelLimits {
    pub context_tokens: Option<u64>,
    pub max_output_tokens: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelCapabilities {
    pub input: BTreeSet<InputModality>,
    pub streaming: bool,
    pub tools: bool,
    pub reasoning: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCompatibility {
    pub supports_developer_role: bool,
    pub supports_reasoning_effort: bool,
    pub chat_max_tokens_field: ChatMaxTokensField,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelProfile {
    pub schema_version: u8,
    pub id: String,
    pub display_name: String,
    pub source: ProfileSource,
    pub limits: ModelLimits,
    pub capabilities: ModelCapabilities,
    pub compatibility: ModelCompatibility,
    pub qualification: QualificationMatrix,
    pub warnings: Vec<String>,
}
