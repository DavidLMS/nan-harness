use crate::error::ApiError;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

pub(crate) trait StreamChunk {
    fn stream_error(&self) -> Option<&Value>;
}

pub(crate) fn deserialize_error<'de, D>(deserializer: D) -> Result<Option<Value>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Value::deserialize(deserializer).map(Some)
}

pub(crate) fn parse_chunk<T>(data: &str) -> Result<T, ApiError>
where
    T: DeserializeOwned + StreamChunk,
{
    if let Ok(chunk) = serde_json::from_str::<T>(data) {
        if let Some(error) = chunk.stream_error() {
            return Err(ApiError::InvalidUpstream(upstream_error_detail(error)));
        }
        return Ok(chunk);
    }

    let value: Value = serde_json::from_str(data)
        .map_err(|error| ApiError::InvalidUpstream(format!("invalid streaming JSON: {error}")))?;
    if let Some(error) = value.get("error") {
        return Err(ApiError::InvalidUpstream(upstream_error_detail(error)));
    }
    serde_json::from_value(value)
        .map_err(|error| ApiError::InvalidUpstream(format!("invalid streaming chunk: {error}")))
}

fn upstream_error_detail(error: &Value) -> String {
    error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("NaN returned a streaming error")
        .to_owned()
}

#[cfg(test)]
pub(crate) mod test_support {
    use crate::upstream::UpstreamResponse;
    use axum::http::Response as HttpResponse;
    use reqwest::Body;

    pub(crate) fn response(body: &str) -> UpstreamResponse {
        UpstreamResponse::uncoordinated(reqwest::Response::from(
            HttpResponse::builder()
                .header("content-type", "text/event-stream")
                .body(Body::from(body.to_owned()))
                .expect("test response should build"),
        ))
    }
}
