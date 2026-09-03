use super::parsing::{
    parse_models_response, read_bounded_models_response, read_discovery_error_prefix,
};
use crate::error::BridgeError;
use nan_harness_core::{CodingModelProfile, SecretValue, coding_models_from_provider_ids};
use reqwest::header::ACCEPT;
use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

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
    parse_models_response(&body).map_err(BridgeError::InvalidModelDiscoveryResponse)
}

#[cfg(test)]
mod tests {
    use super::super::parsing::MAX_MODELS_RESPONSE_BYTES;
    use super::discover_coding_models;
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
}
