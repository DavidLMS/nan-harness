use crate::error::BridgeError;
use crate::models::discover_coding_models;
use nan_harness_core::model::{ReasoningEffort, ReasoningPolicy};
use nan_harness_core::{CodingModelProfile, SecretValue, coding_models_from_provider_ids};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct CodexModelCatalog {
    models: Vec<CodingModelProfile>,
    by_slug: BTreeMap<String, usize>,
}

impl CodexModelCatalog {
    /// Discovers the coding models available to one NaN credential.
    ///
    /// # Errors
    ///
    /// Returns [`BridgeError`] when discovery fails, no compatible model is
    /// available, or the selected model is unavailable.
    pub async fn discover(
        provider_base_url: &str,
        provider_api_key: Arc<SecretValue>,
        selected_model: &str,
    ) -> Result<Self, BridgeError> {
        let models = discover_coding_models(provider_base_url, provider_api_key).await?;
        Self::from_models(models, selected_model)
    }

    /// Builds a Codex catalog from the models available to one credential.
    ///
    /// # Errors
    ///
    /// Returns [`BridgeError`] when no compatible model is available or the
    /// selected model is not part of the resulting catalog.
    pub fn from_provider_ids(
        provider_ids: impl IntoIterator<Item = String>,
        selected_model: &str,
    ) -> Result<Self, BridgeError> {
        Self::from_models(
            coding_models_from_provider_ids(provider_ids),
            selected_model,
        )
    }

    /// Builds a Codex catalog from an already filtered catalog.
    ///
    /// # Errors
    ///
    /// Returns [`BridgeError`] when no compatible model is available or the
    /// selected model is not part of the catalog.
    pub fn from_models(
        models: Vec<CodingModelProfile>,
        selected_model: &str,
    ) -> Result<Self, BridgeError> {
        Self::from_models_with_aliases(models, selected_model, &[])
    }

    /// Adds request-only aliases without exposing them in the model picker.
    ///
    /// # Errors
    ///
    /// Returns [`BridgeError`] when a selected model or alias target is absent.
    pub fn from_models_with_aliases(
        models: Vec<CodingModelProfile>,
        selected_model: &str,
        aliases: &[(String, String)],
    ) -> Result<Self, BridgeError> {
        if models.is_empty() {
            return Err(BridgeError::NoCompatibleModels);
        }
        if !models.iter().any(|model| model.id == selected_model) {
            return Err(BridgeError::SelectedModelUnavailable {
                model: selected_model.to_owned(),
                available: models.iter().map(|model| model.id.clone()).collect(),
            });
        }
        let mut by_slug = models
            .iter()
            .enumerate()
            .map(|(index, model)| (model.id.clone(), index))
            .collect::<BTreeMap<_, _>>();
        for (alias, target) in aliases {
            let Some(index) = by_slug.get(target).copied() else {
                return Err(BridgeError::SelectedModelUnavailable {
                    model: target.clone(),
                    available: models.iter().map(|model| model.id.clone()).collect(),
                });
            };
            by_slug.entry(alias.clone()).or_insert(index);
        }
        Ok(Self { models, by_slug })
    }

    #[must_use]
    pub fn api_response(&self) -> Value {
        json!({
            "models": self.models.iter().map(api_model).collect::<Vec<_>>()
        })
    }

    pub(crate) fn resolve(&self, slug: &str) -> Option<&CodingModelProfile> {
        self.by_slug.get(slug).map(|index| &self.models[*index])
    }
}

fn api_model(model: &CodingModelProfile) -> Value {
    let input_modalities = if model.image_input {
        json!(["text", "image"])
    } else {
        json!(["text"])
    };
    let (default_reasoning_level, supported_reasoning_levels) = reasoning_levels(model.reasoning);
    json!({
        "slug": model.id,
        "display_name": model.display_name,
        "description": model.description,
        "default_reasoning_level": default_reasoning_level,
        "supported_reasoning_levels": supported_reasoning_levels,
        "shell_type": "shell_command",
        "visibility": "list",
        "supported_in_api": true,
        "priority": 0,
        "availability_nux": null,
        "upgrade": null,
        "base_instructions": concat!(
            "You are an agentic coding assistant working in the user's repository. ",
            "Use the available tools to inspect, change, and verify the project. ",
            "Communicate clearly, preserve user work, and finish the requested task."
        ),
        "include_skills_usage_instructions": true,
        "supports_reasoning_summary_parameter": false,
        "default_reasoning_summary": "none",
        "support_verbosity": false,
        "default_verbosity": null,
        "apply_patch_tool_type": "freeform",
        "web_search_tool_type": "text",
        "truncation_policy": {"mode": "tokens", "limit": 10_000},
        "supports_parallel_tool_calls": true,
        "context_window": model.context_window,
        "max_context_window": model.context_window,
        "auto_compact_token_limit": null,
        "effective_context_window_percent": 90,
        "experimental_supported_tools": [],
        "input_modalities": input_modalities,
        "supports_search_tool": false,
        "use_responses_lite": false,
        "tool_mode": "direct",
        "multi_agent_version": "v1"
    })
}

fn reasoning_levels(policy: ReasoningPolicy) -> (&'static str, Value) {
    let level = |effort: &str, description: &str| {
        json!({
            "effort": effort,
            "description": description
        })
    };
    match policy {
        ReasoningPolicy::Toggle { default_enabled } => (
            if default_enabled { "high" } else { "none" },
            json!([
                level("none", "Disable reasoning"),
                level("high", "Enable reasoning")
            ]),
        ),
        ReasoningPolicy::Effort { default, supported } => {
            let effort_name = |effort| match effort {
                ReasoningEffort::Low => "low",
                ReasoningEffort::Medium => "medium",
                ReasoningEffort::High => "high",
            };
            (
                effort_name(default),
                Value::Array(
                    supported
                        .into_iter()
                        .map(|effort| {
                            let name = effort_name(effort);
                            level(name, &format!("{name} reasoning effort"))
                        })
                        .collect(),
                ),
            )
        }
        ReasoningPolicy::AlwaysOn => (
            "high",
            json!([level("high", "Model reasoning is always enabled")]),
        ),
        ReasoningPolicy::Unsupported | ReasoningPolicy::Unknown => {
            ("none", json!([level("none", "No reasoning control")]))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::CodexModelCatalog;
    use serde_json::json;

    #[test]
    fn catalog_keeps_entitled_coding_models_and_excludes_other_modalities() {
        let catalog = CodexModelCatalog::from_provider_ids(
            [
                "qwen3.6".to_owned(),
                "mimo-v2.5".to_owned(),
                "qwen3-embedding".to_owned(),
                "whisper".to_owned(),
                "minimax-h3".to_owned(),
            ],
            "qwen3.6",
        )
        .expect("catalog should build");

        let response = catalog.api_response();
        let slugs = response["models"]
            .as_array()
            .expect("models should be an array")
            .iter()
            .map(|model| model["slug"].as_str().expect("slug should be text"))
            .collect::<Vec<_>>();
        assert_eq!(slugs, ["qwen3.6", "mimo-v2.5"]);
    }

    #[test]
    fn catalog_exposes_new_models_with_an_honest_generic_description() {
        let catalog = CodexModelCatalog::from_provider_ids(
            ["qwen3.6".to_owned(), "deepseek-v4-flash-0731".to_owned()],
            "deepseek-v4-flash-0731",
        )
        .expect("new text models should be accepted provisionally");

        let response = catalog.api_response();
        assert_eq!(response["models"][1]["slug"], "deepseek-v4-flash-0731");
        assert_eq!(
            response["models"][1]["description"],
            "NaN text model · capabilities not yet profiled"
        );
        assert_eq!(response["models"][1]["default_reasoning_level"], "none");
        assert_eq!(
            response["models"][1]["supported_reasoning_levels"][0]["effort"],
            "none"
        );
    }

    #[test]
    fn catalog_exposes_each_bundled_models_reasoning_contract() {
        let catalog = CodexModelCatalog::from_provider_ids(
            [
                "qwen3.6",
                "deepseek-v4-flash",
                "mimo-v2.5",
                "gemma4",
                "glm5.2",
            ]
            .into_iter()
            .map(str::to_owned),
            "qwen3.6",
        )
        .expect("catalog should build");
        let models = catalog.api_response();
        let models = models["models"].as_array().expect("models array");
        let contract = |index: usize| {
            (
                models[index]["default_reasoning_level"].clone(),
                models[index]["supported_reasoning_levels"]
                    .as_array()
                    .expect("levels")
                    .iter()
                    .map(|level| level["effort"].clone())
                    .collect::<Vec<_>>(),
            )
        };
        assert_eq!(
            contract(0),
            (json!("high"), vec![json!("none"), json!("high")])
        );
        assert_eq!(
            contract(1),
            (
                json!("medium"),
                vec![json!("low"), json!("medium"), json!("high")]
            )
        );
        assert_eq!(contract(2), (json!("high"), vec![json!("high")]));
        assert_eq!(
            contract(3),
            (json!("none"), vec![json!("none"), json!("high")])
        );
        assert_eq!(
            contract(4),
            (
                json!("medium"),
                vec![json!("low"), json!("medium"), json!("high")]
            )
        );
    }

    #[test]
    fn catalog_exposes_new_nan_models_with_vision_and_context_metadata() {
        let catalog = CodexModelCatalog::from_provider_ids(
            ["qwen3.8-flash", "glm5.3-flash"]
                .into_iter()
                .map(str::to_owned),
            "qwen3.8-flash",
        )
        .expect("catalog should build");
        let response = catalog.api_response();
        let models = response["models"].as_array().expect("models array");

        assert_eq!(models[0]["slug"], "qwen3.8-flash");
        assert_eq!(models[0]["context_window"], 1_000_000);
        assert_eq!(models[0]["input_modalities"], json!(["text", "image"]));
        assert_eq!(models[0]["default_reasoning_level"], "high");
        assert_eq!(
            models[0]["supported_reasoning_levels"]
                .as_array()
                .expect("reasoning levels"),
            &[json!({
                "effort": "high",
                "description": "Model reasoning is always enabled"
            })]
        );

        assert_eq!(models[1]["slug"], "glm5.3-flash");
        assert_eq!(models[1]["context_window"], 1_000_000);
        assert_eq!(models[1]["input_modalities"], json!(["text", "image"]));
        assert_eq!(models[1]["default_reasoning_level"], "medium");
        assert_eq!(
            models[1]["supported_reasoning_levels"],
            json!([
                {"effort": "low", "description": "low reasoning effort"},
                {"effort": "medium", "description": "medium reasoning effort"},
                {"effort": "high", "description": "high reasoning effort"}
            ])
        );
    }

    #[test]
    fn catalog_rejects_an_unavailable_selected_model() {
        let error = CodexModelCatalog::from_provider_ids(
            ["qwen3.6".to_owned(), "mimo-v2.5".to_owned()],
            "gemma4",
        )
        .expect_err("unavailable model should be rejected");

        assert_eq!(error.code(), "NH-BRIDGE-005");
    }
}
