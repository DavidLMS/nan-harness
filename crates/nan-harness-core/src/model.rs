use crate::HarnessKind;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProfileSource {
    UserOverride,
    Bundled,
    Generic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ModelAvailability {
    Discovered,
    ExplicitUndiscovered,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum QualificationStatus {
    Qualified,
    Unqualified,
    Unknown,
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum QualificationTransport {
    DirectChat,
    AnthropicBridge,
    ResponsesBridge,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelQualification {
    pub status: QualificationStatus,
    pub transport: QualificationTransport,
    pub tested_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QualificationMatrix {
    #[serde(rename = "claude-code")]
    pub claude_code: ModelQualification,
    pub codex: ModelQualification,
    pub opencode: ModelQualification,
    pub hermes: ModelQualification,
    pub pi: ModelQualification,
}

impl QualificationMatrix {
    #[must_use]
    pub const fn for_harness(&self, harness: HarnessKind) -> &ModelQualification {
        match harness {
            HarnessKind::ClaudeCode => &self.claude_code,
            HarnessKind::Codex => &self.codex,
            HarnessKind::OpenCode => &self.opencode,
            HarnessKind::Hermes => &self.hermes,
            HarnessKind::Pi => &self.pi,
        }
    }
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedModel {
    pub requested_id: String,
    pub resolved_id: String,
    pub availability: ModelAvailability,
    pub profile_source: ProfileSource,
    pub qualification: QualificationStatus,
    pub warnings: Vec<String>,
}

#[derive(Debug, Default)]
pub struct ModelCatalog {
    profiles: BTreeMap<String, ModelProfile>,
}

impl ModelCatalog {
    #[must_use]
    pub fn new(profiles: impl IntoIterator<Item = ModelProfile>) -> Self {
        Self {
            profiles: profiles
                .into_iter()
                .map(|profile| (profile.id.clone(), profile))
                .collect(),
        }
    }

    #[must_use]
    pub fn resolve_explicit(
        &self,
        requested_id: &str,
        harness: HarnessKind,
        discovered_ids: &BTreeSet<String>,
    ) -> ResolvedModel {
        let availability = if discovered_ids.contains(requested_id) {
            ModelAvailability::Discovered
        } else {
            ModelAvailability::ExplicitUndiscovered
        };

        let Some(profile) = self.profiles.get(requested_id) else {
            let mut warnings = vec![
                "This model has no bundled capability profile and will use conservative defaults."
                    .to_owned(),
            ];
            push_unique(&mut warnings, availability_warning(availability));
            return ResolvedModel {
                requested_id: requested_id.to_owned(),
                resolved_id: requested_id.to_owned(),
                availability,
                profile_source: ProfileSource::Generic,
                qualification: QualificationStatus::Unknown,
                warnings,
            };
        };

        let qualification = profile.qualification.for_harness(harness).status;
        let mut warnings = profile.warnings.clone();
        if availability == ModelAvailability::ExplicitUndiscovered {
            push_unique(&mut warnings, availability_warning(availability));
        }
        if qualification != QualificationStatus::Qualified {
            push_unique(
                &mut warnings,
                format!("Model '{requested_id}' is not qualified for {harness}."),
            );
        }

        ResolvedModel {
            requested_id: requested_id.to_owned(),
            resolved_id: profile.id.clone(),
            availability,
            profile_source: profile.source,
            qualification,
            warnings,
        }
    }
}

fn availability_warning(availability: ModelAvailability) -> String {
    match availability {
        ModelAvailability::Discovered => String::new(),
        ModelAvailability::ExplicitUndiscovered => {
            "The requested model was not returned by live discovery for this credential.".to_owned()
        }
    }
}

fn push_unique(values: &mut Vec<String>, value: String) {
    if !value.is_empty() && !values.contains(&value) {
        values.push(value);
    }
}
