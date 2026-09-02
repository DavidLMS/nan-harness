use crate::error::ApiError;
use nan_harness_core::model::{CodingModelProfile, ReasoningHint, ReasoningSelection};
use serde_json::{Value, json};

const PERMISSION_REVIEW_TOOL: &str = "permission_decision";

#[derive(Debug, Clone)]
pub(super) struct ProviderSearchTool {
    pub(super) name: String,
    pub(super) max_results: usize,
    pub(super) allowed_domains: Vec<String>,
    pub(super) blocked_domains: Vec<String>,
}

pub(super) fn translate(request: &Value, model: &CodingModelProfile) -> Result<Value, ApiError> {
    let prompt = request
        .get("prompt")
        .and_then(Value::as_array)
        .ok_or_else(|| ApiError::InvalidRequest("fx request is missing prompt".to_owned()))?;
    let mut messages = Vec::new();
    for message in prompt {
        translate_message(message, &mut messages)?;
    }

    let mut body = json!({
        "model": model.id,
        "messages": messages,
        "stream": true,
        "stream_options": {"include_usage": true}
    });
    if let Some(max_tokens) = request.get("maxOutputTokens").and_then(Value::as_u64) {
        body["max_tokens"] = json!(max_tokens);
    }
    if let Some(tools) = request.get("tools").and_then(Value::as_array) {
        body["tools"] = Value::Array(tools.iter().map(translate_tool).collect());
    }
    if let Some(choice) = request.get("toolChoice") {
        body["tool_choice"] = translate_tool_choice(choice);
    }
    if let Some(reasoning) = request.get("reasoning").and_then(Value::as_str) {
        apply_reasoning(&mut body, model, reasoning)?;
    }
    Ok(body)
}

fn translate_message(message: &Value, output: &mut Vec<Value>) -> Result<(), ApiError> {
    let role = message
        .get("role")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::InvalidRequest("fx prompt message has no role".to_owned()))?;
    let content = message.get("content").cloned().unwrap_or_else(|| json!(""));
    match role {
        "system" | "user" => output.push(json!({
            "role": role,
            "content": content_for_chat(&content)
        })),
        "assistant" => {
            let parts = content.as_array().cloned().unwrap_or_default();
            let text = parts
                .iter()
                .filter(|part| part.get("type").and_then(Value::as_str) == Some("text"))
                .filter_map(|part| part.get("text").and_then(Value::as_str))
                .collect::<String>();
            let tool_calls = parts
                .iter()
                .filter(|part| part.get("type").and_then(Value::as_str) == Some("tool-call"))
                .map(|part| {
                    json!({
                        "id": part.get("toolCallId").and_then(Value::as_str).unwrap_or("fx_tool_call"),
                        "type": "function",
                        "function": {
                            "name": part.get("toolName").and_then(Value::as_str).unwrap_or("tool"),
                            "arguments": serde_json::to_string(part.get("input").unwrap_or(&Value::Null)).unwrap_or_else(|_| "{}".to_owned())
                        }
                    })
                })
                .collect::<Vec<_>>();
            let mut translated = json!({"role":"assistant","content":text});
            if !tool_calls.is_empty() {
                translated["tool_calls"] = Value::Array(tool_calls);
            }
            output.push(translated);
        }
        "tool" => {
            for part in content.as_array().into_iter().flatten() {
                if part.get("type").and_then(Value::as_str) != Some("tool-result") {
                    continue;
                }
                output.push(json!({
                    "role": "tool",
                    "tool_call_id": part.get("toolCallId").and_then(Value::as_str).unwrap_or("fx_tool_call"),
                    "content": tool_result_text(part.get("output"))
                }));
            }
        }
        other => {
            return Err(ApiError::InvalidRequest(format!(
                "unsupported fx prompt role '{other}'"
            )));
        }
    }
    Ok(())
}

fn content_for_chat(content: &Value) -> Value {
    match content {
        Value::String(value) => Value::String(value.clone()),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| match part.get("type").and_then(Value::as_str) {
                Some("text") => Some(json!({
                    "type": "text",
                    "text": part.get("text").and_then(Value::as_str).unwrap_or_default()
                })),
                Some("file") => {
                    let data = part.get("data").and_then(Value::as_str)?;
                    let media_type = part
                        .get("mediaType")
                        .and_then(Value::as_str)
                        .unwrap_or("application/octet-stream");
                    Some(json!({
                        "type": "image_url",
                        "image_url": {"url": format!("data:{media_type};base64,{data}")}
                    }))
                }
                _ => None,
            })
            .collect::<Vec<_>>()
            .into(),
        _ => Value::String(content.to_string()),
    }
}

fn tool_result_text(output: Option<&Value>) -> String {
    match output {
        Some(Value::Object(value)) if value.get("type").and_then(Value::as_str) == Some("text") => {
            value
                .get("value")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        }
        Some(value) => value.to_string(),
        None => String::new(),
    }
}

fn translate_tool(tool: &Value) -> Value {
    let provider_name = tool.get("name").and_then(Value::as_str).unwrap_or("tool");
    let parameters = if tool.get("type").and_then(Value::as_str) == Some("provider") {
        json!({
            "type": "object",
            "properties": {
                "query": {"type": "string"}
            }
        })
    } else {
        tool.get("inputSchema")
            .cloned()
            .unwrap_or_else(|| json!({"type":"object"}))
    };
    json!({
        "type": "function",
        "function": {
            "name": provider_name,
            "description": tool.get("description").and_then(Value::as_str).unwrap_or_default(),
            "parameters": parameters
        }
    })
}

fn translate_tool_choice(choice: &Value) -> Value {
    match choice.get("type").and_then(Value::as_str).unwrap_or("auto") {
        "required" => json!("required"),
        "none" => json!("none"),
        _ => json!("auto"),
    }
}

pub(super) fn is_permission_review(request: &Value) -> bool {
    request
        .get("tools")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .any(|tool| tool.get("name").and_then(Value::as_str) == Some(PERMISSION_REVIEW_TOOL))
}

pub(super) fn provider_search_tool(request: &Value) -> Option<ProviderSearchTool> {
    request
        .get("tools")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find_map(|tool| {
            let name = tool.get("name").and_then(Value::as_str)?;
            let id = tool.get("id").and_then(Value::as_str)?;
            let supported = matches!(
                (id, name),
                ("gateway.perplexity_search", "perplexity_search")
                    | ("gateway.parallel_search", "parallel_search")
            );
            if !supported {
                return None;
            }
            let args = tool.get("args").unwrap_or(&Value::Null);
            let max_results = args
                .get("maxResults")
                .and_then(Value::as_u64)
                .unwrap_or(10)
                .clamp(1, 20) as usize;
            let mut allowed_domains = Vec::new();
            let mut blocked_domains = Vec::new();
            if name == "perplexity_search" {
                for domain in args
                    .get("searchDomainFilter")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                {
                    if let Some(domain) = domain.strip_prefix('-') {
                        blocked_domains.push(domain.to_owned());
                    } else {
                        allowed_domains.push(domain.to_owned());
                    }
                }
            } else if let Some(source_policy) = args.get("sourcePolicy") {
                allowed_domains = string_array(source_policy.get("includeDomains"));
                blocked_domains = string_array(source_policy.get("excludeDomains"));
            }
            Some(ProviderSearchTool {
                name: name.to_owned(),
                max_results,
                allowed_domains,
                blocked_domains,
            })
        })
}

fn string_array(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

pub(super) fn latest_user_text(request: &Value) -> String {
    request
        .get("prompt")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .rev()
        .find(|message| message.get("role").and_then(Value::as_str) == Some("user"))
        .map(|message| message_text(message.get("content").unwrap_or(&Value::Null)))
        .filter(|text| !text.trim().is_empty())
        .unwrap_or_else(|| "web search".to_owned())
}

fn message_text(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter(|part| part.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn apply_reasoning(
    body: &mut Value,
    model: &CodingModelProfile,
    effort: &str,
) -> Result<(), ApiError> {
    let hint = match effort {
        "none" => ReasoningHint::Disabled,
        "low" => ReasoningHint::Low,
        "medium" => ReasoningHint::Medium,
        "high" => ReasoningHint::High,
        "xhigh" => ReasoningHint::ExtraHigh,
        other => {
            return Err(ApiError::InvalidRequest(format!(
                "unsupported fx reasoning effort '{other}'"
            )));
        }
    };
    let selection = model.reasoning.resolve_hint(hint).ok_or_else(|| {
        ApiError::InvalidRequest(format!(
            "reasoning effort '{effort}' is incompatible with model policy"
        ))
    })?;
    match selection {
        ReasoningSelection::Toggle(enabled)
            if model.id.starts_with("qwen") || model.id.starts_with("gemma") =>
        {
            body["chat_template_kwargs"] = json!({"enable_thinking": enabled});
        }
        ReasoningSelection::Effort(effort) => {
            body["reasoning_effort"] = serde_json::to_value(effort).expect("effort serializes");
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{apply_reasoning, latest_user_text, provider_search_tool, translate};
    use nan_harness_core::CodingModelProfile;
    use serde_json::json;

    #[test]
    fn reasoning_hints_follow_shared_model_policy_resolution() {
        let qwen = nan_harness_core::coding_model_profile("qwen3.6").expect("known model");
        let mut qwen_body = json!({});
        apply_reasoning(&mut qwen_body, &qwen, "medium")
            .expect("positive effort should enable toggle reasoning");
        assert_eq!(qwen_body["chat_template_kwargs"]["enable_thinking"], true);

        let qwen38 = nan_harness_core::coding_model_profile("qwen3.8-flash").expect("known model");
        let mut qwen38_body = json!({});
        apply_reasoning(&mut qwen38_body, &qwen38, "high")
            .expect("always-on reasoning should be accepted");
        assert_eq!(qwen38_body["chat_template_kwargs"]["enable_thinking"], true);
        assert!(apply_reasoning(&mut qwen38_body, &qwen38, "none").is_err());

        let glm53 = nan_harness_core::coding_model_profile("glm5.3-flash").expect("known model");
        let mut glm53_body = json!({});
        apply_reasoning(&mut glm53_body, &glm53, "low")
            .expect("effort reasoning should be accepted");
        assert_eq!(glm53_body["reasoning_effort"], "low");
        assert!(apply_reasoning(&mut glm53_body, &glm53, "none").is_err());

        let mut future_effort = glm53.clone();
        future_effort.id = "future-effort-model".to_owned();
        let mut future_effort_body = json!({});
        apply_reasoning(&mut future_effort_body, &future_effort, "low")
            .expect("catalog effort policy should not depend on model family names");
        assert_eq!(future_effort_body["reasoning_effort"], "low");

        let mimo = nan_harness_core::coding_model_profile("mimo-v2.5").expect("known model");
        let mut mimo_body = json!({});
        apply_reasoning(&mut mimo_body, &mimo, "medium")
            .expect("positive effort should preserve always-on reasoning");
        assert_eq!(mimo_body, json!({}));

        let generic = CodingModelProfile::generic("future-coding-model");
        let mut generic_body = json!({});
        apply_reasoning(&mut generic_body, &generic, "medium")
            .expect("unprofiled models should use native reasoning defaults");
        assert_eq!(generic_body, json!({}));
    }

    #[test]
    fn translation_and_stream_context_share_request_boundaries() {
        let model = nan_harness_core::coding_model_profile("qwen3.6").expect("known model");
        let request = json!({
            "prompt": [
                {"role":"user","content":"first query"},
                {"role":"assistant","content":[]},
                {"role":"user","content":[
                    {"type":"text","text":"latest"},
                    {"type":"text","text":"question"}
                ]}
            ],
            "tools": [{
                "type":"provider",
                "id":"gateway.perplexity_search",
                "name":"perplexity_search",
                "args":{
                    "maxResults":25,
                    "searchDomainFilter":["docs.rs","-example.com"]
                }
            }]
        });

        let translated = translate(&request, &model).expect("request should translate");
        let search = provider_search_tool(&request).expect("search tool should be detected");

        assert_eq!(
            translated["tools"][0]["function"]["name"],
            "perplexity_search"
        );
        assert_eq!(latest_user_text(&request), "latest\nquestion");
        assert_eq!(search.max_results, 20);
        assert_eq!(search.allowed_domains, ["docs.rs"]);
        assert_eq!(search.blocked_domains, ["example.com"]);
    }
}
