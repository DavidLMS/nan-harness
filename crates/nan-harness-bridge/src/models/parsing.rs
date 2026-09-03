use super::sanitization::sanitize_discovery_error;
use crate::error::BridgeError;
use serde::Deserialize;
use std::collections::BTreeSet;

pub(super) const MAX_MODELS_RESPONSE_BYTES: usize = 1024 * 1024;
const MAX_DISCOVERY_ERROR_BYTES: usize = 64 * 1024;

pub(super) fn parse_models_response(body: &[u8]) -> Result<BTreeSet<String>, serde_json::Error> {
    let payload = serde_json::from_slice::<NanModelsResponse>(body)?;
    Ok(payload.data.into_iter().map(|model| model.id).collect())
}

pub(super) async fn read_bounded_models_response(
    response: &mut reqwest::Response,
) -> Result<Vec<u8>, BridgeError> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_MODELS_RESPONSE_BYTES as u64)
    {
        return Err(BridgeError::ModelDiscoveryTooLarge);
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(BridgeError::ModelDiscoveryTransport)?
    {
        let next_len = body.len().saturating_add(chunk.len());
        if next_len > MAX_MODELS_RESPONSE_BYTES {
            return Err(BridgeError::ModelDiscoveryTooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

pub(super) async fn read_discovery_error_prefix(response: &mut reqwest::Response) -> String {
    let mut prefix = Vec::new();
    while prefix.len() < MAX_DISCOVERY_ERROR_BYTES {
        let Ok(Some(chunk)) = response.chunk().await else {
            break;
        };
        let remaining = MAX_DISCOVERY_ERROR_BYTES - prefix.len();
        prefix.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
        if chunk.len() >= remaining {
            break;
        }
    }
    let body = String::from_utf8_lossy(&prefix);
    sanitize_discovery_error(&body)
}

#[derive(Debug, Deserialize)]
struct NanModelsResponse {
    data: Vec<NanModel>,
}

#[derive(Debug, Deserialize)]
struct NanModel {
    id: String,
}
