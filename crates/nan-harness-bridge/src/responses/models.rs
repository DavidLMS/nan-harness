use serde_json::{Value, json};

#[must_use]
pub fn catalog(model: &str) -> Value {
    let (display_name, context_window, image_input) = match model {
        "qwen3.6" => ("NaN · Qwen 3.6", 262_144, true),
        "deepseek-v4-flash" => ("NaN · DeepSeek V4 Flash", 1_000_000, false),
        "mimo-v2.5" => ("NaN · MiMo V2.5", 1_000_000, true),
        "gemma4" => ("NaN · Gemma 4", 262_144, true),
        _ => (model, 262_144, false),
    };
    let input_modalities = if image_input {
        json!(["text", "image"])
    } else {
        json!(["text"])
    };
    json!({
        "models": [{
            "slug": model,
            "display_name": display_name,
            "description": "NaN model routed through the local NaN Harness bridge.",
            "default_reasoning_level": null,
            "supported_reasoning_levels": [],
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
            "context_window": context_window,
            "max_context_window": context_window,
            "auto_compact_token_limit": null,
            "effective_context_window_percent": 90,
            "experimental_supported_tools": [],
            "input_modalities": input_modalities,
            "supports_search_tool": false,
            "use_responses_lite": false,
            "tool_mode": "direct",
            "multi_agent_version": "v1"
        }]
    })
}

pub(crate) fn max_output_tokens(model: &str) -> u64 {
    match model {
        "deepseek-v4-flash" => 262_144,
        "qwen3.6" | "mimo-v2.5" | "gemma4" => 65_536,
        _ => 32_768,
    }
}
