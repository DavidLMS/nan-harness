use super::wire::MessagesRequest;
use crate::error::ApiError;
use serde_json::{Map, Value};

pub(super) fn validate_generation_request(request: &MessagesRequest) -> Result<u64, ApiError> {
    if request.messages.is_empty() {
        return Err(ApiError::InvalidRequest(
            "messages must contain at least one message".to_owned(),
        ));
    }
    let max_tokens = request.max_tokens.ok_or_else(|| {
        ApiError::InvalidRequest("max_tokens is required for message generation".to_owned())
    })?;
    if max_tokens == 0 {
        return Err(ApiError::InvalidRequest(
            "max_tokens must be greater than zero".to_owned(),
        ));
    }
    Ok(max_tokens)
}

pub(super) fn insert_number(
    body: &mut Map<String, Value>,
    name: &str,
    value: f64,
) -> Result<(), ApiError> {
    let number = serde_json::Number::from_f64(value)
        .ok_or_else(|| ApiError::InvalidRequest(format!("{name} must be a finite number")))?;
    body.insert(name.to_owned(), Value::Number(number));
    Ok(())
}

pub(crate) fn unsupported_content<T>() -> Result<T, ApiError> {
    Err(ApiError::InvalidRequest(
        "request contains an unsupported content block".to_owned(),
    ))
}
