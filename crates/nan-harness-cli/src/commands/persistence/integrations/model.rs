use nan_harness_core::CodingModelProfile;

pub(super) fn preferred_persistent_model(models: &[CodingModelProfile]) -> &str {
    models
        .iter()
        .find(|model| model.id == "qwen3.6")
        .or_else(|| models.first())
        .map_or("qwen3.6", |model| model.id.as_str())
}
