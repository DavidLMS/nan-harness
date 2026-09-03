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
    if !streaming {
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
