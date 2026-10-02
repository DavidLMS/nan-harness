use super::{ApiError, OverloadSource};
use axum::http::StatusCode;
use axum::response::IntoResponse;

#[tokio::test]
async fn preserves_only_exact_http_overload_without_provider_message() {
    let error = ApiError::from_provider_response(
        StatusCode::SERVICE_UNAVAILABLE,
        r#"{"error":{"code":"server_is_overloaded","message":"PRIVATE_PROVIDER_TEXT"}}"#,
        None,
    );
    assert!(matches!(
        error,
        ApiError::ServerOverloaded(OverloadSource::Http)
    ));
    let response = error.into_response();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let bytes = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["error"]["code"], "server_is_overloaded");
    assert!(!String::from_utf8_lossy(&bytes).contains("PRIVATE_PROVIDER_TEXT"));
}

#[test]
fn rejects_status_and_envelope_lookalikes() {
    for (status, body) in [
        (
            StatusCode::BAD_REQUEST,
            r#"{"error":{"code":"server_is_overloaded"}}"#,
        ),
        (
            StatusCode::TOO_MANY_REQUESTS,
            r#"{"error":{"code":"server_is_overloaded"}}"#,
        ),
        (
            StatusCode::SERVICE_UNAVAILABLE,
            r#"{"error":{"message":"server_is_overloaded"}}"#,
        ),
        (
            StatusCode::SERVICE_UNAVAILABLE,
            r#"{"code":"server_is_overloaded"}"#,
        ),
        (
            StatusCode::SERVICE_UNAVAILABLE,
            r#"{"error":{"code":["server_is_overloaded"]}}"#,
        ),
        (StatusCode::SERVICE_UNAVAILABLE, "server_is_overloaded"),
    ] {
        let error = ApiError::from_provider_response(status, body, None);
        assert!(matches!(error, ApiError::UpstreamStatus { .. }));
        assert!(error.event_data()["error"].get("code").is_none());
    }
}

#[test]
fn retains_source_diagnostics_for_typed_overload() {
    use crate::diagnostics::{BridgeDiagnostic, BridgeDiagnosticReason, BridgeEndpoint};
    for (source, code, reason, status) in [
        (
            OverloadSource::Http,
            "NH-BRIDGE-104",
            BridgeDiagnosticReason::UpstreamStatus,
            Some(503),
        ),
        (
            OverloadSource::Stream,
            "NH-BRIDGE-105",
            BridgeDiagnosticReason::InvalidUpstreamResponse,
            None,
        ),
    ] {
        let diagnostic = BridgeDiagnostic::from_api_error(
            &ApiError::ServerOverloaded(source),
            BridgeEndpoint::Responses,
        );
        assert_eq!(diagnostic.code, code);
        assert_eq!(diagnostic.reason, reason);
        assert_eq!(diagnostic.http_status, status);
    }
}
