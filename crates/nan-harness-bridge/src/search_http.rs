use crate::auth::is_authorized;
use crate::error::ApiError;
use crate::search_service::{self, SearchRequest};
use crate::upstream::NanClient;
use axum::Json;
use axum::body::Bytes;
use axum::http::HeaderMap;
use nan_harness_core::SecretValue;
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct HttpSearchRequest {
    query: String,
    #[serde(default = "default_max_results")]
    max_results: usize,
    #[serde(default)]
    allowed_domains: Vec<String>,
    #[serde(default)]
    blocked_domains: Vec<String>,
}

pub(crate) async fn execute(
    headers: &HeaderMap,
    body: &Bytes,
    upstream: &NanClient,
    session_token: &SecretValue,
) -> Result<Json<Value>, ApiError> {
    if !is_authorized(headers, session_token) {
        return Err(ApiError::Unauthorized);
    }
    let request: HttpSearchRequest = serde_json::from_slice(body)
        .map_err(|error| ApiError::InvalidRequest(format!("invalid search JSON: {error}")))?;
    let results = search_service::execute(
        upstream,
        SearchRequest {
            query: request.query,
            max_results: request.max_results,
            allowed_domains: request.allowed_domains,
            blocked_domains: request.blocked_domains,
        },
    )
    .await?;
    let summary = search_service::result_summary(&results);
    Ok(Json(json!({"results": results, "summary": summary})))
}

const fn default_max_results() -> usize {
    10
}
