use nan_harness_core::CodingModelProfile;
use nan_harness_core::launch_plan::{
    AIDER_MODEL_METADATA_PLACEHOLDER, AIDER_MODEL_SETTINGS_PLACEHOLDER,
    CLAUDE_MODEL_PICKER_PLACEHOLDER, CLAUDE_MODEL_PRESENTATIONS_PLACEHOLDER,
    CLINE_MODEL_CATALOG_PLACEHOLDER, DEEPSEEK_MODEL_CATALOG_PLACEHOLDER,
    GOOSE_MODEL_CATALOG_PLACEHOLDER, HERMES_MODEL_CATALOG_PLACEHOLDER,
    KIMI_CODE_MODEL_CATALOG_PLACEHOLDER, OPENCLAW_MODEL_ALIASES_PLACEHOLDER,
    OPENCLAW_MODEL_CATALOG_PLACEHOLDER, OPENCODE_MODEL_CATALOG_PLACEHOLDER,
    PI_MODEL_CATALOG_PLACEHOLDER, QWEN_CODE_MODEL_CATALOG_PLACEHOLDER,
    SELECTED_MODEL_CAPABILITIES_PLACEHOLDER, SELECTED_MODEL_CONTEXT_WINDOW_PLACEHOLDER,
    SELECTED_MODEL_DISPLAY_NAME_PLACEHOLDER, SELECTED_MODEL_MAX_OUTPUT_TOKENS_PLACEHOLDER,
    SELECTED_MODEL_REASONING_EFFORT_PLACEHOLDER,
};

mod claude;
mod json;
mod model;
mod structured;

#[expect(
    unused_imports,
    reason = "preserve the former catalogs module's internal facade"
)]
pub(super) use claude::{
    append_claude_model_presentation, claude_gateway_model_presentations, claude_model_picker,
    claude_model_presentations, render_claude_model_presentations,
};
pub(super) use json::{
    aider_model_metadata, aider_model_settings, cline_model_catalog, goose_model_catalog,
    hermes_model_catalog, openclaw_model_aliases, openclaw_model_catalog, opencode_model_catalog,
    pi_model_catalog, qwen_code_model_catalog, replace_json_placeholder,
};
pub(super) use model::{
    effort_name, model_input, reasoning_capable, render_reasoning_effort, render_selected_model,
    selected_model_reasoning_effort, unique_models,
};
#[expect(
    unused_imports,
    reason = "preserve the former catalogs module's internal facade"
)]
pub(super) use structured::{
    deepseek_model_catalog, kimi_code_model_catalog, kimi_model_capabilities, kimi_model_limits,
    kimi_model_table, render_kimi_model_catalog,
};

pub(super) fn contains_model_catalog_placeholder(value: &str) -> bool {
    [
        AIDER_MODEL_METADATA_PLACEHOLDER,
        AIDER_MODEL_SETTINGS_PLACEHOLDER,
        CLINE_MODEL_CATALOG_PLACEHOLDER,
        DEEPSEEK_MODEL_CATALOG_PLACEHOLDER,
        GOOSE_MODEL_CATALOG_PLACEHOLDER,
        HERMES_MODEL_CATALOG_PLACEHOLDER,
        OPENCODE_MODEL_CATALOG_PLACEHOLDER,
        OPENCLAW_MODEL_ALIASES_PLACEHOLDER,
        OPENCLAW_MODEL_CATALOG_PLACEHOLDER,
        PI_MODEL_CATALOG_PLACEHOLDER,
        QWEN_CODE_MODEL_CATALOG_PLACEHOLDER,
        KIMI_CODE_MODEL_CATALOG_PLACEHOLDER,
        CLAUDE_MODEL_PICKER_PLACEHOLDER,
        CLAUDE_MODEL_PRESENTATIONS_PLACEHOLDER,
        SELECTED_MODEL_CAPABILITIES_PLACEHOLDER,
        SELECTED_MODEL_CONTEXT_WINDOW_PLACEHOLDER,
        SELECTED_MODEL_DISPLAY_NAME_PLACEHOLDER,
        SELECTED_MODEL_MAX_OUTPUT_TOKENS_PLACEHOLDER,
        SELECTED_MODEL_REASONING_EFFORT_PLACEHOLDER,
    ]
    .iter()
    .any(|placeholder| value.contains(placeholder))
}

pub(super) fn render_model_catalogs(
    template: &str,
    provider_base_url: &str,
    selected_model_id: &str,
    model_catalog: Option<&[CodingModelProfile]>,
) -> Result<String, String> {
    if !contains_model_catalog_placeholder(template) {
        return Ok(template.to_owned());
    }
    let models = model_catalog
        .ok_or_else(|| "model catalog placeholders require live NaN model discovery".to_owned())?;
    let models = unique_models(models);
    let mut rendered = template.to_owned();
    render_selected_model(&mut rendered, selected_model_id, &models)?;
    replace_json_placeholder(
        &mut rendered,
        AIDER_MODEL_METADATA_PLACEHOLDER,
        &aider_model_metadata(&models),
    )?;
    replace_json_placeholder(
        &mut rendered,
        AIDER_MODEL_SETTINGS_PLACEHOLDER,
        &aider_model_settings(&models),
    )?;
    replace_json_placeholder(
        &mut rendered,
        CLINE_MODEL_CATALOG_PLACEHOLDER,
        &cline_model_catalog(&models),
    )?;
    replace_json_placeholder(
        &mut rendered,
        GOOSE_MODEL_CATALOG_PLACEHOLDER,
        &goose_model_catalog(&models),
    )?;
    replace_json_placeholder(
        &mut rendered,
        HERMES_MODEL_CATALOG_PLACEHOLDER,
        &hermes_model_catalog(&models),
    )?;
    replace_json_placeholder(
        &mut rendered,
        PI_MODEL_CATALOG_PLACEHOLDER,
        &pi_model_catalog(&models),
    )?;
    replace_json_placeholder(
        &mut rendered,
        OPENCODE_MODEL_CATALOG_PLACEHOLDER,
        &opencode_model_catalog(&models),
    )?;
    replace_json_placeholder(
        &mut rendered,
        OPENCLAW_MODEL_ALIASES_PLACEHOLDER,
        &openclaw_model_aliases(&models),
    )?;
    replace_json_placeholder(
        &mut rendered,
        OPENCLAW_MODEL_CATALOG_PLACEHOLDER,
        &openclaw_model_catalog(&models),
    )?;
    replace_json_placeholder(
        &mut rendered,
        QWEN_CODE_MODEL_CATALOG_PLACEHOLDER,
        &qwen_code_model_catalog(&models, provider_base_url),
    )?;
    rendered = rendered.replace(
        DEEPSEEK_MODEL_CATALOG_PLACEHOLDER,
        &deepseek_model_catalog(&models)?,
    );
    rendered = rendered.replace(
        KIMI_CODE_MODEL_CATALOG_PLACEHOLDER,
        &kimi_code_model_catalog(&models, selected_model_id)?,
    );
    rendered = render_claude_model_presentations(&rendered, selected_model_id, &models)?;
    replace_json_placeholder(
        &mut rendered,
        CLAUDE_MODEL_PICKER_PLACEHOLDER,
        &claude_model_picker(&models),
    )?;
    Ok(rendered)
}

#[cfg(test)]
mod tests {
    use super::{
        aider_model_metadata, aider_model_settings, claude_model_picker, cline_model_catalog,
        deepseek_model_catalog, goose_model_catalog, hermes_model_catalog, kimi_code_model_catalog,
        openclaw_model_aliases, openclaw_model_catalog, opencode_model_catalog, pi_model_catalog,
        qwen_code_model_catalog, render_model_catalogs, unique_models,
    };
    use nan_harness_core::coding_model_profile;
    use nan_harness_core::launch_plan::{
        AIDER_MODEL_METADATA_PLACEHOLDER, AIDER_MODEL_SETTINGS_PLACEHOLDER,
        CLAUDE_MODEL_PICKER_PLACEHOLDER, CLINE_MODEL_CATALOG_PLACEHOLDER,
        DEEPSEEK_MODEL_CATALOG_PLACEHOLDER, GOOSE_MODEL_CATALOG_PLACEHOLDER,
        HERMES_MODEL_CATALOG_PLACEHOLDER, KIMI_CODE_MODEL_CATALOG_PLACEHOLDER,
        OPENCLAW_MODEL_ALIASES_PLACEHOLDER, OPENCLAW_MODEL_CATALOG_PLACEHOLDER,
        OPENCODE_MODEL_CATALOG_PLACEHOLDER, PI_MODEL_CATALOG_PLACEHOLDER,
        QWEN_CODE_MODEL_CATALOG_PLACEHOLDER,
    };

    #[test]
    fn facade_preserves_specialized_catalog_output() {
        let qwen = coding_model_profile("qwen3.6").expect("known coding model");
        let deepseek = coding_model_profile("deepseek-v4-flash").expect("known coding model");
        let models = [qwen.clone(), deepseek, qwen];
        let unique = unique_models(&models);
        let provider_base_url = "https://nan.invalid/v1";
        let selected_model_id = "qwen3.6";
        let json_catalogs = [
            (
                AIDER_MODEL_METADATA_PLACEHOLDER,
                aider_model_metadata(&unique),
            ),
            (
                AIDER_MODEL_SETTINGS_PLACEHOLDER,
                aider_model_settings(&unique),
            ),
            (
                CLINE_MODEL_CATALOG_PLACEHOLDER,
                cline_model_catalog(&unique),
            ),
            (
                GOOSE_MODEL_CATALOG_PLACEHOLDER,
                goose_model_catalog(&unique),
            ),
            (
                HERMES_MODEL_CATALOG_PLACEHOLDER,
                hermes_model_catalog(&unique),
            ),
            (PI_MODEL_CATALOG_PLACEHOLDER, pi_model_catalog(&unique)),
            (
                OPENCODE_MODEL_CATALOG_PLACEHOLDER,
                opencode_model_catalog(&unique),
            ),
            (
                OPENCLAW_MODEL_ALIASES_PLACEHOLDER,
                openclaw_model_aliases(&unique),
            ),
            (
                OPENCLAW_MODEL_CATALOG_PLACEHOLDER,
                openclaw_model_catalog(&unique),
            ),
            (
                QWEN_CODE_MODEL_CATALOG_PLACEHOLDER,
                qwen_code_model_catalog(&unique, provider_base_url),
            ),
            (
                CLAUDE_MODEL_PICKER_PLACEHOLDER,
                claude_model_picker(&unique),
            ),
        ];

        for (placeholder, catalog) in json_catalogs {
            let rendered = render_model_catalogs(
                placeholder,
                provider_base_url,
                selected_model_id,
                Some(&models),
            )
            .expect("facade should render JSON catalog");
            assert_eq!(
                rendered,
                serde_json::to_string(&catalog).expect("catalog should serialize"),
                "facade output changed for {placeholder}"
            );
        }

        for (placeholder, expected) in [
            (
                DEEPSEEK_MODEL_CATALOG_PLACEHOLDER,
                deepseek_model_catalog(&unique).expect("DeepSeek catalog should render"),
            ),
            (
                KIMI_CODE_MODEL_CATALOG_PLACEHOLDER,
                kimi_code_model_catalog(&unique, selected_model_id)
                    .expect("Kimi Code catalog should render"),
            ),
        ] {
            assert_eq!(
                render_model_catalogs(
                    placeholder,
                    provider_base_url,
                    selected_model_id,
                    Some(&models),
                )
                .expect("facade should render structured catalog"),
                expected,
                "facade output changed for {placeholder}"
            );
        }
    }
}
