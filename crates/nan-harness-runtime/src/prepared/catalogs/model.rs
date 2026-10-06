use nan_harness_core::CodingModelProfile;
use nan_harness_core::launch_plan::{
    SELECTED_MODEL_CAPABILITIES_PLACEHOLDER, SELECTED_MODEL_CONTEXT_WINDOW_PLACEHOLDER,
    SELECTED_MODEL_DISPLAY_NAME_PLACEHOLDER, SELECTED_MODEL_MAX_OUTPUT_TOKENS_PLACEHOLDER,
    SELECTED_MODEL_REASONING_EFFORT_PLACEHOLDER,
};
use nan_harness_core::model::{ReasoningEffort, ReasoningPolicy, ReasoningSelection};
use nan_harness_i18n::DiagnosticText;
use nan_harness_i18n::messages as detail_messages;
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
) -> Result<(), DiagnosticText> {
    let Some(model) = models.iter().find(|model| model.id == selected_model_id) else {
        return Err(DiagnosticText::new(|locale| {
            detail_messages::detail_selected_model_selected_model_id_is_not_present_in_the_discovered_nan_catalog(locale, &(selected_model_id))
        }));
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
        ReasoningEffort::Max => "max",
    }
}

pub(in crate::prepared) fn selected_model_reasoning_effort(
    selected_model_id: &str,
    requested: Option<ReasoningSelection>,
    models: &[CodingModelProfile],
) -> Result<Option<String>, DiagnosticText> {
    let model = models
        .iter()
        .find(|model| model.id == selected_model_id)
        .ok_or_else(|| {
            DiagnosticText::new(|locale| detail_messages::detail_selected_model_selected_model_id_is_not_present_in_the_discovered_nan_catalog(locale, &(selected_model_id)))
        })?;
    let selection = model_reasoning_selection(requested, model.reasoning);
    Ok(match selection {
        ReasoningSelection::Auto => None,
        ReasoningSelection::Toggle(false) => Some("none".to_owned()),
        ReasoningSelection::Toggle(true) => Some("high".to_owned()),
        ReasoningSelection::Effort(ReasoningEffort::Max) => Some("xhigh".to_owned()),
        ReasoningSelection::Effort(effort) => Some(effort_name(effort).to_owned()),
    })
}

pub(in crate::prepared) fn model_reasoning_selection(
    requested: Option<ReasoningSelection>,
    policy: ReasoningPolicy,
) -> ReasoningSelection {
    // Saved binary-on preferences remain enabled when a model gains effort levels.
    let requested = match (requested, policy) {
        (Some(ReasoningSelection::Toggle(true)), ReasoningPolicy::Effort { default, .. }) => {
            Some(ReasoningSelection::Effort(default))
        }
        (selection, _) => selection,
    };
    requested
        .filter(|selection| policy.accepts(*selection))
        .unwrap_or(ReasoningSelection::Auto)
}

pub(in crate::prepared) fn render_reasoning_effort(
    value: &str,
    effort: Option<&str>,
) -> Result<String, DiagnosticText> {
    if !value.contains(SELECTED_MODEL_REASONING_EFFORT_PLACEHOLDER) {
        return Ok(value.to_owned());
    }
    if let Some(effort) = effort {
        return Ok(value.replace(SELECTED_MODEL_REASONING_EFFORT_PLACEHOLDER, effort));
    }
    // A saved preference may stop applying after a model capability refresh.
    // Remove the owned Codex setting rather than turn omission into "none".
    let mut rendered = String::new();
    for line in value.split_inclusive('\n') {
        if line.contains(SELECTED_MODEL_REASONING_EFFORT_PLACEHOLDER) {
            if !line.trim_start().starts_with("model_reasoning_effort") {
                return Err(DiagnosticText::new(
                    detail_messages::detail_selected_model_reasoning_requires_live_nan_model_discovery,
                ));
            }
        } else {
            rendered.push_str(line);
        }
    }
    Ok(rendered)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nan_harness_core::coding_model_profile;

    #[test]
    fn codex_preferences_preserve_omission_off_and_native_maximum() {
        let models = [coding_model_profile("gemma4").unwrap()];
        for (selection, expected) in [
            (None, None),
            (Some(ReasoningSelection::Auto), None),
            (Some(ReasoningSelection::Toggle(false)), Some("none")),
            (
                Some(ReasoningSelection::Effort(ReasoningEffort::Low)),
                Some("low"),
            ),
            (
                Some(ReasoningSelection::Effort(ReasoningEffort::Max)),
                Some("xhigh"),
            ),
        ] {
            assert_eq!(
                selected_model_reasoning_effort("gemma4", selection, &models)
                    .unwrap()
                    .as_deref(),
                expected
            );
        }
    }
}
