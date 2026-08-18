use crate::HarnessKind;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const CLAUDE_GATEWAY_MODEL_PREFIX: &str = "anthropic/nan/";
pub const CLAUDE_AUTO_MODE_COMPATIBILITY_ALIAS: &str = "opus";
pub const CLAUDE_AUTO_MODE_PROVIDER_MODEL_ID: &str = "qwen3.6";
pub const GENERIC_CODING_MODEL_DESCRIPTION: &str = "NaN text model · capabilities not yet profiled";
pub const GENERIC_CODING_MODEL_CONTEXT_WINDOW: u64 = 262_144;
pub const GENERIC_CODING_MODEL_MAX_OUTPUT_TOKENS: u64 = 32_768;
pub const KNOWN_NON_CODING_MODELS: [&str; 5] = [
    "whisper",
    "qwen3-embedding",
    "rerank",
    "kokoro",
    "flux-2-klein",
];
pub const KNOWN_CODING_MODELS: [CodingModelMetadata; 5] = [
    CodingModelMetadata {
        id: "qwen3.6",
        display_name: "NaN · Qwen 3.6",
        description: "General reasoning · tools + vision · 256K",
        context_window: 262_144,
        max_output_tokens: 65_536,
        image_input: true,
    },
    CodingModelMetadata {
        id: "deepseek-v4-flash",
        display_name: "NaN · DeepSeek V4 Flash",
        description: "Advanced reasoning · tools · 1M context",
        context_window: 1_000_000,
        max_output_tokens: 262_144,
        image_input: false,
    },
    CodingModelMetadata {
        id: "mimo-v2.5",
        display_name: "NaN · MiMo V2.5",
        description: "Omnimodal reasoning · tools + vision · 1M",
        context_window: 1_000_000,
        max_output_tokens: 65_536,
        image_input: true,
    },
    CodingModelMetadata {
        id: "gemma4",
        display_name: "NaN · Gemma 4",
        description: "Opt-in reasoning · tools + vision · 256K",
        context_window: 262_144,
        max_output_tokens: 65_536,
        image_input: true,
    },
    CodingModelMetadata {
        id: "glm5.2",
        display_name: "NaN · GLM 5.2",
        description: "Agentic coding · tools + reasoning · 500K",
        context_window: 500_000,
        max_output_tokens: 65_536,
        image_input: false,
    },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodingModelMetadata {
    pub id: &'static str,
    pub display_name: &'static str,
    pub description: &'static str,
    pub context_window: u64,
    pub max_output_tokens: u64,
    pub image_input: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingModelProfile {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub context_window: u64,
    pub max_output_tokens: u64,
    pub image_input: bool,
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
            source: ProfileSource::Bundled,
        }
    }
}

#[must_use]
pub fn known_coding_model(model_id: &str) -> Option<&'static CodingModelMetadata> {
    KNOWN_CODING_MODELS
        .iter()
        .find(|model| model.id == model_id)
}

#[must_use]
pub fn coding_model_profile(model_id: &str) -> Option<CodingModelProfile> {
    if !is_valid_provider_model_id(model_id) || is_known_non_coding_model(model_id) {
        return None;
    }
    Some(known_coding_model(model_id).map_or_else(
        || CodingModelProfile::generic(model_id),
        CodingModelProfile::from,
    ))
}

#[must_use]
pub fn coding_models_from_provider_ids(
    provider_ids: impl IntoIterator<Item = String>,
) -> Vec<CodingModelProfile> {
    let available = provider_ids
        .into_iter()
        .filter(|model_id| is_valid_provider_model_id(model_id))
        .collect::<BTreeSet<_>>();
    let mut models = KNOWN_CODING_MODELS
        .iter()
        .filter(|metadata| available.contains(metadata.id))
        .map(CodingModelProfile::from)
        .collect::<Vec<_>>();
    models.extend(
        available
            .into_iter()
            .filter(|model_id| known_coding_model(model_id).is_none())
            .filter_map(|model_id| coding_model_profile(&model_id)),
    );
    models
}

#[must_use]
pub fn is_known_non_coding_model(model_id: &str) -> bool {
    KNOWN_NON_CODING_MODELS.contains(&model_id)
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
    format!("{CLAUDE_GATEWAY_MODEL_PREFIX}{provider_model_id}")
}

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
    #[serde(rename = "prime-agent")]
    pub prime_agent: ModelQualification,
    #[serde(rename = "deepseek-harness")]
    pub deepseek_harness: ModelQualification,
    #[serde(rename = "openclaw")]
    pub openclaw: ModelQualification,
    pub cline: ModelQualification,
    #[serde(rename = "qwen-code")]
    pub qwen_code: ModelQualification,
    pub aider: ModelQualification,
    pub goose: ModelQualification,
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
            HarnessKind::PrimeAgent => &self.prime_agent,
            HarnessKind::DeepSeekHarness => &self.deepseek_harness,
            HarnessKind::OpenClaw => &self.openclaw,
            HarnessKind::Cline => &self.cline,
            HarnessKind::QwenCode => &self.qwen_code,
            HarnessKind::Aider => &self.aider,
            HarnessKind::Goose => &self.goose,
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

#[cfg(test)]
mod tests {
    use super::{
        GENERIC_CODING_MODEL_DESCRIPTION, KNOWN_CODING_MODELS, ProfileSource, coding_model_profile,
        coding_models_from_provider_ids, known_coding_model,
    };
    use std::collections::BTreeSet;

    #[test]
    fn coding_model_metadata_is_complete_and_uniquely_addressable() {
        let ids = KNOWN_CODING_MODELS
            .iter()
            .map(|model| model.id)
            .collect::<BTreeSet<_>>();

        assert_eq!(ids.len(), KNOWN_CODING_MODELS.len());
        for model in KNOWN_CODING_MODELS {
            assert!(!model.display_name.trim().is_empty());
            assert!(!model.description.trim().is_empty());
            assert!(model.context_window > 0);
            assert!(model.max_output_tokens > 0);
            assert_eq!(known_coding_model(model.id), Some(&model));
        }
    }

    #[test]
    fn live_catalog_enriches_known_models_and_accepts_unknown_text_models() {
        let models = coding_models_from_provider_ids([
            "deepseek-v4-flash-0731".to_owned(),
            "qwen3.6".to_owned(),
            "glm5.2".to_owned(),
        ]);

        assert_eq!(
            models
                .iter()
                .map(|model| model.id.as_str())
                .collect::<Vec<_>>(),
            ["qwen3.6", "glm5.2", "deepseek-v4-flash-0731"]
        );
        let provisional = models
            .iter()
            .find(|model| model.id == "deepseek-v4-flash-0731")
            .expect("new model should remain selectable");
        assert_eq!(provisional.source, ProfileSource::Generic);
        assert_eq!(provisional.description, GENERIC_CODING_MODEL_DESCRIPTION);
    }

    #[test]
    fn live_catalog_excludes_only_known_non_coding_models() {
        let models = coding_models_from_provider_ids([
            "whisper".to_owned(),
            "qwen3-embedding".to_owned(),
            "rerank".to_owned(),
            "kokoro".to_owned(),
            "flux-2-klein".to_owned(),
            "future-text-model".to_owned(),
        ]);

        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "future-text-model");
        assert!(coding_model_profile("whisper").is_none());
    }
}
