use super::ResponsesRequest;
use crate::error::ApiError;
use serde_json::Value;

pub(super) fn validate_request(request: &ResponsesRequest) -> Result<(), ApiError> {
    if request.model.trim().is_empty() {
        return Err(ApiError::InvalidRequest("model cannot be empty".to_owned()));
    }
    if !request.stream {
        return Err(ApiError::InvalidRequest(
            "Codex must request a streaming response".to_owned(),
        ));
    }
    Ok(())
}

pub(super) fn required_string<'a>(value: &'a Value, field: &str) -> Result<&'a str, ApiError> {
    value.get(field).and_then(Value::as_str).ok_or_else(|| {
        ApiError::InvalidRequest(format!("Responses item requires string field '{field}'"))
    })
}
