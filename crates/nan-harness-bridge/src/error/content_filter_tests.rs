use super::ApiError;
use crate::{BridgeDiagnostic, BridgeDiagnosticReason, BridgeEndpoint};
use axum::http::StatusCode;
use serde_json::json;

const REJECTION: &str = "Input text data may contain inappropriate content.";

#[test]
fn content_filter_rejection_has_actionable_copy_and_a_provider_code() {
    let body = json!({"error": {
        "code": "400", "message": REJECTION, "param": "None", "type": "None",
        "private_detail": "synthetic-private-marker"
    }});
    let error = ApiError::from_provider_response(
        StatusCode::BAD_REQUEST,
        &body.to_string(),
        Some("qwen3.8-flash"),
    );
    assert_eq!(error.code(), "NH-PROVIDER-CONTENT-FILTERED");
    assert_eq!(error.status(), StatusCode::BAD_REQUEST);
    assert_eq!(error.anthropic_type(), "invalid_request_error");
    assert_eq!(
        error.to_string(),
        "Qwen's guardrails rejected this request. This may be a false positive. Try another model."
    );
    let event = error.event_data().to_string();
    assert!(!event.contains(REJECTION));
    assert!(!event.contains("synthetic-private-marker"));
    let diagnostic = BridgeDiagnostic::from_api_error(&error, BridgeEndpoint::Responses);
    assert_eq!(
        diagnostic.reason,
        BridgeDiagnosticReason::ProviderContentFiltered
    );
    assert_eq!(diagnostic.http_status, Some(400));
    assert_eq!(diagnostic.code, error.code());
    assert_eq!(diagnostic.model_id, None);
}

#[test]
fn content_filter_copy_does_not_misidentify_other_models() {
    for model in [
        None,
        Some("deepseek-v4-flash"),
        Some("synthetic-private-model"),
    ] {
        let error = ApiError::from_provider_response(
            StatusCode::BAD_REQUEST,
            &json!({"message": REJECTION}).to_string(),
            model,
        );
        assert_eq!(error.code(), "NH-PROVIDER-CONTENT-FILTERED");
        assert!(
            error
                .to_string()
                .starts_with("The selected model's guardrails rejected this request.")
        );
        assert!(!error.to_string().contains("Qwen"));
        assert!(!error.to_string().contains("synthetic-private-model"));
    }
}

#[test]
fn content_filter_classification_requires_the_known_status_and_message() {
    let cases = [
        (
            StatusCode::FORBIDDEN,
            json!({"error": {"message": REJECTION}}),
        ),
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            json!({"error": {"message": REJECTION}}),
        ),
        (
            StatusCode::BAD_REQUEST,
            json!({"error": {"message": "Invalid tool arguments"}}),
        ),
        (
            StatusCode::BAD_REQUEST,
            json!({"error": {"message": format!("{REJECTION} Additional unrelated content")}}),
        ),
        (
            StatusCode::BAD_REQUEST,
            json!({"error": {"message": "Content moderation configuration is invalid"}}),
        ),
        (
            StatusCode::BAD_REQUEST,
            json!({"error": {"message": 400}, "detail": REJECTION}),
        ),
    ];
    for (status, body) in cases {
        let error =
            ApiError::from_provider_response(status, &body.to_string(), Some("qwen3.8-flash"));
        assert!(matches!(error, ApiError::UpstreamStatus { .. }), "{body}");
        assert_eq!(error.code(), "NH-BRIDGE-104");
    }
}

#[test]
fn unknown_provider_errors_keep_the_bounded_existing_fallback() {
    let error = ApiError::from_provider_response(StatusCode::BAD_REQUEST, "not JSON", None);
    assert!(error.to_string().contains("NaN request failed"));
    let body = json!({"error": {"message": format!("line\r\n{}", "x".repeat(400))}});
    let ApiError::UpstreamStatus { message, .. } =
        ApiError::from_provider_response(StatusCode::BAD_REQUEST, &body.to_string(), None)
    else {
        panic!("expected the generic provider error");
    };
    assert_eq!(message.chars().count(), 300);
    assert!(!message.contains(['\r', '\n']));
}
