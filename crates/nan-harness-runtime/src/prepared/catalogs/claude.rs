use nan_harness_core::launch_plan::{
    CLAUDE_MODEL_PICKER_PLACEHOLDER, CLAUDE_MODEL_PRESENTATIONS_PLACEHOLDER,
};
use nan_harness_core::{CodingModelProfile, claude_gateway_model_id};

const CLAUDE_STANDARD_CONTEXT_DESCRIPTION: &str = "Standard context · 256K";
const CLAUDE_EXTENDED_CONTEXT_DESCRIPTION: &str = "Extended context · 1M";
const CLAUDE_EXTENDED_CONTEXT_MIN_TOKENS: u64 = 1_000_000;

/// Builds the credential-scoped Claude Code picker introduced in 2.1.243.
///
/// The initial launch model never carries the `[1m]` suffix, so standard context remains an
/// explicit default. Claude Code strips the suffix before routing custom gateway model IDs.
pub(in crate::prepared) fn claude_model_picker(models: &[CodingModelProfile]) -> serde_json::Value {
    let mut options = Vec::new();
    for model in models {
        let standard_model = if model.id == nan_harness_core::CLAUDE_AUTO_MODE_PROVIDER_MODEL_ID {
            nan_harness_core::CLAUDE_AUTO_MODE_COMPATIBILITY_ALIAS.to_owned()
        } else {
            claude_gateway_model_id(&model.id)
        };
        options.push(serde_json::json!({
            "model": standard_model,
            "label": model.display_name,
            "description": CLAUDE_STANDARD_CONTEXT_DESCRIPTION,
        }));
        if model.context_window >= CLAUDE_EXTENDED_CONTEXT_MIN_TOKENS {
            options.push(serde_json::json!({
                "model": format!("{}[1m]", claude_gateway_model_id(&model.id)),
                "label": format!("{} (1M)", model.display_name),
                "description": CLAUDE_EXTENDED_CONTEXT_DESCRIPTION,
            }));
        }
    }
    serde_json::json!({
        "options": options,
        "replaceBuiltInOptions": true,
    })
}

/// Claude Code exposes one model per built-in family plus a single custom slot.
const CLAUDE_MODEL_FAMILIES: [&str; 3] = ["OPUS", "SONNET", "HAIKU"];

/// Models whose Claude picker presentation has been curated and verified together.
///
/// Keep this separate from the shared bundled-model catalog: adding metadata for a new
/// provider model must not silently move Claude Code back from gateway discovery to its
/// four presentation slots. The order is the curated picker priority after the selected
/// model, with GLM intentionally preferred over Gemma when both are available.
const CLAUDE_CURATED_MODEL_PRIORITY: [&str; 5] = [
    "qwen3.6",
    "deepseek-v4-flash",
    "mimo-v2.5",
    "glm5.2",
    "gemma4",
];

/// Chooses the Claude Code model-picker presentation for the live NaN catalog.
///
/// A catalog containing only the verified curated IDs uses Claude's three family slots plus its
/// custom slot. A catalog containing any other ID leaves the complete picker to gateway discovery
/// and pins only Qwen's Opus compatibility alias for native Auto mode. In both cases the placeholder
/// is removed from the rendered settings artifact.
pub(in crate::prepared) fn render_claude_model_presentations(
    template: &str,
    selected_model_id: &str,
    models: &[CodingModelProfile],
) -> Result<String, String> {
    if !template.contains(CLAUDE_MODEL_PRESENTATIONS_PLACEHOLDER) {
        return Ok(template.to_owned());
    }
    let uses_model_picker = template.contains(CLAUDE_MODEL_PICKER_PLACEHOLDER);
    let mut settings = serde_json::from_str::<serde_json::Value>(template)
        .map_err(|error| format!("Claude Code settings are not valid JSON: {error}"))?;
    let environment = settings
        .get_mut("env")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| "Claude Code settings have no 'env' object".to_owned())?;
    environment.remove(CLAUDE_MODEL_PRESENTATIONS_PLACEHOLDER);
    let presentations = if uses_model_picker {
        claude_gateway_model_presentations(models)
    } else {
        claude_model_presentations(selected_model_id, models)
    };
    for (key, value) in presentations {
        environment.insert(key, serde_json::Value::String(value));
    }
    serde_json::to_string(&settings)
        .map_err(|error| format!("could not serialize the Claude Code settings: {error}"))
}

pub(in crate::prepared) fn claude_model_presentations(
    selected_model_id: &str,
    models: &[CodingModelProfile],
) -> Vec<(String, String)> {
    if models
        .iter()
        .any(|model| !CLAUDE_CURATED_MODEL_PRIORITY.contains(&model.id.as_str()))
    {
        return claude_gateway_model_presentations(models);
    }

    let selected = models.iter().find(|model| model.id == selected_model_id);
    let ordered = selected
        .into_iter()
        .chain(
            CLAUDE_CURATED_MODEL_PRIORITY
                .iter()
                .filter_map(|model_id| models.iter().find(|model| model.id == *model_id))
                .filter(|model| model.id != selected_model_id),
        )
        .take(CLAUDE_MODEL_FAMILIES.len() + 1);

    let mut entries = Vec::new();
    for (slot, model) in ordered.enumerate() {
        let prefix = CLAUDE_MODEL_FAMILIES.get(slot).map_or_else(
            || "ANTHROPIC_CUSTOM_MODEL_OPTION".to_owned(),
            |family| format!("ANTHROPIC_DEFAULT_{family}_MODEL"),
        );
        append_claude_model_presentation(&mut entries, &prefix, model);
    }
    entries
}

/// Gateway discovery owns the complete picker as soon as NaN returns a model outside the
/// verified curated set. Qwen keeps only the Opus compatibility alias required by Claude
/// Code's native Auto permission mode; every model, including Qwen, remains present in the
/// credential-scoped gateway catalog and `availableModels` allowlist.
pub(in crate::prepared) fn claude_gateway_model_presentations(
    models: &[CodingModelProfile],
) -> Vec<(String, String)> {
    let mut entries = Vec::new();
    if let Some(qwen) = models
        .iter()
        .find(|model| model.id == nan_harness_core::CLAUDE_AUTO_MODE_PROVIDER_MODEL_ID)
    {
        append_claude_model_presentation(&mut entries, "ANTHROPIC_DEFAULT_OPUS_MODEL", qwen);
    }
    entries
}

pub(in crate::prepared) fn append_claude_model_presentation(
    entries: &mut Vec<(String, String)>,
    prefix: &str,
    model: &CodingModelProfile,
) {
    entries.push((prefix.to_owned(), claude_gateway_model_id(&model.id)));
    entries.push((format!("{prefix}_NAME"), model.display_name.clone()));
    entries.push((format!("{prefix}_DESCRIPTION"), model.description.clone()));
}
