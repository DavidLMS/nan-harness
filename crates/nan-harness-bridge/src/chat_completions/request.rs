use crate::error::ApiError;
use axum::body::Bytes;
use nan_harness_core::is_valid_provider_model_id;
use serde_json::Value;

pub(super) struct PreparedChatBody {
    pub(super) body: Bytes,
    pub(super) streaming: bool,
    pub(super) requested_model_id: Option<String>,
}

pub(super) fn prepare_chat_body(body: &[u8]) -> Result<PreparedChatBody, ApiError> {
    let mut value: Value = serde_json::from_slice(body)
        .map_err(|error| ApiError::InvalidRequest(format!("invalid JSON body: {error}")))?;
    let requested_model_id = value
        .get("model")
        .and_then(Value::as_str)
        .filter(|model_id| is_valid_provider_model_id(model_id))
        .map(ToOwned::to_owned);
    let streaming = value
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    // Pi emits an empty tool list when replaying tool history without active tools.
    // NaN rejects that list; omit it while preserving the calls and their results.
    let empty_tools = value
        .get("tools")
        .and_then(Value::as_array)
        .is_some_and(Vec::is_empty);
    if empty_tools && let Some(object) = value.as_object_mut() {
        object.remove("tools");
    }
    if !streaming && !empty_tools {
        return Ok(PreparedChatBody {
            body: Bytes::copy_from_slice(body),
            streaming,
            requested_model_id,
        });
    }
    if streaming {
        let options = value
            .as_object_mut()
            .ok_or_else(|| {
                ApiError::InvalidRequest("request body must be a JSON object".to_owned())
            })?
            .entry("stream_options")
            .or_insert_with(|| Value::Object(serde_json::Map::new()));
        let options = options.as_object_mut().ok_or_else(|| {
            ApiError::InvalidRequest("stream_options must be a JSON object".to_owned())
        })?;
        options.insert("include_usage".to_owned(), Value::Bool(true));
    }
    serde_json::to_vec(&value)
        .map(|body| PreparedChatBody {
            body: Bytes::from(body),
            streaming,
            requested_model_id,
        })
        .map_err(|error| ApiError::InvalidRequest(format!("could not encode JSON body: {error}")))
}

#[cfg(test)]
mod tests {
    use super::prepare_chat_body;
    use crate::error::ApiError;
    use serde_json::{Value, json};

    #[test]
    fn non_streaming_requests_keep_their_original_serialization() {
        let body = br#"{ "model": "qwen3.6", "stream": false, "messages": [] }"#;

        let prepared = prepare_chat_body(body).expect("request should be prepared");

        assert_eq!(prepared.body.as_ref(), body);
        assert!(!prepared.streaming);
        assert_eq!(prepared.requested_model_id.as_deref(), Some("qwen3.6"));
    }

    #[test]
    fn streaming_requests_enable_usage_and_keep_other_fields() {
        let body = serde_json::to_vec(&json!({
            "model": "qwen3.6",
            "stream": true,
            "stream_options": {"include_usage": false, "custom": "preserved"},
            "messages": [{"role": "user", "content": "hello"}]
        }))
        .expect("fixture should serialize");

        let prepared = prepare_chat_body(&body).expect("request should be prepared");
        let value: Value = serde_json::from_slice(&prepared.body).expect("prepared JSON");

        assert!(prepared.streaming);
        assert_eq!(value["stream_options"]["include_usage"], true);
        assert_eq!(value["stream_options"]["custom"], "preserved");
        assert_eq!(value["messages"][0]["content"], "hello");
    }

    #[test]
    fn empty_tools_are_omitted_without_changing_tool_history() {
        let messages = json!([
            {"role": "user", "content": "Call ping."},
            {"role": "assistant", "content": null, "tool_calls": [
                {"id": "call_synthetic", "type": "function", "function": {
                    "name": "ping", "arguments": "{}"
                }}
            ]},
            {"role": "tool", "tool_call_id": "call_synthetic", "content": "pong"}
        ]);
        for streaming in [false, true] {
            let body = json!({
                "model": "gemma4", "stream": streaming, "tools": [],
                "messages": messages, "store": false, "max_tokens": 8
            });
            let prepared = prepare_chat_body(&serde_json::to_vec(&body).unwrap()).unwrap();
            let actual: Value = serde_json::from_slice(&prepared.body).unwrap();
            let mut expected = body;
            expected.as_object_mut().unwrap().remove("tools");
            if streaming {
                expected["stream_options"] = json!({"include_usage": true});
            }
            assert_eq!(actual, expected);
        }
    }

    #[test]
    fn nonempty_and_invalid_tools_are_preserved() {
        for tools in [
            json!([{"type": "function", "function": {
                "name": "ping", "parameters": {"type": "object"}
            }}]),
            Value::Null,
            json!({}),
            json!("invalid"),
        ] {
            for streaming in [false, true] {
                let body = json!({"stream": streaming, "tools": tools});
                let prepared = prepare_chat_body(&serde_json::to_vec(&body).unwrap()).unwrap();
                let actual: Value = serde_json::from_slice(&prepared.body).unwrap();
                assert_eq!(actual["tools"], tools);
            }
        }
    }

    #[test]
    fn streaming_request_validation_keeps_the_existing_error_contract() {
        let error = prepare_chat_body(br#"{"stream":true,"stream_options":false}"#)
            .err()
            .expect("request should be rejected");

        assert!(matches!(
            error,
            ApiError::InvalidRequest(message)
                if message == "stream_options must be a JSON object"
        ));
    }
}
