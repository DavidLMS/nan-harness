use crate::error::BridgeError;
use nan_harness_core::coding_models_from_provider_ids;
use nan_harness_core::model::{CodingModelProfile, ReasoningPolicy};
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct FxModelCatalog {
    models: Vec<CodingModelProfile>,
    by_id: BTreeMap<String, usize>,
}

impl FxModelCatalog {
    /// Builds a catalog from the dynamically discovered NaN profiles.
    ///
    /// # Errors
    ///
    /// Returns [`BridgeError::NoCompatibleModels`] when the discovery result is empty.
    pub fn from_models(models: Vec<CodingModelProfile>) -> Result<Self, BridgeError> {
        if models.is_empty() {
            return Err(BridgeError::NoCompatibleModels);
        }
        let by_id = models
            .iter()
            .enumerate()
            .map(|(index, model)| (model.id.clone(), index))
            .collect();
        Ok(Self { models, by_id })
    }

    /// Builds a catalog from provider model IDs after applying NaN's coding-model policy.
    ///
    /// # Errors
    ///
    /// Returns [`BridgeError::NoCompatibleModels`] when no coding model is available.
    pub fn from_provider_ids(
        provider_ids: impl IntoIterator<Item = String>,
    ) -> Result<Self, BridgeError> {
        Self::from_models(coding_models_from_provider_ids(provider_ids))
    }

    #[must_use]
    pub fn resolve(&self, id: &str) -> Option<&CodingModelProfile> {
        self.by_id.get(id).map(|index| &self.models[*index])
    }

    pub fn api_response(&self) -> Value {
        json!({
            "object": "list",
            "data": self.models.iter().map(api_model).collect::<Vec<_>>()
        })
    }
}

fn api_model(model: &CodingModelProfile) -> Value {
    let mut tags = vec!["tool-use"];
    if model.image_input {
        tags.push("vision");
    }
    let reasoning_options = match model.reasoning {
        ReasoningPolicy::Toggle { .. } => {
            tags.push("reasoning");
            json!([{"type":"effort","values":["none","high"]}])
        }
        ReasoningPolicy::Effort { .. } => {
            tags.push("reasoning");
            json!([{"type":"effort","values":["low","medium","high"]}])
        }
        ReasoningPolicy::AlwaysOn => {
            tags.push("reasoning");
            json!([{"type":"effort","values":["high"]}])
        }
        ReasoningPolicy::Unsupported | ReasoningPolicy::Unknown => Value::Null,
    };
    json!({
        "id": model.id,
        "type": "language",
        "released": 0,
        "tags": tags,
        "reasoning_options": reasoning_options,
        "context_window": model.context_window,
        "max_tokens": model.max_output_tokens
    })
}

#[cfg(test)]
mod tests {
    use super::FxModelCatalog;

    #[test]
    fn catalog_uses_fx_gateway_shape() {
        let catalog = FxModelCatalog::from_provider_ids(["qwen3.6".to_owned()])
            .expect("catalog should build");
        let model = &catalog.api_response()["data"][0];
        assert_eq!(model["id"], "qwen3.6");
        assert_eq!(model["type"], "language");
        assert_eq!(model["reasoning_options"][0]["values"][0], "none");
    }
}
