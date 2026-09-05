use nan_harness_core::model::ReasoningPolicy;
use nan_harness_core::{CodingModelProfile, ProfileSource, coding_model_profile};

pub(super) fn model(id: &str) -> CodingModelProfile {
    CodingModelProfile {
        id: id.to_owned(),
        display_name: format!("NaN · {id}"),
        description: "test model".to_owned(),
        context_window: 262_144,
        max_output_tokens: 32_768,
        image_input: false,
        reasoning: ReasoningPolicy::Unknown,
        source: ProfileSource::Generic,
    }
}

pub(super) fn known_models() -> Vec<CodingModelProfile> {
    [
        "qwen3.6",
        "deepseek-v4-flash",
        "mimo-v2.5",
        "gemma4",
        "glm5.2",
    ]
    .into_iter()
    .map(|id| coding_model_profile(id).expect("known coding model"))
    .collect()
}
