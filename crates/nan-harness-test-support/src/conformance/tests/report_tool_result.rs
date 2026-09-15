use crate::conformance::{
    CONFORMANCE_SCHEMA_VERSION, ConformanceObservation, ConformanceObservationKind,
    ConformanceOutcome, ConformanceReport, ConformanceStatus, InventoryFailureReason,
    InventoryProcessEvidence, InventoryProcessStatus, ReportShapeError, scenario, tool_result,
    tool_result_failed,
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
        inventory_failure_reasons: Vec::new(),
        inventory_process: None,
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
        inventory_failure_reasons: Vec::new(),
        inventory_process: None,
        outcome: ConformanceOutcome::Passed,
        duration_milliseconds: 1,
    };
    assert!(matches!(
        report.validate_shape(),
        Err(ReportShapeError::LegacyObservations)
    ));
}

#[test]
fn legacy_conformance_reports_reject_inventory_process_evidence() {
    let report = ConformanceReport {
        schema_version: 1,
        harness: HarnessKind::Codex,
        scenarios: vec![scenario(
            "inventory",
            ConformanceStatus::Failed,
            std::time::Instant::now(),
        )],
        observations: Vec::new(),
        inventory_failure_reasons: Vec::new(),
        inventory_process: Some(InventoryProcessEvidence {
            status: InventoryProcessStatus::EnvironmentError,
            exit_code: None,
            os_error_code: Some(5),
            timeout_milliseconds: None,
        }),
        outcome: ConformanceOutcome::Failed,
        duration_milliseconds: 1,
    };
    assert_eq!(
        report.validate_shape(),
        Err(ReportShapeError::LegacyInventoryProcess)
    );
}

#[test]
fn inventory_failure_reasons_are_bounded_and_safe() {
    let mut report = ConformanceReport {
        schema_version: CONFORMANCE_SCHEMA_VERSION,
        harness: HarnessKind::Fx,
        scenarios: vec![scenario(
            "inventory",
            ConformanceStatus::Failed,
            std::time::Instant::now(),
        )],
        observations: Vec::new(),
        inventory_failure_reasons: vec![
            InventoryFailureReason::ProcessFailed,
            InventoryFailureReason::MarkerMissing,
        ],
        inventory_process: None,
        outcome: ConformanceOutcome::Failed,
        duration_milliseconds: 0,
    };
    report.validate_shape().expect("reasons should validate");
    let encoded = serde_json::to_string(&report).expect("reasons should serialize");
    assert!(encoded.contains("inventoryFailureReasons"));
    assert!(encoded.contains("process-failed"));
    report
        .inventory_failure_reasons
        .push(InventoryFailureReason::ProcessFailed);
    assert_eq!(
        report.validate_shape(),
        Err(ReportShapeError::DuplicateInventoryFailureReason)
    );
}

#[test]
fn inventory_process_evidence_is_closed_and_bounded() {
    let mut report = ConformanceReport {
        schema_version: CONFORMANCE_SCHEMA_VERSION,
        harness: HarnessKind::Codex,
        scenarios: vec![scenario(
            "inventory",
            ConformanceStatus::Failed,
            std::time::Instant::now(),
        )],
        observations: Vec::new(),
        inventory_failure_reasons: vec![InventoryFailureReason::ProcessFailed],
        inventory_process: Some(InventoryProcessEvidence {
            status: InventoryProcessStatus::NonzeroExit,
            exit_code: Some(-1_073_741_819),
            os_error_code: None,
            timeout_milliseconds: None,
        }),
        outcome: ConformanceOutcome::Failed,
        duration_milliseconds: 0,
    };
    report
        .validate_shape()
        .expect("nonzero process evidence should validate");
    let encoded = serde_json::to_string(&report).expect("process evidence should serialize");
    assert!(encoded.contains("inventoryProcess"));
    assert!(encoded.contains("nonzero-exit"));
    report.inventory_process = Some(InventoryProcessEvidence {
        status: InventoryProcessStatus::Completed,
        exit_code: Some(23),
        os_error_code: None,
        timeout_milliseconds: None,
    });
    assert_eq!(
        report.validate_shape(),
        Err(ReportShapeError::InventoryProcess)
    );
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
