use crate::conformance::{
    CONFORMANCE_SCHEMA_VERSION, ConformanceObservation, ConformanceObservationKind,
    ConformanceOutcome, ConformanceReport, ConformanceStatus, ReportShapeError, scenario,
    tool_result, tool_result_failed,
};
use nan_harness_core::HarnessKind;
use serde_json::json;

#[test]
fn report_serialization_is_bounded_and_safe() {
    let report = ConformanceReport {
        schema_version: CONFORMANCE_SCHEMA_VERSION,
        harness: HarnessKind::ClaudeCode,
        scenarios: vec![scenario(
            "sentinel",
            ConformanceStatus::Passed,
            std::time::Instant::now(),
        )],
        observations: vec![ConformanceObservation {
            kind: ConformanceObservationKind::InventoryDrift,
            fingerprint: "d".repeat(64),
        }],
        outcome: ConformanceOutcome::Passed,
        duration_milliseconds: 3,
    };
    report.validate_shape().expect("report should be bounded");
    let encoded = serde_json::to_string(&report).expect("report should serialize");
    assert!(encoded.contains("schemaVersion"));
    assert!(encoded.contains("durationMilliseconds"));
    assert!(!encoded.contains("prompt"));
    assert!(!encoded.contains("credential"));
    assert!(!encoded.contains("tool_calls"));
    assert!(encoded.contains("inventory-drift"));
    assert!(matches!(
        report.outcome,
        ConformanceOutcome::Passed | ConformanceOutcome::Failed
    ));
}

#[test]
fn legacy_conformance_reports_reject_observations() {
    let report = ConformanceReport {
        schema_version: 1,
        harness: HarnessKind::Hermes,
        scenarios: vec![scenario(
            "inventory",
            ConformanceStatus::Passed,
            std::time::Instant::now(),
        )],
        observations: vec![ConformanceObservation {
            kind: ConformanceObservationKind::InventoryDrift,
            fingerprint: "d".repeat(64),
        }],
        outcome: ConformanceOutcome::Passed,
        duration_milliseconds: 1,
    };
    assert!(matches!(
        report.validate_shape(),
        Err(ReportShapeError::LegacyObservations)
    ));
}

#[test]
fn tool_result_supports_plain_and_content_block_array_results() {
    let requests = vec![
        json!({
            "messages": [{
                "role": "tool",
                "tool_call_id": "call_nan_harness_conformance_0",
                "content": "plain result"
            }]
        }),
        json!({
            "messages": [{
                "role": "tool",
                "tool_call_id": "call-nan-harness-conformance-1",
                "content": [
                    {"type": "text", "text": "first block"},
                    {"type": "text", "text": "second block"}
                ]
            }]
        }),
    ];

    assert_eq!(
        tool_result(&requests, "callnan_harness_conformance0").as_deref(),
        Some("plain result")
    );
    assert_eq!(
        tool_result(&requests, "call_nan_harness_conformance_1").as_deref(),
        Some("first block\nsecond block")
    );
}

#[test]
fn tool_result_returns_none_for_a_missing_identifier() {
    let requests = vec![json!({
        "messages": [{
            "role": "tool",
            "tool_call_id": "call_nan_harness_conformance_0",
            "content": "result"
        }]
    })];

    assert_eq!(tool_result(&requests, "missing"), None);
}

#[test]
fn tool_result_failed_accepts_quoted_and_unquoted_error_text() {
    assert!(tool_result_failed("ERROR: tool failed"));
    assert!(tool_result_failed(r#""error: tool failed""#));
    assert!(tool_result_failed(
        "<system>ERROR: tool failed</system>\nThe file must be read first."
    ));
    assert!(tool_result_failed(r#"{"isError":true}"#));
    assert!(!tool_result_failed("tool completed successfully"));
}
