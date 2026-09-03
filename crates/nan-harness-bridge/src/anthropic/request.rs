mod messages;
mod reasoning;
mod tokens;
mod tools;
mod validation;
mod wire;

use crate::anthropic::auto_mode;
use crate::error::ApiError;
use nan_harness_core::model::ReasoningPolicy;
use serde_json::{Map, Value, json};

pub(crate) use tools::WebSearchInvocation;
pub(crate) use wire::MessagesRequest;

#[derive(Debug)]
pub(crate) struct TranslatedRequest {
    pub(crate) body: Value,
    pub(crate) stream: bool,
    pub(crate) auto_mode_stage: Option<auto_mode::ClassifierStage>,
}

pub(crate) fn translate(
    request: MessagesRequest,
    model: &str,
    max_output_tokens: u64,
    reasoning_policy: ReasoningPolicy,
) -> Result<TranslatedRequest, ApiError> {
    let max_tokens = validation::validate_generation_request(&request)?;
    let classifier_stage = reasoning::classifier_stage(&request)?;
    let MessagesRequest {
        system,
        messages,
        tools,
        tool_choice,
        stream,
        temperature,
        top_p,
        stop_sequences,
        thinking,
        output_config,
        ..
    } = request;

    let messages = messages::translate(system, messages)?;
    let mut body = Map::from_iter([
        ("model".to_owned(), Value::String(model.to_owned())),
        ("messages".to_owned(), Value::Array(messages)),
        (
            "max_tokens".to_owned(),
            Value::Number(max_tokens.min(max_output_tokens).into()),
        ),
        ("stream".to_owned(), Value::Bool(stream)),
    ]);

    if stream {
        body.insert("stream_options".to_owned(), json!({"include_usage": true}));
    }
    if let Some(temperature) = temperature {
        validation::insert_number(&mut body, "temperature", temperature)?;
    }
    if let Some(top_p) = top_p {
        validation::insert_number(&mut body, "top_p", top_p)?;
    }
    if !stop_sequences.is_empty() {
        body.insert("stop".to_owned(), json!(stop_sequences));
    }
    if !tools.is_empty() {
        body.insert(
            "tools".to_owned(),
            Value::Array(tools::translate_tools(tools)?),
        );
    }
    if let Some(choice) = tool_choice {
        tools::translate_tool_choice(choice, &mut body)?;
    }
    reasoning::translate_thinking(
        thinking,
        output_config.and_then(|config| config.effort),
        reasoning_policy,
        &mut body,
    )?;
    if let Some(stage) = classifier_stage {
        auto_mode::tune_for_qwen(stage, &mut body);
    }

    Ok(TranslatedRequest {
        body: Value::Object(body),
        stream,
        auto_mode_stage: classifier_stage,
    })
}

pub(crate) fn web_search_invocation(
    request: &MessagesRequest,
) -> Result<Option<WebSearchInvocation>, ApiError> {
    tools::web_search_invocation(request)
}

pub(crate) fn estimate_input_tokens(request: &MessagesRequest) -> u64 {
    tokens::estimate_input_tokens(request)
}

#[cfg(test)]
mod tests {
    use super::{
        MessagesRequest, WebSearchInvocation, estimate_input_tokens, translate,
        web_search_invocation,
    };
    use nan_harness_core::model::ReasoningPolicy;
    use serde_json::{Value, json};

    #[test]
    fn translates_claude_tool_round_trip() {
        let request: MessagesRequest = serde_json::from_value(json!({
            "model": "ignored-by-bridge",
            "max_tokens": 100_000,
            "stream": true,
            "system": [{"type": "text", "text": "Be precise"}],
            "tools": [{
                "name": "Read",
                "description": "Reads a file",
                "input_schema": {"type": "object", "properties": {"file_path": {"type": "string"}}}
            }],
            "messages": [
                {"role": "user", "content": "Read the file"},
                {"role": "assistant", "content": [{
                    "type": "tool_use", "id": "tool_1", "name": "Read",
                    "input": {"file_path": "/tmp/probe.txt"}
                }]},
                {"role": "user", "content": [{
                    "type": "tool_result", "tool_use_id": "tool_1", "content": "hello"
                }]}
            ]
        }))
        .expect("fixture should deserialize");

        let translated = translate(
            request,
            "qwen3.6",
            65_536,
            ReasoningPolicy::Toggle {
                default_enabled: true,
            },
        )
        .expect("translation should work");
        assert!(translated.stream);
        assert_eq!(translated.body["model"], "qwen3.6");
        assert_eq!(translated.body["max_tokens"], 65_536);
        assert_eq!(
            translated.body["messages"][2]["tool_calls"][0]["function"]["name"],
            "Read"
        );
        assert_eq!(translated.body["messages"][3]["role"], "tool");
        assert_eq!(translated.body["stream_options"]["include_usage"], true);
    }

    #[test]
    fn estimates_tokens_without_contacting_nan() {
        let request: MessagesRequest = serde_json::from_value(json!({
            "model": "qwen3.6",
            "max_tokens": 100,
            "messages": [{"role": "user", "content": "A moderately sized prompt"}]
        }))
        .expect("fixture should deserialize");

        assert!(estimate_input_tokens(&request) > 16);
    }

    #[test]
    fn rejects_unknown_content_blocks() {
        let request: MessagesRequest = serde_json::from_value(json!({
            "model": "qwen3.6",
            "max_tokens": 100,
            "messages": [{"role": "user", "content": [{"type": "document"}]}]
        }))
        .expect("unknown variants should deserialize");

        let error = translate(
            request,
            "qwen3.6",
            100,
            ReasoningPolicy::Toggle {
                default_enabled: true,
            },
        )
        .expect_err("translation must fail");
        assert_eq!(error.code(), "NH-BRIDGE-102");
    }

    #[test]
    fn emits_image_urls_for_base64_images() {
        let request: MessagesRequest = serde_json::from_value(json!({
            "model": "qwen3.6",
            "max_tokens": 100,
            "messages": [{"role": "user", "content": [{
                "type": "image",
                "source": {"type": "base64", "media_type": "image/png", "data": "AA=="}
            }]}]
        }))
        .expect("fixture should deserialize");

        let translated = translate(
            request,
            "qwen3.6",
            100,
            ReasoningPolicy::Toggle {
                default_enabled: true,
            },
        )
        .expect("translation should work");
        let url: &Value = &translated.body["messages"][0]["content"][0]["image_url"]["url"];
        assert_eq!(url, "data:image/png;base64,AA==");
    }

    #[test]
    fn forwards_images_for_the_new_nan_models_without_profile_gating() {
        for model_id in ["deepseek-v4-flash", "qwen3.8-flash", "glm5.3-flash"] {
            let request: MessagesRequest = serde_json::from_value(json!({
                "model": model_id,
                "max_tokens": 100,
                "messages": [{"role": "user", "content": [{
                    "type": "image",
                    "source": {"type": "base64", "media_type": "image/png", "data": "AA=="}
                }]}]
            }))
            .expect("fixture should deserialize");
            let model = nan_harness_core::coding_model_profile(model_id)
                .expect("new NaN model should be profiled");

            let translated = translate(request, model_id, model.max_output_tokens, model.reasoning)
                .expect("translation should work");
            let url = &translated.body["messages"][0]["content"][0]["image_url"]["url"];
            assert_eq!(url, "data:image/png;base64,AA==");
        }
    }

    #[test]
    fn moves_claude_system_messages_to_the_first_upstream_position() {
        let request: MessagesRequest = serde_json::from_value(json!({
            "model": "qwen3.6",
            "max_tokens": 100,
            "system": "primary system prompt",
            "messages": [
                {"role": "user", "content": "hello"},
                {"role": "system", "content": [{"type": "text", "text": "runtime context"}]}
            ]
        }))
        .expect("fixture should deserialize");

        let translated = translate(
            request,
            "qwen3.6",
            100,
            ReasoningPolicy::Toggle {
                default_enabled: true,
            },
        )
        .expect("translation should work");
        assert_eq!(translated.body["messages"][0]["role"], "system");
        assert_eq!(
            translated.body["messages"][0]["content"],
            "primary system prompt\n\nruntime context"
        );
        assert_eq!(translated.body["messages"][1]["role"], "user");
    }

    #[test]
    fn recognizes_claude_code_web_search_server_requests() {
        let request: MessagesRequest = serde_json::from_value(json!({
            "model": "qwen3.6",
            "max_tokens": 32_000,
            "stream": true,
            "tools": [{
                "type": "web_search_20250305",
                "name": "web_search",
                "max_uses": 8
            }],
            "tool_choice": {"type": "tool", "name": "web_search"},
            "messages": [{
                "role": "user",
                "content": "Perform a web search for the query: best Rust async runtime 2025"
            }]
        }))
        .expect("server tool fixture should deserialize");

        let invocation = web_search_invocation(&request)
            .expect("server tool should be valid")
            .expect("web search should be recognized");
        assert_eq!(
            invocation,
            WebSearchInvocation {
                query: "best Rust async runtime 2025".to_owned(),
                max_results: 8,
                allowed_domains: Vec::new(),
                blocked_domains: Vec::new(),
                stream: true,
            }
        );
    }
}
