use crate::error::BridgeError;
use nan_harness_core::model::ReasoningPolicy;
use nan_harness_core::{
    CLAUDE_AUTO_MODE_COMPATIBILITY_ALIAS, CLAUDE_AUTO_MODE_PROVIDER_MODEL_ID, CodingModelProfile,
    SecretValue, claude_gateway_model_id, coding_models_from_provider_ids,
};
use reqwest::header::ACCEPT;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::Duration;

const UNKNOWN_RELEASE_DATE: &str = "1970-01-01T00:00:00Z";
const MAX_MODELS_RESPONSE_BYTES: usize = 1024 * 1024;
const MAX_DISCOVERY_ERROR_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeModel {
    provider_id: String,
    gateway_id: String,
    display_name: String,
    max_input_tokens: u64,
    max_output_tokens: u64,
    reasoning: ReasoningPolicy,
}

impl ClaudeModel {
    #[must_use]
    pub fn provider_id(&self) -> &str {
        &self.provider_id
    }

    #[must_use]
    pub fn gateway_id(&self) -> &str {
        &self.gateway_id
    }

    #[must_use]
    pub const fn max_output_tokens(&self) -> u64 {
        self.max_output_tokens
    }

    #[must_use]
    pub const fn reasoning(&self) -> ReasoningPolicy {
        self.reasoning
    }

    fn api_model(&self) -> AnthropicModel {
        AnthropicModel {
            id: self.gateway_id.clone(),
            model_type: "model",
            created_at: UNKNOWN_RELEASE_DATE,
            display_name: self.display_name.clone(),
            max_input_tokens: self.max_input_tokens,
            max_tokens: self.max_output_tokens,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ClaudeModelCatalog {
    models: Vec<ClaudeModel>,
    by_gateway_id: BTreeMap<String, usize>,
    default_index: usize,
}

impl ClaudeModelCatalog {
    /// Discovers the models available to one NaN credential and keeps only the
    /// models supported by the Claude Code bridge.
    ///
    /// # Errors
    ///
    /// Returns [`BridgeError`] when discovery fails, the response is invalid,
    /// no compatible model is available, or the selected default is unavailable.
    pub async fn discover(
        provider_base_url: &str,
        provider_api_key: Arc<SecretValue>,
        default_provider_id: &str,
    ) -> Result<Self, BridgeError> {
        let models = discover_coding_models(provider_base_url, provider_api_key).await?;
        Self::from_models(models, default_provider_id)
    }

    /// Builds the Claude-compatible catalog from provider model IDs.
    ///
    /// # Errors
    ///
    /// Returns [`BridgeError`] when no supported model is present or the
    /// selected default model is not part of the resulting catalog.
    pub fn from_provider_ids(
        provider_ids: impl IntoIterator<Item = String>,
        default_provider_id: &str,
    ) -> Result<Self, BridgeError> {
        Self::from_models(
            coding_models_from_provider_ids(provider_ids),
            default_provider_id,
        )
    }

    /// Builds the Claude-compatible catalog from an already filtered catalog.
    ///
    /// # Errors
    ///
    /// Returns [`BridgeError`] when no compatible model is present or the
    /// selected default model is not part of the catalog.
    pub fn from_models(
        profiles: impl IntoIterator<Item = CodingModelProfile>,
        default_provider_id: &str,
    ) -> Result<Self, BridgeError> {
        let models = profiles
            .into_iter()
            .map(|profile| ClaudeModel {
                gateway_id: claude_gateway_model_id(&profile.id),
                provider_id: profile.id,
                display_name: profile.display_name,
                max_input_tokens: profile.context_window,
                max_output_tokens: profile.max_output_tokens,
                reasoning: profile.reasoning,
            })
            .collect::<Vec<_>>();
        if models.is_empty() {
            return Err(BridgeError::NoCompatibleModels);
        }
        let Some(default_index) = models
            .iter()
            .position(|model| model.provider_id == default_provider_id)
        else {
            return Err(BridgeError::SelectedModelUnavailable {
                model: default_provider_id.to_owned(),
                available: models
                    .iter()
                    .map(|model| model.provider_id.clone())
                    .collect(),
            });
        };
        let mut by_gateway_id = models
            .iter()
            .enumerate()
            .map(|(index, model)| (model.gateway_id.clone(), index))
            .collect::<BTreeMap<_, _>>();
        if default_provider_id == CLAUDE_AUTO_MODE_PROVIDER_MODEL_ID {
            by_gateway_id.insert(
                CLAUDE_AUTO_MODE_COMPATIBILITY_ALIAS.to_owned(),
                default_index,
            );
        }
        Ok(Self {
            models,
            by_gateway_id,
            default_index,
        })
    }

    #[must_use]
    pub fn default_model(&self) -> &ClaudeModel {
        &self.models[self.default_index]
    }

    #[must_use]
    pub fn resolve(&self, gateway_id: &str) -> Option<&ClaudeModel> {
        self.by_gateway_id
            .get(gateway_id)
            .map(|index| &self.models[*index])
            .or_else(|| is_claude_default_model(gateway_id).then(|| self.default_model()))
    }

    #[must_use]
    pub fn gateway_ids(&self) -> Vec<String> {
        self.models
            .iter()
            .map(|model| model.gateway_id.clone())
            .collect()
    }

    pub(crate) fn api_response(&self) -> AnthropicModelsResponse {
        let data = self.models.iter().map(ClaudeModel::api_model).collect();
        AnthropicModelsResponse {
            first_id: self.models.first().map(|model| model.gateway_id.clone()),
            last_id: self.models.last().map(|model| model.gateway_id.clone()),
            data,
            has_more: false,
        }
    }
}

/// Discovers and classifies the conversational models available to one NaN credential.
///
/// Known non-conversational endpoints are removed. Unknown IDs remain available with
/// conservative metadata so newly released text models work before the next harness release.
///
/// # Errors
///
/// Returns [`BridgeError`] when the model endpoint cannot be queried or decoded.
pub async fn discover_coding_models(
    provider_base_url: &str,
    provider_api_key: Arc<SecretValue>,
) -> Result<Vec<CodingModelProfile>, BridgeError> {
    let provider_ids = discover_provider_ids(provider_base_url, provider_api_key).await?;
    Ok(coding_models_from_provider_ids(provider_ids))
}

async fn discover_provider_ids(
    provider_base_url: &str,
    provider_api_key: Arc<SecretValue>,
) -> Result<BTreeSet<String>, BridgeError> {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(BridgeError::BuildClient)?;
    let endpoint = format!("{}/models", provider_base_url.trim_end_matches('/'));
    let request = provider_api_key.with_secret(|api_key| {
        client
            .get(endpoint)
            .header(ACCEPT, "application/json")
            .bearer_auth(api_key)
    });
    let mut response = request
        .send()
        .await
        .map_err(BridgeError::ModelDiscoveryTransport)?;
    let status = response.status();
    if !status.is_success() {
        let message = read_discovery_error_prefix(&mut response).await;
        return Err(BridgeError::ModelDiscoveryStatus { status, message });
    }
    let body = read_bounded_models_response(&mut response).await?;
    let payload = serde_json::from_slice::<NanModelsResponse>(&body)
        .map_err(BridgeError::InvalidModelDiscoveryResponse)?;
    Ok(payload.data.into_iter().map(|model| model.id).collect())
}

async fn read_bounded_models_response(
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

async fn read_discovery_error_prefix(response: &mut reqwest::Response) -> String {
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

fn is_claude_default_model(model: &str) -> bool {
    matches!(model, "default" | "sonnet" | "opus" | "haiku" | "opusplan")
        || model.starts_with("claude-")
}

#[derive(Debug, Deserialize)]
struct NanModelsResponse {
    data: Vec<NanModel>,
}

#[derive(Debug, Deserialize)]
struct NanModel {
    id: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct AnthropicModelsResponse {
    data: Vec<AnthropicModel>,
    first_id: Option<String>,
    has_more: bool,
    last_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct AnthropicModel {
    id: String,
    #[serde(rename = "type")]
    model_type: &'static str,
    created_at: &'static str,
    display_name: String,
    max_input_tokens: u64,
    max_tokens: u64,
}

fn sanitize_discovery_error(body: &str) -> String {
    let parsed = serde_json::from_str::<serde_json::Value>(body).unwrap_or_default();
    let raw = parsed
        .pointer("/error/message")
        .or_else(|| parsed.get("message"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("NaN model discovery failed");
    raw.replace(['\r', '\n'], " ").chars().take(300).collect()
}

#[cfg(test)]
mod tests {
    use super::{
        ClaudeModel, ClaudeModelCatalog, MAX_MODELS_RESPONSE_BYTES, discover_coding_models,
    };
    use crate::BridgeError;
    use axum::Router;
    use axum::body::{Body, Bytes};
    use axum::extract::State;
    use axum::http::{HeaderValue, Response, StatusCode, header};
    use axum::routing::get;
    use nan_harness_core::SecretValue;
    use std::convert::Infallible;
    use std::sync::Arc;

    #[derive(Clone)]
    struct CatalogResponse {
        status: StatusCode,
        chunks: Vec<Bytes>,
        content_length: Option<u64>,
    }

    async fn catalog_response(State(response): State<CatalogResponse>) -> Response<Body> {
        let stream =
            futures_util::stream::iter(response.chunks.into_iter().map(Ok::<Bytes, Infallible>));
        let mut result = Response::new(Body::from_stream(stream));
        *result.status_mut() = response.status;
        if let Some(content_length) = response.content_length {
            result.headers_mut().insert(
                header::CONTENT_LENGTH,
                HeaderValue::from_str(&content_length.to_string())
                    .expect("test content length should be valid"),
            );
        }
        result
    }

    async fn discover_from(response: CatalogResponse) -> Result<Vec<String>, BridgeError> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("test provider should bind");
        let address = listener.local_addr().expect("test provider address");
        let app = Router::new()
            .route("/v1/models", get(catalog_response))
            .with_state(response);
        let task = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("test provider should serve");
        });
        let result = discover_coding_models(
            &format!("http://{address}/v1"),
            Arc::new(SecretValue::new("test-key").expect("test key should be valid")),
        )
        .await
        .map(|models| models.into_iter().map(|model| model.id).collect());
        task.abort();
        result
    }

    fn padded_catalog(size: usize) -> Vec<u8> {
        let mut body = br#"{"data":[{"id":"qwen3.6"}]}"#.to_vec();
        assert!(body.len() <= size, "requested test body is too small");
        body.resize(size, b' ');
        body
    }

    #[tokio::test]
    async fn discovery_bounds_success_and_error_bodies() {
        let small = padded_catalog(64);
        assert_eq!(
            discover_from(CatalogResponse {
                status: StatusCode::OK,
                chunks: vec![Bytes::from(small.clone())],
                content_length: Some(small.len() as u64),
            })
            .await
            .expect("small catalog should be accepted"),
            ["qwen3.6"]
        );

        let declared = discover_from(CatalogResponse {
            status: StatusCode::OK,
            chunks: vec![Bytes::from(padded_catalog(MAX_MODELS_RESPONSE_BYTES + 1))],
            content_length: Some((MAX_MODELS_RESPONSE_BYTES + 1) as u64),
        })
        .await
        .expect_err("oversized declared catalog should be rejected");
        assert!(matches!(declared, BridgeError::ModelDiscoveryTooLarge));

        let oversized = padded_catalog(MAX_MODELS_RESPONSE_BYTES + 1);
        let chunked = discover_from(CatalogResponse {
            status: StatusCode::OK,
            chunks: vec![
                Bytes::copy_from_slice(&oversized[..MAX_MODELS_RESPONSE_BYTES]),
                Bytes::copy_from_slice(&oversized[MAX_MODELS_RESPONSE_BYTES..]),
            ],
            content_length: None,
        })
        .await
        .expect_err("oversized chunked catalog should be rejected");
        assert!(matches!(chunked, BridgeError::ModelDiscoveryTooLarge));

        let invalid = discover_from(CatalogResponse {
            status: StatusCode::OK,
            chunks: vec![Bytes::from_static(b"not-json")],
            content_length: Some(8),
        })
        .await
        .expect_err("invalid catalog should be rejected");
        assert!(matches!(
            invalid,
            BridgeError::InvalidModelDiscoveryResponse(_)
        ));

        let boundary = padded_catalog(MAX_MODELS_RESPONSE_BYTES);
        assert_eq!(
            discover_from(CatalogResponse {
                status: StatusCode::OK,
                chunks: vec![Bytes::from(boundary)],
                content_length: Some(MAX_MODELS_RESPONSE_BYTES as u64),
            })
            .await
            .expect("catalog at the exact boundary should be accepted"),
            ["qwen3.6"]
        );

        let mut status_body = br#"{"message":"bounded status"}"#.to_vec();
        status_body.resize(128 * 1024, b' ');
        let status = discover_from(CatalogResponse {
            status: StatusCode::BAD_GATEWAY,
            chunks: vec![Bytes::from(status_body)],
            content_length: Some(128 * 1024),
        })
        .await
        .expect_err("non-success response should remain a status error");
        assert!(matches!(
            status,
            BridgeError::ModelDiscoveryStatus {
                status: StatusCode::BAD_GATEWAY,
                ref message,
            } if message == "bounded status"
        ));
    }

    #[test]
    fn catalog_keeps_only_qualified_claude_code_models() {
        let catalog = ClaudeModelCatalog::from_provider_ids(
            [
                "qwen3.6".to_owned(),
                "gemma4".to_owned(),
                "qwen3-embedding".to_owned(),
                "deepseek-v4-flash".to_owned(),
            ],
            "qwen3.6",
        )
        .expect("catalog should build");

        assert_eq!(
            catalog.gateway_ids(),
            [
                "anthropic/nan/qwen3.6".to_owned(),
                "anthropic/nan/deepseek-v4-flash".to_owned(),
                "anthropic/nan/gemma4".to_owned(),
            ]
        );
    }

    #[test]
    fn catalog_keeps_new_provider_models_with_generic_metadata() {
        let catalog = ClaudeModelCatalog::from_provider_ids(
            [
                "qwen3.6".to_owned(),
                "deepseek-v4-flash-0731".to_owned(),
                "whisper".to_owned(),
            ],
            "deepseek-v4-flash-0731",
        )
        .expect("new text models should be accepted provisionally");

        assert_eq!(
            catalog.gateway_ids(),
            [
                "anthropic/nan/qwen3.6".to_owned(),
                "anthropic/nan/deepseek-v4-flash-0731".to_owned(),
            ]
        );
    }

    #[test]
    fn catalog_enriches_new_nan_models_for_claude_gateway_discovery() {
        let catalog = ClaudeModelCatalog::from_provider_ids(
            ["qwen3.8-flash", "glm5.3-flash"]
                .into_iter()
                .map(str::to_owned),
            "qwen3.8-flash",
        )
        .expect("catalog should build");

        let response = catalog.api_response();
        assert_eq!(response.data[0].id, "anthropic/nan/qwen3.8-flash");
        assert_eq!(response.data[0].display_name, "NaN · Qwen 3.8 Flash");
        assert_eq!(response.data[0].max_input_tokens, 1_000_000);
        assert_eq!(response.data[1].id, "anthropic/nan/glm5.3-flash");
        assert_eq!(response.data[1].display_name, "NaN · GLM 5.3 Flash");
        assert_eq!(response.data[1].max_input_tokens, 1_000_000);
    }

    #[test]
    fn catalog_rejects_an_unavailable_default() {
        let error = ClaudeModelCatalog::from_provider_ids(["qwen3.6".to_owned()], "mimo-v2.5")
            .expect_err("default should be rejected");

        assert_eq!(error.code(), "NH-BRIDGE-005");
    }

    #[test]
    fn catalog_routes_claude_aliases_to_the_selected_default() {
        let catalog = ClaudeModelCatalog::from_provider_ids(
            ["qwen3.6".to_owned(), "mimo-v2.5".to_owned()],
            "mimo-v2.5",
        )
        .expect("catalog should build");

        assert_eq!(
            catalog.resolve("default").map(ClaudeModel::provider_id),
            Some("mimo-v2.5")
        );
        assert_eq!(
            catalog
                .resolve("claude-sonnet-4-6")
                .map(ClaudeModel::provider_id),
            Some("mimo-v2.5")
        );
        assert_eq!(
            catalog
                .resolve("claude-opus-4-6")
                .map(ClaudeModel::provider_id),
            Some("mimo-v2.5")
        );
        assert!(catalog.resolve("anthropic/untrusted-model").is_none());
    }

    #[test]
    fn catalog_routes_the_auto_mode_alias_to_qwen_when_selected() {
        let catalog = ClaudeModelCatalog::from_provider_ids(
            ["qwen3.6".to_owned(), "mimo-v2.5".to_owned()],
            "qwen3.6",
        )
        .expect("catalog should build");

        assert_eq!(
            catalog.resolve("opus").map(ClaudeModel::provider_id),
            Some("qwen3.6")
        );
    }
}
