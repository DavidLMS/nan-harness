use super::unsafe_resource;
use crate::error::PlanError;
use crate::launch_plan::{
    AIDER_MODEL_METADATA_PLACEHOLDER, AIDER_MODEL_SETTINGS_PLACEHOLDER,
    BRIDGE_BASE_URL_PLACEHOLDER, CLAUDE_AVAILABLE_MODELS_PLACEHOLDER,
    CLAUDE_MODEL_PRESENTATIONS_PLACEHOLDER, CLINE_MODEL_CATALOG_PLACEHOLDER,
    CODEX_HOME_PLACEHOLDER, CODEX_MODEL_CATALOG_PLACEHOLDER, DEEPSEEK_MODEL_CATALOG_PLACEHOLDER,
    GOOSE_ADDITIONAL_CONFIG_FILES_PLACEHOLDER, GOOSE_MODEL_CATALOG_PLACEHOLDER,
    HERMES_MODEL_CATALOG_PLACEHOLDER, KIMI_CODE_MODEL_CATALOG_PLACEHOLDER, LaunchPlan,
    NAN_SEARCH_BLOCK_BEGIN, NAN_SEARCH_BLOCK_END, OPENCLAW_MODEL_ALIASES_PLACEHOLDER,
    OPENCLAW_MODEL_CATALOG_PLACEHOLDER, OPENCODE_MODEL_CATALOG_PLACEHOLDER,
    PI_MODEL_CATALOG_PLACEHOLDER, PROVIDER_BASE_URL_PLACEHOLDER,
    QWEN_CODE_MODEL_CATALOG_PLACEHOLDER, SELECTED_MODEL_CAPABILITIES_PLACEHOLDER,
    SELECTED_MODEL_CONTEXT_WINDOW_PLACEHOLDER, SELECTED_MODEL_DISPLAY_NAME_PLACEHOLDER,
    SELECTED_MODEL_MAX_OUTPUT_TOKENS_PLACEHOLDER, SELECTED_MODEL_REASONING_EFFORT_PLACEHOLDER,
    Transport, USER_HOME_PLACEHOLDER,
};
use crate::secret::SecretRef;

pub(super) fn validate_template_placeholders(
    plan: &LaunchPlan,
    resource_id: &str,
    template: Option<&str>,
) -> Result<(), PlanError> {
    let Some(template) = template else {
        return Ok(());
    };
    validate_nan_search_blocks(resource_id, template)?;
    let mut remainder = template
        .replace(BRIDGE_BASE_URL_PLACEHOLDER, "")
        .replace(PROVIDER_BASE_URL_PLACEHOLDER, "")
        .replace(CLAUDE_AVAILABLE_MODELS_PLACEHOLDER, "")
        .replace(CLAUDE_MODEL_PRESENTATIONS_PLACEHOLDER, "")
        .replace(CODEX_MODEL_CATALOG_PLACEHOLDER, "")
        .replace(SELECTED_MODEL_REASONING_EFFORT_PLACEHOLDER, "")
        .replace(AIDER_MODEL_METADATA_PLACEHOLDER, "")
        .replace(AIDER_MODEL_SETTINGS_PLACEHOLDER, "")
        .replace(CLINE_MODEL_CATALOG_PLACEHOLDER, "")
        .replace(DEEPSEEK_MODEL_CATALOG_PLACEHOLDER, "")
        .replace(GOOSE_MODEL_CATALOG_PLACEHOLDER, "")
        .replace(GOOSE_ADDITIONAL_CONFIG_FILES_PLACEHOLDER, "")
        .replace(HERMES_MODEL_CATALOG_PLACEHOLDER, "")
        .replace(OPENCODE_MODEL_CATALOG_PLACEHOLDER, "")
        .replace(OPENCLAW_MODEL_ALIASES_PLACEHOLDER, "")
        .replace(OPENCLAW_MODEL_CATALOG_PLACEHOLDER, "")
        .replace(PI_MODEL_CATALOG_PLACEHOLDER, "")
        .replace(QWEN_CODE_MODEL_CATALOG_PLACEHOLDER, "")
        .replace(KIMI_CODE_MODEL_CATALOG_PLACEHOLDER, "")
        .replace(SELECTED_MODEL_DISPLAY_NAME_PLACEHOLDER, "")
        .replace(SELECTED_MODEL_CONTEXT_WINDOW_PLACEHOLDER, "")
        .replace(SELECTED_MODEL_MAX_OUTPUT_TOKENS_PLACEHOLDER, "")
        .replace(SELECTED_MODEL_CAPABILITIES_PLACEHOLDER, "")
        .replace(USER_HOME_PLACEHOLDER, "")
        .replace(CODEX_HOME_PLACEHOLDER, "")
        .replace(NAN_SEARCH_BLOCK_BEGIN, "")
        .replace(NAN_SEARCH_BLOCK_END, "");

    if let Some(session_token_ref) = session_token_reference(&plan.transport) {
        remainder = remainder.replace(&format!("{{secret:{}}}", session_token_ref.as_str()), "");
    }

    if remainder.contains("{runtime:") || remainder.contains("{secret:") {
        unsafe_resource(
            resource_id,
            "contentTemplate contains an unknown runtime or secret placeholder",
        )
    } else {
        Ok(())
    }
}

fn validate_nan_search_blocks(resource_id: &str, template: &str) -> Result<(), PlanError> {
    let mut remainder = template;
    let mut inside_block = false;
    loop {
        let begin = remainder.find(NAN_SEARCH_BLOCK_BEGIN);
        let end = remainder.find(NAN_SEARCH_BLOCK_END);
        match (begin, end, inside_block) {
            (None, None, false) => return Ok(()),
            (Some(begin), Some(end), false) if begin < end => {
                inside_block = true;
                remainder = &remainder[begin + NAN_SEARCH_BLOCK_BEGIN.len()..];
            }
            (Some(begin), Some(end), true) if end < begin => {
                inside_block = false;
                remainder = &remainder[end + NAN_SEARCH_BLOCK_END.len()..];
            }
            (None, Some(end), true) => {
                inside_block = false;
                remainder = &remainder[end + NAN_SEARCH_BLOCK_END.len()..];
            }
            _ => {
                return unsafe_resource(
                    resource_id,
                    "contentTemplate contains malformed or nested NaN search blocks",
                );
            }
        }
    }
}

fn session_token_reference(transport: &Transport) -> Option<&SecretRef> {
    match transport {
        Transport::AnthropicBridge {
            session_token_ref, ..
        }
        | Transport::ResponsesBridge {
            session_token_ref, ..
        }
        | Transport::FxGatewayBridge {
            session_token_ref, ..
        } => Some(session_token_ref),
        Transport::DirectChat { .. } => None,
    }
}
