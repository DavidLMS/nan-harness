use nan_harness_core::CodingModelProfile;
use nan_harness_core::launch_plan::{
    SELECTED_MODEL_CAPABILITIES_PLACEHOLDER, SELECTED_MODEL_CONTEXT_WINDOW_PLACEHOLDER,
    SELECTED_MODEL_DISPLAY_NAME_PLACEHOLDER, SELECTED_MODEL_MAX_OUTPUT_TOKENS_PLACEHOLDER,
    SELECTED_MODEL_REASONING_EFFORT_PLACEHOLDER,
};
use nan_harness_core::model::{ReasoningEffort, ReasoningPolicy, ReasoningSelection};
use std::collections::BTreeSet;

pub(in crate::prepared) fn unique_models(models: &[CodingModelProfile]) -> Vec<CodingModelProfile> {
    let mut seen = BTreeSet::new();
    models
        .iter()
        .filter(|model| seen.insert(model.id.clone()))
        .cloned()
        .collect()
}

pub(in crate::prepared) fn render_selected_model(
    target: &mut String,
    selected_model_id: &str,
    models: &[CodingModelProfile],
) -> Result<(), String> {
    let Some(model) = models.iter().find(|model| model.id == selected_model_id) else {
        return Err(format!(
            "selected model '{selected_model_id}' is not present in the discovered NaN catalog"
        ));
    };
    let capabilities = match (model.image_input, reasoning_capable(model.reasoning)) {
        (true, true) => "image_in,thinking",
        (true, false) => "image_in",
        (false, true) => "thinking",
        (false, false) => "",
    };
    *target = target
        .replace(SELECTED_MODEL_DISPLAY_NAME_PLACEHOLDER, &model.display_name)
        .replace(
            SELECTED_MODEL_CONTEXT_WINDOW_PLACEHOLDER,
            &model.context_window.to_string(),
        )
        .replace(
            SELECTED_MODEL_MAX_OUTPUT_TOKENS_PLACEHOLDER,
            &model.max_output_tokens.to_string(),
        )
        .replace(SELECTED_MODEL_CAPABILITIES_PLACEHOLDER, capabilities);
    Ok(())
}

pub(in crate::prepared) fn model_input(model: &CodingModelProfile) -> serde_json::Value {
    if model.image_input {
        serde_json::json!(["text", "image"])
    } else {
        serde_json::json!(["text"])
    }
}

pub(in crate::prepared) fn reasoning_capable(policy: ReasoningPolicy) -> bool {
    matches!(
        policy,
        ReasoningPolicy::Toggle { .. } | ReasoningPolicy::Effort { .. } | ReasoningPolicy::AlwaysOn
    )
}

pub(in crate::prepared) fn effort_name(effort: ReasoningEffort) -> &'static str {
    match effort {
        ReasoningEffort::Low => "low",
        ReasoningEffort::Medium => "medium",
        ReasoningEffort::High => "high",
    }
}

pub(in crate::prepared) fn selected_model_reasoning_effort(
    selected_model_id: &str,
    requested: Option<ReasoningSelection>,
    models: &[CodingModelProfile],
) -> Result<String, String> {
    let model = models
        .iter()
        .find(|model| model.id == selected_model_id)
        .ok_or_else(|| {
            format!(
                "selected model '{selected_model_id}' is not present in the discovered NaN catalog"
            )
        })?;
    let default = model.reasoning.default_selection();
    let selection = requested
        .filter(|selection| model.reasoning.accepts(*selection))
        .unwrap_or(default);
    Ok(match selection {
        ReasoningSelection::Auto | ReasoningSelection::Toggle(false) => "none".to_owned(),
        ReasoningSelection::Toggle(true) => "high".to_owned(),
        ReasoningSelection::Effort(effort) => effort_name(effort).to_owned(),
    })
}

pub(in crate::prepared) fn render_reasoning_effort(
    value: &str,
    effort: Option<&str>,
) -> Result<String, String> {
    if !value.contains(SELECTED_MODEL_REASONING_EFFORT_PLACEHOLDER) {
        return Ok(value.to_owned());
    }
    let effort = effort
        .ok_or_else(|| "selected model reasoning requires live NaN model discovery".to_owned())?;
    Ok(value.replace(SELECTED_MODEL_REASONING_EFFORT_PLACEHOLDER, effort))
}
