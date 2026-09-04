use nan_harness_telemetry::consent::ReportConsent;
use nan_harness_telemetry::diagnostic::{
    BridgeEndpoint, Diagnostic, DiagnosticDetails, DiagnosticReason,
};
use nan_harness_telemetry::event::{
    ErrorReport, ErrorReportContext, Failure, FailureCategory, FailureStage, HarnessIdentity,
    HarnessKind, REOPEN_TERMINAL_GUIDANCE_TEXT, UserGuidance,
};
use nan_harness_telemetry::redaction::{RedactionError, sanitize};
use serde_json::Value;

use crate::{context, installation_id, report};

#[test]
fn generated_reports_validate_against_the_published_contract() {
    let report = report(false);
    let value = serde_json::to_value(&report).expect("report should serialize");
    let schema: Value = serde_json::from_str(include_str!(
        "../../../../tests/telemetry/error-report.schema.json"
    ))
    .expect("error report schema should parse");
    let validator = jsonschema::validator_for(&schema).expect("schema should compile");

    assert!(validator.is_valid(&value));
    assert_eq!(value["schemaVersion"], 4);
    assert!(value["installationId"].as_str().is_some());
    assert_eq!(value["diagnostic"]["reason"], "invalid-response");
    assert_eq!(value["application"]["name"], "nan-harness");
}

#[test]
fn approved_user_guidance_is_preserved_in_the_report_contract() {
    let report = ErrorReport::new(
        context(false).with_user_guidance(UserGuidance::reopen_terminal(true)),
        ReportConsent::automatic(),
        installation_id(),
    )
    .expect("report should build");
    let value = serde_json::to_value(&report).expect("report should serialize");
    let schema: Value = serde_json::from_str(include_str!(
        "../../../../tests/telemetry/error-report.schema.json"
    ))
    .expect("error report schema should parse");

    assert_eq!(value["userGuidance"]["classification"], "environmental");
    assert_eq!(value["userGuidance"]["id"], "reopen-terminal");
    assert_eq!(value["userGuidance"]["shown"], true);
    assert_eq!(value["userGuidance"]["locale"], "en");
    assert_eq!(value["userGuidance"]["version"], 1);
    assert_eq!(value["userGuidance"]["text"], REOPEN_TERMINAL_GUIDANCE_TEXT);
    assert!(
        jsonschema::validator_for(&schema)
            .expect("error report schema should compile")
            .is_valid(&value)
    );

    sanitize(report).expect("approved guidance should pass the privacy allowlist");
}

#[test]
fn unapproved_user_guidance_is_rejected_before_export() {
    let report = ErrorReport::new(
        context(false).with_user_guidance(UserGuidance::reopen_terminal(true)),
        ReportConsent::automatic(),
        installation_id(),
    )
    .expect("report should build");
    let mut value = serde_json::to_value(&report).expect("report should serialize");
    value["userGuidance"]["text"] = Value::String("/Users/private/project".to_owned());
    let report: ErrorReport = serde_json::from_value(value).expect("report should deserialize");

    assert_eq!(
        sanitize(report).expect_err("dynamic guidance must be rejected"),
        RedactionError::ForbiddenValue {
            field: "userGuidance"
        }
    );
}

#[test]
fn version_one_pending_reports_remain_readable_after_the_contract_upgrade() {
    let mut value = serde_json::to_value(report(false)).expect("report should serialize");
    value["schemaVersion"] = serde_json::json!(1);
    let report = value.as_object_mut().expect("report should be an object");
    report.remove("operation");
    report.remove("installationId");
    report.remove("diagnostic");
    value["application"]
        .as_object_mut()
        .expect("application should be an object")
        .remove("buildCommit");
    value["failure"]
        .as_object_mut()
        .expect("failure should be an object")
        .remove("cause");
    value["runtime"]
        .as_object_mut()
        .expect("runtime should be an object")
        .remove("targetEnvironment");
    let report: ErrorReport = serde_json::from_value(value).expect("v1 report should deserialize");

    sanitize(report).expect("v1 report should remain valid");
}

#[test]
fn forbidden_metadata_is_rejected_before_an_exporter_can_receive_it() {
    let context = ErrorReportContext::new(
        Failure::new(
            "NH-TEST-001",
            FailureCategory::Internal,
            FailureStage::Startup,
            false,
        ),
        false,
    )
    .with_diagnostic(Diagnostic::general(DiagnosticReason::InvalidConfiguration))
    .with_harness(HarnessIdentity::new(
        HarnessKind::ClaudeCode,
        Some("/Users/private/project".to_owned()),
    ));
    let report = ErrorReport::new(context, ReportConsent::automatic(), installation_id())
        .expect("report should build");

    assert_eq!(
        sanitize(report).expect_err("path-like metadata must be rejected"),
        RedactionError::ForbiddenValue {
            field: "harness.version"
        }
    );
}

#[test]
fn path_like_model_context_is_rejected_before_export() {
    let context = ErrorReportContext::new(
        Failure::new(
            "NH-TEST-001",
            FailureCategory::Bridge,
            FailureStage::RequestTranslation,
            false,
        ),
        false,
    )
    .with_diagnostic(Diagnostic::new(
        DiagnosticReason::InvalidRequest,
        DiagnosticDetails::Bridge {
            endpoint: BridgeEndpoint::Responses,
            model_id: Some("/Users/private/model".to_owned()),
            requested_reasoning: None,
            model_policy: None,
            timeout_phase: None,
            recovery_outcome: None,
            attempt: None,
            priority: None,
            cache_replay_detected: None,
            cache_bypass_attempted: None,
        },
    ));
    let report = ErrorReport::new(context, ReportConsent::automatic(), installation_id())
        .expect("report should build");

    assert_eq!(
        sanitize(report).expect_err("path-like model IDs must be rejected"),
        RedactionError::ForbiddenValue {
            field: "diagnostic.details.modelId"
        }
    );
}

#[test]
fn non_panic_reports_require_an_actionable_classification() {
    let context = ErrorReportContext::new(
        Failure::new(
            "NH-TEST-001",
            FailureCategory::Internal,
            FailureStage::Startup,
            false,
        ),
        false,
    );
    let report = ErrorReport::new(context, ReportConsent::automatic(), installation_id())
        .expect("report should build");

    assert_eq!(
        sanitize(report).expect_err("unclassified failures must be rejected"),
        RedactionError::UnclassifiedFailure
    );
}
