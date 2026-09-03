use super::discovery::discover_coding_models;
use crate::error::BridgeError;
use nan_harness_core::model::ReasoningPolicy;
use nan_harness_core::{
    CLAUDE_AUTO_MODE_COMPATIBILITY_ALIAS, CLAUDE_AUTO_MODE_PROVIDER_MODEL_ID, CodingModelProfile,
    SecretValue, claude_gateway_model_id, coding_models_from_provider_ids,
};
use serde::Serialize;
use sha2::{Digest as _, Sha256};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::Arc;

const UNKNOWN_RELEASE_DATE: &str = "1970-01-01T00:00:00Z";
const CLAUDE_DESKTOP_MODEL_PREFIX: &str = "claude-nan-";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeModel {
    provider_id: String,
    gateway_id: String,
    display_name: String,
    max_input_tokens: u64,
    max_output_tokens: u64,
    reasoning: ReasoningPolicy,
}

impl ClaudeModel {
    #[must_use]
    pub fn provider_id(&self) -> &str {
        &self.provider_id
    }

    #[must_use]
    pub fn gateway_id(&self) -> &str {
        &self.gateway_id
    }

    #[must_use]
    pub const fn max_output_tokens(&self) -> u64 {
        self.max_output_tokens
    }

    #[must_use]
    pub const fn reasoning(&self) -> ReasoningPolicy {
        self.reasoning
    }

    fn api_model(&self) -> AnthropicModel {
        AnthropicModel {
            id: self.gateway_id.clone(),
            model_type: "model",
            created_at: UNKNOWN_RELEASE_DATE,
            display_name: self.display_name.clone(),
            max_input_tokens: self.max_input_tokens,
            max_tokens: self.max_output_tokens,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ClaudeModelCatalog {
    models: Vec<ClaudeModel>,
    by_gateway_id: BTreeMap<String, usize>,
    default_index: usize,
}

impl ClaudeModelCatalog {
    /// Discovers the models available to one NaN credential and keeps only the
    /// models supported by the Claude Code bridge.
    ///
    /// # Errors
    ///
    /// Returns [`BridgeError`] when discovery fails, the response is invalid,
    /// no compatible model is available, or the selected default is unavailable.
    pub async fn discover(
        provider_base_url: &str,
        provider_api_key: Arc<SecretValue>,
        default_provider_id: &str,
    ) -> Result<Self, BridgeError> {
        let models = discover_coding_models(provider_base_url, provider_api_key).await?;
        Self::from_models(models, default_provider_id)
    }

    /// Builds the Claude-compatible catalog from provider model IDs.
    ///
    /// # Errors
    ///
    /// Returns [`BridgeError`] when no supported model is present or the
    /// selected default model is not part of the resulting catalog.
    pub fn from_provider_ids(
        provider_ids: impl IntoIterator<Item = String>,
        default_provider_id: &str,
    ) -> Result<Self, BridgeError> {
        Self::from_models(
            coding_models_from_provider_ids(provider_ids),
            default_provider_id,
        )
    }

    /// Builds the Claude-compatible catalog from an already filtered catalog.
    ///
    /// # Errors
    ///
    /// Returns [`BridgeError`] when no compatible model is present or the
    /// selected default model is not part of the catalog.
    pub fn from_models(
        profiles: impl IntoIterator<Item = CodingModelProfile>,
        default_provider_id: &str,
    ) -> Result<Self, BridgeError> {
        let models = profiles
            .into_iter()
            .map(|profile| ClaudeModel {
                gateway_id: claude_gateway_model_id(&profile.id),
                provider_id: profile.id,
                display_name: profile.display_name,
                max_input_tokens: profile.context_window,
                max_output_tokens: profile.max_output_tokens,
                reasoning: profile.reasoning,
            })
            .collect::<Vec<_>>();
        if models.is_empty() {
            return Err(BridgeError::NoCompatibleModels);
        }
        let Some(default_index) = models
            .iter()
            .position(|model| model.provider_id == default_provider_id)
        else {
            return Err(BridgeError::SelectedModelUnavailable {
                model: default_provider_id.to_owned(),
                available: models
                    .iter()
                    .map(|model| model.provider_id.clone())
                    .collect(),
            });
        };
        let mut by_gateway_id = models
            .iter()
            .enumerate()
            .map(|(index, model)| (model.gateway_id.clone(), index))
            .collect::<BTreeMap<_, _>>();
        if default_provider_id == CLAUDE_AUTO_MODE_PROVIDER_MODEL_ID {
            by_gateway_id.insert(
                CLAUDE_AUTO_MODE_COMPATIBILITY_ALIAS.to_owned(),
                default_index,
            );
        }
        Ok(Self {
            models,
            by_gateway_id,
            default_index,
        })
    }

    /// Builds a dynamic Claude Desktop catalog with stable opaque gateway IDs.
    /// Provider IDs remain internal so the UI does not assign misleading
    /// Claude family descriptions to non-Anthropic models.
    ///
    /// # Errors
    ///
    /// Returns [`BridgeError`] when the catalog is empty or an explicit model
    /// is not available to the current credential.
    pub fn for_desktop(
        profiles: impl IntoIterator<Item = CodingModelProfile>,
        selected_provider_id: Option<&str>,
    ) -> Result<Self, BridgeError> {
        let mut profiles = profiles.into_iter().collect::<Vec<_>>();
        if profiles.is_empty() {
            return Err(BridgeError::NoCompatibleModels);
        }
        profiles.sort_by(|left, right| {
            desktop_model_priority(&left.id)
                .cmp(&desktop_model_priority(&right.id))
                .then_with(|| left.id.cmp(&right.id))
        });
        if let Some(selected) = selected_provider_id {
            let Some(index) = profiles.iter().position(|profile| profile.id == selected) else {
                return Err(BridgeError::SelectedModelUnavailable {
                    model: selected.to_owned(),
                    available: profiles.iter().map(|profile| profile.id.clone()).collect(),
                });
            };
            profiles.swap(0, index);
        }
        let models = profiles
            .into_iter()
            .map(|profile| ClaudeModel {
                gateway_id: claude_desktop_gateway_model_id(&profile.id),
                provider_id: profile.id,
                display_name: profile
                    .display_name
                    .strip_prefix("NaN · ")
                    .unwrap_or(&profile.display_name)
                    .to_owned(),
                max_input_tokens: profile.context_window,
                max_output_tokens: profile.max_output_tokens,
                reasoning: profile.reasoning,
            })
            .collect::<Vec<_>>();
        let by_gateway_id = models
            .iter()
            .enumerate()
            .map(|(index, model)| (model.gateway_id.clone(), index))
            .collect();
        Ok(Self {
            models,
            by_gateway_id,
            default_index: 0,
        })
    }

    #[must_use]
    pub fn default_model(&self) -> &ClaudeModel {
        &self.models[self.default_index]
    }

    #[must_use]
    pub fn resolve(&self, gateway_id: &str) -> Option<&ClaudeModel> {
        self.by_gateway_id
            .get(gateway_id)
            .map(|index| &self.models[*index])
            .or_else(|| is_claude_default_model(gateway_id).then(|| self.default_model()))
    }

    #[must_use]
    pub fn gateway_ids(&self) -> Vec<String> {
        self.models
            .iter()
            .map(|model| model.gateway_id.clone())
            .collect()
    }

    pub(crate) fn api_response(&self) -> AnthropicModelsResponse {
        let data = self.models.iter().map(ClaudeModel::api_model).collect();
        AnthropicModelsResponse {
            first_id: self.models.first().map(|model| model.gateway_id.clone()),
            last_id: self.models.last().map(|model| model.gateway_id.clone()),
            data,
            has_more: false,
        }
    }
}

fn claude_desktop_gateway_model_id(provider_model_id: &str) -> String {
    let digest = Sha256::digest(provider_model_id.as_bytes());
    let mut digest_hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(digest_hex, "{byte:02x}");
    }
    format!("{CLAUDE_DESKTOP_MODEL_PREFIX}{digest_hex}")
}

fn desktop_model_priority(model_id: &str) -> usize {
    match model_id {
        "qwen3.6" => 0,
        "qwen3.8-flash" => 1,
        "deepseek-v4-flash" => 2,
        "mimo-v2.5" => 3,
        "gemma4" => 4,
        "glm5.2" => 5,
        "glm5.3-flash" => 6,
        "glm5.3" => 7,
        _ => usize::MAX,
    }
}

fn is_claude_default_model(model: &str) -> bool {
    matches!(model, "default" | "sonnet" | "opus" | "haiku" | "opusplan")
        || model.starts_with("claude-")
}

#[derive(Debug, Serialize)]
pub(crate) struct AnthropicModelsResponse {
    data: Vec<AnthropicModel>,
    first_id: Option<String>,
    has_more: bool,
    last_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct AnthropicModel {
    id: String,
    #[serde(rename = "type")]
    model_type: &'static str,
    created_at: &'static str,
    display_name: String,
    max_input_tokens: u64,
    max_tokens: u64,
}

#[cfg(test)]
mod tests {
    use super::{ClaudeModel, ClaudeModelCatalog};

    #[test]
    fn catalog_keeps_only_qualified_claude_code_models() {
        let catalog = ClaudeModelCatalog::from_provider_ids(
            [
                "qwen3.6".to_owned(),
                "gemma4".to_owned(),
                "qwen3-embedding".to_owned(),
                "deepseek-v4-flash".to_owned(),
            ],
            "qwen3.6",
        )
        .expect("catalog should build");

        assert_eq!(
            catalog.gateway_ids(),
            [
                "anthropic/nan/qwen3.6".to_owned(),
                "anthropic/nan/deepseek-v4-flash".to_owned(),
                "anthropic/nan/gemma4".to_owned(),
            ]
        );
    }

    #[test]
    fn catalog_keeps_new_provider_models_with_generic_metadata() {
        let catalog = ClaudeModelCatalog::from_provider_ids(
            [
                "qwen3.6".to_owned(),
                "deepseek-v4-flash-0731".to_owned(),
                "whisper".to_owned(),
            ],
            "deepseek-v4-flash-0731",
        )
        .expect("new text models should be accepted provisionally");

        assert_eq!(
            catalog.gateway_ids(),
            [
                "anthropic/nan/qwen3.6".to_owned(),
                "anthropic/nan/deepseek-v4-flash-0731".to_owned(),
            ]
        );
    }

    #[test]
    fn catalog_enriches_new_nan_models_for_claude_gateway_discovery() {
        let catalog = ClaudeModelCatalog::from_provider_ids(
            ["qwen3.8-flash", "glm5.3-flash", "glm5.3"]
                .into_iter()
                .map(str::to_owned),
            "qwen3.8-flash",
        )
        .expect("catalog should build");

        let response = catalog.api_response();
        assert_eq!(response.data[0].id, "anthropic/nan/qwen3.8-flash");
        assert_eq!(response.data[0].display_name, "NaN · Qwen 3.8 Flash");
        assert_eq!(response.data[0].max_input_tokens, 1_000_000);
        assert_eq!(response.data[1].id, "anthropic/nan/glm5.3-flash");
        assert_eq!(response.data[1].display_name, "NaN · GLM 5.3 Flash");
        assert_eq!(response.data[1].max_input_tokens, 1_000_000);
        assert_eq!(response.data[2].id, "anthropic/nan/glm5.3");
        assert_eq!(response.data[2].display_name, "NaN · GLM 5.3");
        assert_eq!(response.data[2].max_input_tokens, 1_000_000);
    }

    #[test]
    fn catalog_rejects_an_unavailable_default() {
        let error = ClaudeModelCatalog::from_provider_ids(["qwen3.6".to_owned()], "mimo-v2.5")
            .expect_err("default should be rejected");

        assert_eq!(error.code(), "NH-BRIDGE-005");
    }

    #[test]
    fn catalog_routes_claude_aliases_to_the_selected_default() {
        let catalog = ClaudeModelCatalog::from_provider_ids(
            ["qwen3.6".to_owned(), "mimo-v2.5".to_owned()],
            "mimo-v2.5",
        )
        .expect("catalog should build");

        assert_eq!(
            catalog.resolve("default").map(ClaudeModel::provider_id),
            Some("mimo-v2.5")
        );
        assert_eq!(
            catalog
                .resolve("claude-sonnet-4-6")
                .map(ClaudeModel::provider_id),
            Some("mimo-v2.5")
        );
        assert_eq!(
            catalog
                .resolve("claude-opus-4-6")
                .map(ClaudeModel::provider_id),
            Some("mimo-v2.5")
        );
        assert_eq!(
            catalog.resolve("haiku").map(ClaudeModel::provider_id),
            Some("mimo-v2.5")
        );
        assert_eq!(
            catalog.resolve("opusplan").map(ClaudeModel::provider_id),
            Some("mimo-v2.5")
        );
        assert!(catalog.resolve("anthropic/untrusted-model").is_none());
    }

    #[test]
    fn catalog_routes_the_auto_mode_alias_to_qwen_when_selected() {
        let catalog = ClaudeModelCatalog::from_provider_ids(
            ["qwen3.6".to_owned(), "mimo-v2.5".to_owned()],
            "qwen3.6",
        )
        .expect("catalog should build");

        assert_eq!(
            catalog.resolve("opus").map(ClaudeModel::provider_id),
            Some("qwen3.6")
        );
    }

    #[test]
    fn catalog_preserves_vision_profiles_for_deepseek_and_glm() {
        let catalog = ClaudeModelCatalog::from_provider_ids(
            ["deepseek-v4-flash", "glm5.3"]
                .into_iter()
                .map(str::to_owned),
            "deepseek-v4-flash",
        )
        .expect("catalog should build");

        let response = catalog.api_response();
        assert_eq!(response.data[0].max_input_tokens, 1_000_000);
        assert_eq!(response.data[0].max_tokens, 262_144);
        assert_eq!(response.data[1].max_input_tokens, 1_000_000);
        assert_eq!(response.data[1].max_tokens, 32_768);
    }
}
